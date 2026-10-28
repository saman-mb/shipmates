import assert from "node:assert/strict";
import { existsSync, mkdtempSync, rmSync } from "node:fs";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { createBroker, ipcListenPath, startBroker } from "./broker.mjs";
import { createFrameParser, createLinkedPair, createMessageConnection, createRpcClient, encodeMessage } from "./protocol.mjs";

const MODEL = "engine/model-a";
const JOURNAL_KEYS = new Set([
  "type",
  "atMs",
  "runId",
  "modelId",
  "generation",
  "pendingBytes",
  "contextTokens",
  "outputReserveTokens",
  "slots",
  "queuePosition",
  "enqueuedAtMs",
  "grantedAtMs",
  "expiresAtMs",
  "releasedAtMs",
]);

function snapshot(over = {}) {
  return {
    observedAtMs: 1_000,
    confidence: "unknown",
    pressure: "ok",
    pools: [{ id: "pool-a", availableBytes: 8_000_000_000 }],
    models: [
      {
        id: MODEL,
        residentBytes: 1_000,
        configuredContextTokens: 8192,
        slots: 1,
        slotsInUse: 0,
        loaded: true,
      },
    ],
    headroomBytes: 0,
    ...over,
  };
}

function admitParams(runId, over = {}) {
  return {
    runId,
    sessionId: "session-do-not-store",
    role: "senior-engineer",
    model: MODEL,
    pinned: true,
    contextTokens: 128,
    outputReserveTokens: 32,
    priority: 1,
    enqueuedAtMs: 5_000,
    prompt: "do-not-store this prompt",
    credential: "do-not-store-credential",
    ...over,
  };
}

function harness(options = {}) {
  const journal = [];
  let current = snapshot(options.snapshot);
  const broker = createBroker({
    probe: async () => current,
    journal: { append: (entry) => journal.push(entry) },
    now: options.now ?? (() => 5_000),
    leaseMs: options.leaseMs ?? 60_000,
    connection: options.connection,
  });
  return {
    broker,
    journal,
    setSnapshot(next) {
      current = next;
    },
  };
}

function pair(broker) {
  const linked = createLinkedPair();
  broker.attach(linked.server);
  return createRpcClient(linked.client);
}

async function delay(ms) {
  await new Promise((resolve) => setTimeout(resolve, ms));
}

async function until(fn, message) {
  const start = Date.now();
  for (;;) {
    const value = await fn();
    if (value) return value;
    if (Date.now() - start > 1_000) throw new Error(message);
    await delay(10);
  }
}

function assertJournal(journal) {
  const encoded = JSON.stringify(journal);
  assert.equal(encoded.includes("do-not-store"), false);
  assert.equal(encoded.includes("session-do-not-store"), false);
  assert.equal(encoded.includes("prompt"), false);
  assert.equal(encoded.includes("credential"), false);
  for (const entry of journal) {
    for (const key of Object.keys(entry)) assert.equal(JOURNAL_KEYS.has(key), true, key);
    if (entry.modelId !== undefined) assert.equal(entry.modelId, MODEL);
  }
}

test("Content-Length framing round-trips split chunks and unicode bodies", () => {
  const first = { jsonrpc: "2.0", id: 1, method: "status", params: { note: "é" } };
  const second = { jsonrpc: "2.0", id: 2, result: { ok: true } };
  const encoded = Buffer.concat([encodeMessage(first), encodeMessage(second)]);
  const headerEnd = encoded.indexOf("\r\n\r\n");
  const length = Number(encoded.subarray(0, headerEnd).toString("ascii").split(":")[1]);
  assert.equal(length, Buffer.byteLength(JSON.stringify(first), "utf8"));

  const seen = [];
  const parser = createFrameParser(
    (message) => seen.push(message),
    (error) => {
      throw error;
    },
  );
  for (const byte of encoded) parser.push(Buffer.from([byte]));
  assert.deepEqual(seen, [first, second]);
});

test("a frame without Content-Length is rejected", () => {
  let failed = false;
  const parser = createFrameParser(
    () => {
      throw new Error("parsed a bad frame");
    },
    () => {
      failed = true;
    },
  );
  parser.push(Buffer.from("Content-Type: application/vscode-jsonrpc; charset=utf-8\r\n\r\n{}"));
  assert.equal(failed, true);
});

