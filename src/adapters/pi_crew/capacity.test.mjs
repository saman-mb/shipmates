import assert from "node:assert/strict";
import test from "node:test";
import {
  applyGrant,
  planAdmission,
  reconcileLedger,
  releaseReservation,
} from "./capacity.mjs";

const MODEL_A = "engine/model-a";
const MODEL_B = "engine/model-b";

function request(over = {}) {
  return {
    runId: "run-1",
    sessionId: "session-a",
    role: "senior-engineer",
    model: MODEL_A,
    pinned: false,
    contextTokens: 100,
    outputReserveTokens: 20,
    priority: 1,
    enqueuedAtMs: 10,
    waiting: [],
    ...over,
  };
}

function model(over = {}) {
  return {
    id: MODEL_A,
    residentBytes: 800,
    configuredContextTokens: 1000,
    slots: 2,
    slotsInUse: 0,
    loaded: true,
    ...over,
  };
}

function snapshot(over = {}) {
  return {
    observedAtMs: 1,
    confidence: "measured",
    pressure: "ok",
    pools: [{ id: "pool-host", availableBytes: 1000 }],
    models: [model()],
    headroomBytes: 50,
    ...over,
  };
}

function ledger(reservations = []) {
  return { reservations };
}

function hold(over = {}) {
  return {
    runId: "run-held",
    modelId: MODEL_A,
    pendingBytes: 80,
    contextTokens: 80,
    slots: 1,
    visible: false,
    ...over,
  };
}

test("shared-weight charges a loaded resident once and an unloaded resident once", () => {
  const loaded = snapshot({
    pools: [{ id: "pool-host", availableBytes: 300 }],
    models: [model({ residentBytes: 800, loaded: true, slots: 2 })],
    headroomBytes: 0,
  });
  const first = planAdmission(request({ runId: "run-a", contextTokens: 100, outputReserveTokens: 0 }), loaded, ledger());
  assert.equal(first.code, "grant");
  assert.equal(first.reservation.pendingBytes, 100);
  assert.equal(first.reservation.modelId, MODEL_A);
  const afterFirst = applyGrant(ledger(), first);
  const second = planAdmission(
    request({ runId: "run-b", contextTokens: 100, outputReserveTokens: 0, enqueuedAtMs: 11 }),
    loaded,
    afterFirst,
  );
  assert.equal(second.code, "grant");
  assert.equal(second.reservation.pendingBytes, 100);

  const unloaded = snapshot({
    pools: [{ id: "pool-host", availableBytes: 1000 }],
    models: [model({ residentBytes: 800, loaded: false, slots: 3 })],
    headroomBytes: 0,
  });
  const load = planAdmission(request({ runId: "run-c", contextTokens: 100, outputReserveTokens: 0 }), unloaded, ledger());
  assert.equal(load.code, "grant");
  assert.equal(load.reservation.pendingBytes, 900);
  const sharing = planAdmission(
    request({ runId: "run-d", contextTokens: 100, outputReserveTokens: 0 }),
    unloaded,
    applyGrant(ledger(), load),
  );
  assert.equal(sharing.code, "grant");
  assert.equal(sharing.reservation.pendingBytes, 100);
  const overflow = planAdmission(
    request({ runId: "run-e", contextTokens: 100, outputReserveTokens: 0 }),
    unloaded,
    applyGrant(applyGrant(ledger(), load), sharing),
  );
  assert.equal(overflow.code, "queued-capacity");
});