test("a pinned admit with no observed engine grants instead of parking", async () => {
  const { broker } = harness({
    snapshot: {
      observedAtMs: 1_000,
      confidence: "reported",
      pressure: "ok",
      pools: [{ id: "host", availableBytes: 100_000_000_000 }],
      models: [],
      headroomBytes: 0,
    },
  });
  const client = pair(broker);
  const grant = await Promise.race([
    client.request("admit", admitParams("run-unseen", { wait: true, contextTokens: 0, outputReserveTokens: 0 })),
    new Promise((_, reject) => setTimeout(() => reject(new Error("parked")), 500)),
  ]);
  assert.equal(grant.action, "grant");
  assert.equal(grant.lease.modelId, MODEL);
  const second = pair(broker);
  const queued = second.request("admit", admitParams("run-unseen-2", { wait: true, model: "engine/model-b" }));
  const status = await until(async () => {
    const current = await client.request("status", {});
    return current.queue.length === 1 ? current : null;
  }, "expected the second unseen seat to queue");
  assert.equal(status.reservations.length, 1);
  await client.request("release", { runId: "run-unseen", generation: grant.generation });
  const followed = await Promise.race([
    queued,
    new Promise((_, reject) => setTimeout(() => reject(new Error("second seat stayed parked")), 500)),
  ]);
  assert.equal(followed.action, "grant");
});

test("two concurrent clients: one reservation wins and the queued client proceeds after release", async () => {
  const { broker, journal } = harness();
  const watcher = pair(broker);
  const clientA = pair(broker);
  const clientB = pair(broker);
  const pending = {
    "run-a": clientA.request("admit", admitParams("run-a", { enqueuedAtMs: 5_000 })),
    "run-b": clientB.request("admit", admitParams("run-b", { enqueuedAtMs: 5_001 })),
  };

  const split = await until(async () => {
    const current = await watcher.request("status", {});
    if (current.reservations.length === 1 && current.queue.length === 1) return current;
    return null;
  }, "expected one grant and one queued client");

  const winnerId = split.reservations[0].runId;
  const queuedId = split.queue[0].runId;
  assert.notEqual(winnerId, queuedId);
  const winner = winnerId === "run-a" ? clientA : clientB;
  const grant = await pending[winnerId];
  assert.equal(grant.action, "grant");
  assert.equal(grant.code, "grant");
  assert.equal(Number.isInteger(grant.generation), true);
  assert.equal(grant.lease.runId, winnerId);
  assert.equal(grant.lease.modelId, MODEL);

  let queuedSettled = false;
  pending[queuedId].then(
    () => {
      queuedSettled = true;
    },
    () => {
      queuedSettled = true;
    },
  );
  await delay(30);
  assert.equal(queuedSettled, false);

  await assert.rejects(
    winner.request("release", { runId: winnerId, generation: grant.generation + 1 }),
    (error) => error.code === -32001,
  );
  await delay(20);
  assert.equal(queuedSettled, false);

  const released = await winner.request("release", { runId: winnerId, generation: grant.generation });
  assert.equal(released.released, true);
  const proceeded = await pending[queuedId];
  assert.equal(proceeded.action, "grant");
  assert.equal(proceeded.lease.runId, queuedId);
  assert.notEqual(proceeded.generation, grant.generation);

  await assert.rejects(
    winner.request("release", { runId: winnerId, generation: grant.generation }),
    (error) => error.code === -32001,
  );
  const after = await watcher.request("status", {});
  assert.equal(after.reservations.length, 1);
  assert.equal(after.reservations[0].runId, queuedId);
  assert.equal(after.queue.length, 0);
  assertJournal(journal);
});

test("the same run cannot take two reservations", async () => {
  const { broker } = harness({ snapshot: { confidence: "measured" } });
  const watcher = pair(broker);
  const clientA = pair(broker);
  const clientB = pair(broker);
  const [first, second] = await Promise.all([
    clientA.request("admit", admitParams("run-same")),
    clientB.request("admit", admitParams("run-same")),
  ]);
  assert.equal(first.action, "grant");
  assert.equal(second.action, "grant");
  assert.equal(first.generation, second.generation);
  const current = await watcher.request("status", {});
  assert.equal(current.reservations.length, 1);
  assert.equal(current.leases.length, 1);
});

test("heartbeat renews a fenced lease and high pressure does not revoke it", async () => {
  let clock = 10_000;
  const { broker, journal, setSnapshot } = harness({
    snapshot: { confidence: "measured", pressure: "ok" },
    now: () => clock,
    leaseMs: 1_000,
  });
  const client = pair(broker);
  const grant = await client.request("admit", admitParams("run-heart"));
  clock = 10_400;
  const beat = await client.request("heartbeat", { runId: "run-heart", generation: grant.generation });
  assert.equal(beat.expiresAtMs, 11_400);
  setSnapshot(snapshot({ confidence: "measured", pressure: "high" }));
  const current = await client.request("status", {});
  assert.equal(current.leases.length, 1);
  assert.equal(current.leases[0].generation, grant.generation);
  const again = await client.request("heartbeat", { runId: "run-heart", generation: grant.generation });
  assert.equal(again.generation, grant.generation);
  await assert.rejects(
    client.request("heartbeat", { runId: "run-heart", generation: grant.generation - 1 }),
    (error) => error.code === -32001 || error.code === -32002,
  );
  assertJournal(journal);
});

test("cancel removes a queued client and an expired lease lets the next client proceed", async () => {
  let clock = 8_000;
  const { broker } = harness({ now: () => clock, leaseMs: 500 });
  const watcher = pair(broker);
  const holder = pair(broker);
  const queued = pair(broker);
  const cancelled = pair(broker);
  const held = holder.request("admit", admitParams("run-hold", { enqueuedAtMs: 1 }));
  const waiting = queued.request("admit", admitParams("run-wait", { enqueuedAtMs: 2 }));
  const dropped = cancelled.request("admit", admitParams("run-drop", { enqueuedAtMs: 3 }));
  await until(async () => {
    const current = await watcher.request("status", {});
    return current.reservations.length === 1 && current.queue.length === 2;
  }, "expected one holder and two queued clients");
  const grant = await held;
  const cancel = await cancelled.request("cancel", { runId: "run-drop" });
  assert.equal(cancel.cancelled, true);
  await assert.rejects(dropped, (error) => error.code === -32004);
  clock = 9_000;
  await watcher.request("status", {});
  const proceeded = await waiting;
  assert.equal(proceeded.action, "grant");
  assert.equal(proceeded.lease.runId, "run-wait");
  assert.notEqual(proceeded.generation, grant.generation);
  const current = await watcher.request("status", {});
  assert.equal(current.reservations.some((reservation) => reservation.runId === "run-hold"), false);
  assert.equal(current.queue.some((entry) => entry.runId === "run-drop"), false);
});

test("unknown methods are rejected and the journal never stores prompts", async () => {
  const { broker, journal } = harness({ snapshot: { confidence: "measured" } });
  const client = pair(broker);
  await assert.rejects(client.request("exec", { command: "do-not-store" }), (error) => error.code === -32601);
  await client.request("admit", admitParams("run-journal"));
  assertJournal(journal);
  assert.equal(journal.some((entry) => entry.type === "grant" && entry.runId === "run-journal"), true);
});

test("ipc path is a named pipe on Windows and a socket path elsewhere, never a TCP port", async () => {
  const uid = (() => {
    if (typeof process.getuid === "function") return String(process.getuid());
    if (process.env.SUDO_UID) return process.env.SUDO_UID;
    try {
      const info = os.userInfo();
      if (info?.username) return info.username;
    } catch {
      // Fall through.
    }
    return process.env.USERNAME || process.env.USER || "unknown";
  })();
  const previous = process.env.XDG_RUNTIME_DIR;
  delete process.env.XDG_RUNTIME_DIR;
  try {
    assert.equal(ipcListenPath("win32", "shipmates-admission"), `\\\\.\\pipe\\shipmates-admission-${uid}`);
    assert.equal(ipcListenPath("darwin", "shipmates-admission", "/tmp"), `/tmp/shipmates-admission-${uid}.sock`);
    assert.equal(ipcListenPath("linux", "shipmates-admission", "/tmp"), `/tmp/shipmates-admission-${uid}.sock`);
  } finally {
    if (previous !== undefined) process.env.XDG_RUNTIME_DIR = previous;
  }
  await assert.rejects(() => startBroker({ path: "9", probe: async () => snapshot() }), /TCP/);
  await assert.rejects(() => startBroker({ path: "127.0.0.1:9", probe: async () => snapshot() }), /TCP/);

  const dir = mkdtempSync(path.join(os.tmpdir(), "sm-broker-"));
  const sock = path.join(dir, "admission.sock");
  const started = await startBroker({
    path: sock,
    probe: async () => snapshot({ confidence: "measured" }),
    journal: { append() {} },
  });
  let socket;
  try {
    const address = started.address();
    assert.equal(typeof address, "string");
    assert.equal(address, sock);
    socket = net.connect(sock);
    socket.on("error", () => {});
    const connection = createMessageConnection(socket, socket);
    const client = createRpcClient(connection);
    const current = await client.request("status", {});
    assert.deepEqual(current.reservations, []);
    assert.equal(current.queueLength, 0);
  } finally {
    if (socket) socket.end();
    await started.close();
    rmSync(dir, { recursive: true, force: true });
  }
});

test("a live broker path is not unlinked on EADDRINUSE from a second startBroker", async () => {
  const dir = mkdtempSync(path.join(os.tmpdir(), "sm-broker-live-"));
  const sock = path.join(dir, "admission.sock");
  const first = await startBroker({
    path: sock,
    probe: async () => snapshot({ confidence: "measured" }),
    journal: { append() {} },
  });
  try {
    assert.equal(existsSync(sock), true);
    await assert.rejects(
      () =>
        startBroker({
          path: sock,
          probe: async () => snapshot({ confidence: "measured" }),
          journal: { append() {} },
        }),
      /EADDRINUSE|listen/,
    );
    assert.equal(
      existsSync(sock),
      true,
      "a second startBroker must not unlink a live peer socket on EADDRINUSE",
    );
    // Peer can still dial the original broker.
    await new Promise((resolve, reject) => {
      const socket = net.connect(sock, resolve);
      socket.on("error", reject);
      socket.on("connect", () => {
        socket.end();
        resolve();
      });
    });
  } finally {
    await first.close();
    rmSync(dir, { recursive: true, force: true });
  }
});