test("unknown-telemetry allows at most one not-yet-visible reservation", () => {
  const unknown = snapshot({ confidence: "unknown" });
  const open = planAdmission(request(), unknown, ledger());
  assert.equal(open.code, "grant");

  const blocked = planAdmission(request({ runId: "run-2" }), unknown, ledger([hold()]));
  assert.equal(blocked.action, "queue");
  assert.equal(blocked.code, "queued-unknown");
  assert.equal(blocked.queuePosition, 1);

  const visibleOnly = planAdmission(
    request({ runId: "run-3" }),
    unknown,
    ledger([hold({ visible: true, pendingBytes: 0 })]),
  );
  assert.equal(visibleOnly.code, "grant");

  const oneLoad = planAdmission(
    request({ runId: "run-4", contextTokens: 50, outputReserveTokens: 0 }),
    snapshot({
      confidence: "unknown",
      headroomBytes: 0,
      pools: [{ id: "pool-host", availableBytes: 900 }],
      models: [model({ loaded: false, residentBytes: 800 })],
    }),
    ledger(),
  );
  assert.equal(oneLoad.code, "grant");
  assert.equal(oneLoad.reservation.pendingBytes, 850);
  const secondLoad = planAdmission(
    request({ runId: "run-4b", contextTokens: 10, outputReserveTokens: 0 }),
    snapshot({
      confidence: "unknown",
      headroomBytes: 0,
      pools: [{ id: "pool-host", availableBytes: 900 }],
      models: [model({ loaded: false, residentBytes: 800, slots: 2 })],
    }),
    applyGrant(ledger(), oneLoad),
  );
  assert.equal(secondLoad.code, "queued-unknown");
  assert.equal(secondLoad.reservation, null);

  for (const confidence of ["measured", "reported", "estimated"]) {
    const admitted = planAdmission(
      request({ runId: "run-5" }),
      snapshot({ confidence }),
      ledger([hold()]),
    );
    assert.equal(admitted.code, "grant", confidence);
  }
});

test("pressure queues new work and does not revoke grants", () => {
  const held = ledger([hold({ visible: true, pendingBytes: 0 })]);
  const before = structuredClone(held);
  const result = planAdmission(request(), snapshot({ pressure: "high" }), held);
  assert.equal(result.action, "queue");
  assert.equal(result.code, "queued-pressure");
  assert.equal(result.reservation, null);
  assert.deepEqual(held, before);
  assert.equal(held.reservations.length, 1);

  const later = planAdmission(request(), snapshot({ pressure: "ok" }), held);
  assert.equal(later.code, "grant");
  assert.equal(applyGrant(held, result).reservations.length, 1);
});

test("pinned-model is never substituted", () => {
  const other = model({ id: MODEL_B, residentBytes: 10, loaded: true, slots: 2 });
  const full = snapshot({
    pools: [{ id: "pool-host", availableBytes: 100 }],
    models: [model({ id: MODEL_A, residentBytes: 5000, loaded: false, slots: 1 }), other],
    headroomBytes: 0,
  });
  const pinned = planAdmission(
    request({ model: MODEL_A, pinned: true, contextTokens: 50, outputReserveTokens: 0 }),
    full,
    ledger(),
  );
  assert.equal(pinned.action, "queue");
  assert.equal(pinned.code, "queued-pin");
  assert.equal(pinned.reservation, null);
  assert.doesNotMatch(pinned.reason, new RegExp(MODEL_B));

  const missing = planAdmission(
    request({ model: MODEL_A, pinned: true }),
    snapshot({ models: [other] }),
    ledger(),
  );
  assert.equal(missing.code, "queued-pin");

  const honoured = planAdmission(
    request({ model: MODEL_B, pinned: true, contextTokens: 10, outputReserveTokens: 0 }),
    snapshot({ models: [other], headroomBytes: 0, pools: [{ id: "pool-host", availableBytes: 100 }] }),
    ledger(),
  );
  assert.equal(honoured.code, "grant");
  assert.equal(honoured.reservation.modelId, MODEL_B);

  const unseen = planAdmission(
    request({ model: MODEL_A, pinned: true, runId: "run-unseen" }),
    snapshot({ models: [], confidence: "reported" }),
    ledger(),
  );
  assert.equal(unseen.action, "grant");
  assert.equal(unseen.code, "grant");
  assert.equal(unseen.reservation.modelId, MODEL_A);
  const second = planAdmission(
    request({ model: MODEL_B, pinned: true, runId: "run-unseen-2" }),
    snapshot({ models: [], confidence: "reported" }),
    applyGrant(ledger(), unseen),
  );
  assert.equal(second.action, "queue");
  assert.equal(second.code, "queued-unknown");
});

test("headroom is subtracted from available bytes before a grant", () => {
  const tight = planAdmission(
    request({ contextTokens: 600, outputReserveTokens: 100 }),
    snapshot({
      pools: [{ id: "pool-host", availableBytes: 1000 }],
      headroomBytes: 400,
      models: [model({ slots: 1 })],
    }),
    ledger(),
  );
  assert.equal(tight.code, "queued-capacity");
  assert.equal(tight.queuePosition, 1);

  const fits = planAdmission(
    request({ contextTokens: 600, outputReserveTokens: 100 }),
    snapshot({
      pools: [
        { id: "pool-host", availableBytes: 400 },
        { id: "pool-device", availableBytes: 1000 },
      ],
      headroomBytes: 200,
      models: [model({ slots: 1 })],
    }),
    ledger(),
  );
  assert.equal(fits.code, "grant");
  assert.equal(fits.reservation.pendingBytes, 700);
  assert.equal(fits.queuePosition, null);
});

test("reconcile marks visible consumption once and releases by run id", () => {
  const held = ledger([
    hold({ runId: "run-a", pendingBytes: 900, contextTokens: 100, visible: false }),
    hold({ runId: "run-b", modelId: MODEL_A, pendingBytes: 100, contextTokens: 100, visible: false }),
  ]);
  const seen = snapshot({
    pools: [{ id: "pool-host", availableBytes: 200 }],
    models: [model({ residentBytes: 5000, loaded: true, slots: 2, slotsInUse: 2 })],
    headroomBytes: 0,
  });
  const reconciled = reconcileLedger(held, seen);
  assert.notEqual(reconciled, held);
  assert.equal(held.reservations[0].visible, false);
  assert.equal(held.reservations[0].pendingBytes, 900);
  assert.equal(reconciled.reservations.every((row) => row.visible), true);
  assert.equal(reconciled.reservations.every((row) => row.pendingBytes === 0), true);

  const next = planAdmission(
    request({ runId: "run-c", contextTokens: 100, outputReserveTokens: 0 }),
    seen,
    reconciled,
  );
  assert.equal(next.code, "queued-capacity");

  const withRoom = snapshot({
    pools: [{ id: "pool-host", availableBytes: 200 }],
    models: [model({ residentBytes: 5000, loaded: true, slots: 4, slotsInUse: 2 })],
    headroomBytes: 0,
  });
  const granted = planAdmission(
    request({ runId: "run-c", contextTokens: 100, outputReserveTokens: 0 }),
    withRoom,
    reconciled,
  );
  assert.equal(granted.code, "grant");
  assert.equal(granted.reservation.pendingBytes, 100);

  const released = releaseReservation(reconciled, "run-a");
  assert.equal(reconciled.reservations.length, 2);
  assert.deepEqual(released.reservations.map((row) => row.runId), ["run-b"]);
});

test("queue position is 1-based among waiting jobs plus this job", () => {
  const result = planAdmission(
    request({
      runId: "run-new",
      priority: 1,
      enqueuedAtMs: 30,
      contextTokens: 900,
      outputReserveTokens: 50,
      waiting: [
        { runId: "run-hot", priority: 5, enqueuedAtMs: 25 },
        { runId: "run-later", priority: 1, enqueuedAtMs: 40 },
      ],
    }),
    snapshot({ models: [model({ configuredContextTokens: 10 })] }),
    ledger(),
  );
  assert.equal(result.code, "queued-capacity");
  assert.equal(result.queuePosition, 2);
});

test("invalid input is refused and does not mutate the ledger", () => {
  const held = ledger();
  const result = planAdmission(request({ runId: "" }), snapshot(), held);
  assert.equal(result.action, "queue");
  assert.equal(result.code, "refused-invalid");
  assert.equal(result.queuePosition, null);
  assert.deepEqual(held, ledger());
  assert.equal(planAdmission(request(), snapshot({ confidence: "guessed" }), held).code, "refused-invalid");
  assert.equal(planAdmission(request(), snapshot({ pressure: "low" }), held).code, "refused-invalid");
  assert.equal(planAdmission(request(), snapshot(), ledger([hold({ runId: "run-1" })])).code, "refused-invalid");
});
