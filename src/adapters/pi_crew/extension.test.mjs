import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { parseState } from "./policy.mjs";
import { install, readState, RUNTIME_PINS, runIdsIn, writeState } from "./index.ts";

const LOCAL = "local-a";
const CLOUD = "cloud-a";
const order = "Outcome: x\nOwned paths: a\nDone-when: b\nOut of scope: c";

function parsed(over = {}) {
  return parseState({
    schemaVersion: 1,
    switch: "Split",
    roles: {},
    ticks: [LOCAL],
    ceiling: 5,
    resources: "off",
    ...over,
  });
}

function harness(state, connect) {
  const handlers = {};
  const notes = [];
  const leases = new Map();
  const calls = [];
  const pi = {
    registerCommand() {},
    on(name, fn) {
      handlers[name] = fn;
    },
  };
  const client = {
    async request(method, params) {
      calls.push({ method, params });
      if (connect) return connect(method, params);
      return { action: "grant", generation: 3, lease: { runId: params.runId, generation: 3, modelId: params.model } };
    },
  };
  install(pi, {
    readState: () => state,
    connect: async () => client,
    notes,
    leases,
    heartbeat: false,
    sessionId: "session-1",
    statePath: join(tmpdir(), "shipmates-crew-unused.json"),
  });
  return { handlers, notes, leases, calls };
}

function seatEvent(over = {}) {
  return {
    toolName: "subagent",
    toolCallId: "call-1",
    input: {
      agent: "senior-engineer",
      model: LOCAL,
      task: order,
      ...over,
    },
  };
}

test("Off allows a spawn and does not ask the broker", async () => {
  const { handlers, calls } = harness(parsed({ switch: "Off", resources: "enforce" }));
  const event = seatEvent();
  const result = await handlers.tool_call(event);
  assert.equal(result, undefined);
  assert.equal(calls.length, 0);
  assert.equal(event.input.model, LOCAL);
});

test("resources off does not start or ask a broker", async () => {
  const { handlers, calls } = harness(parsed({ switch: "Local", resources: "off" }));
  const event = seatEvent();
  const result = await handlers.tool_call(event);
  assert.equal(result, undefined);
  assert.equal(calls.length, 0);
  assert.equal(event.input.model, LOCAL);
});

test("shadow asks, records a queue, allows the spawn, and does not substitute", async () => {
  const { handlers, notes, calls } = harness(parsed({ switch: "Split", resources: "shadow" }), async () => ({
    action: "queue",
    queuePosition: 2,
    generation: null,
    reason: "Waiting: pinned.",
  }));
  const event = seatEvent();
  const result = await handlers.tool_call(event);
  assert.equal(result, undefined);
  assert.equal(event.input.model, LOCAL);
  assert.equal(calls[0].method, "admit");
  assert.equal(calls[0].params.wait, false);
  assert.equal(calls[0].params.pinned, true);
  assert.equal(calls[0].params.model, LOCAL);
  assert.equal(notes[0].type, "shadow");
  assert.equal(notes[0].action, "queue");
  assert.equal(notes[0].queuePosition, 2);
});

test("shadow still allows the spawn when the broker cannot be reached", async () => {
  const handlers = {};
  const notes = [];
  const pi = {
    registerCommand() {},
    on(name, fn) {
      handlers[name] = fn;
    },
  };
  install(pi, {
    readState: () => parsed({ switch: "Local", resources: "shadow" }),
    connect: async () => {
      throw new Error("down");
    },
    notes,
    heartbeat: false,
  });
  const event = seatEvent();
  const result = await handlers.tool_call(event);
  assert.equal(result, undefined);
  assert.equal(event.input.model, LOCAL);
  assert.equal(notes[0].action, "unreachable");
});

test("enforce awaits a grant and blocks when the broker cannot be reached", async () => {
  const handlers = {};
  const pi = {
    registerCommand() {},
    on(name, fn) {
      handlers[name] = fn;
    },
  };
  install(pi, {
    readState: () => parsed({ switch: "Local", resources: "enforce" }),
    connect: async () => {
      throw new Error("down");
    },
    heartbeat: false,
  });
  const event = seatEvent();
  const result = await handlers.tool_call(event);
  assert.equal(result.block, true);
  assert.match(result.reason, /cannot be reached/);
  assert.match(result.reason, /will not substitute/);
  assert.equal(event.input.model, LOCAL);
});

test("enforce keeps the named model when a grant arrives", async () => {
  const { handlers, calls, leases } = harness(parsed({ switch: "Split", resources: "enforce" }));
  const event = seatEvent();
  const result = await handlers.tool_call(event);
  assert.equal(result, undefined);
  assert.equal(event.input.model, LOCAL);
  assert.equal(calls[0].params.wait, true);
  assert.equal(leases.has(LOCAL), false);
  assert.equal(leases.has("call-1#0"), true);
  assert.equal(leases.get("call-1#0").brokerRunId, "call-1#0");
});

test("a cloud seat under enforce does not ask the broker", async () => {
  const { handlers, calls } = harness(parsed({ switch: "Split", resources: "enforce" }));
  const event = seatEvent({ agent: "architect", model: CLOUD, task: "review" });
  const result = await handlers.tool_call(event);
  assert.equal(result, undefined);
  assert.equal(calls.length, 0);
  assert.equal(event.input.model, CLOUD);
});

test("leases release on tool_execution_end and rekey when the result has a run id", async () => {
  const { handlers, calls, leases, notes } = harness(parsed({ switch: "Local", resources: "enforce" }));
  await handlers.tool_call(seatEvent());
  assert.equal(leases.has("call-1#0"), true);
  await handlers.tool_execution_end({
    toolName: "subagent",
    toolCallId: "call-1",
    result: { details: { runId: "run-9" } },
  });
  assert.equal(notes.some((note) => note.type === "rekey" && note.to === "run-9"), true);
  assert.equal(leases.has("call-1#0"), false);
  assert.equal(leases.has("run-9"), false);
  assert.equal(leases.has(LOCAL), false);
  const release = calls.find((call) => call.method === "release");
  assert.equal(release.params.runId, "call-1#0");
  assert.equal(release.params.generation, 3);
  assert.notEqual(release.params.runId, LOCAL);
});

test("run id extraction does not invent a hold hook", () => {
  assert.deepEqual(runIdsIn({ details: { runId: "run-9" } }), ["run-9"]);
  assert.deepEqual(runIdsIn({ content: [{ type: "text", text: "no id here" }] }), []);
});

test("an aborted enforce wait cancels without throwing", async () => {
  const abort = new AbortController();
  const { handlers } = harness(parsed({ switch: "Local", resources: "enforce" }), async (method) => {
    if (method === "admit") {
      abort.abort();
      return new Promise(() => {});
    }
    if (method === "cancel") throw new Error("cancel failed");
    return {};
  });
  const event = seatEvent();
  const started = { toolName: "subagent", toolCallId: "call-1", args: event.input };
  await handlers.tool_execution_start(started, { signal: abort.signal });
  const result = await handlers.tool_call(event);
  assert.equal(result.block, true);
  assert.match(result.reason, /Aborted/);
});

test("the captain chat is reserved only for Local enforce and released when the turn ends", async () => {
  const previous = process.env.PI_SUBAGENT_CHILD;
  delete process.env.PI_SUBAGENT_CHILD;
  try {
    const local = harness(parsed({ switch: "Local", resources: "enforce" }));
    const ctx = {
      model: { provider: "local", id: "a" },
      aborted: false,
      abort() {
        this.aborted = true;
      },
      ui: { notify() {} },
    };
    await local.handlers.turn_start({ turnIndex: 1 }, ctx);
    assert.equal(local.calls[0].method, "admit");
    assert.equal(local.calls[0].params.role, "captain");
    assert.equal(local.calls[0].params.model, "local/a");
    assert.equal(local.calls[0].params.pinned, true);
    assert.equal(ctx.model.id, "a");
    assert.equal(local.leases.has("local/a"), false);
    await local.handlers.turn_end({ turnIndex: 1 });
    assert.equal(local.calls.some((call) => call.method === "release"), true);
    assert.equal(local.leases.size, 0);

    const shadow = harness(parsed({ switch: "Local", resources: "shadow", contextTokens: 128 }));
    await shadow.handlers.turn_start({ turnIndex: 0 }, ctx);
    assert.equal(shadow.calls[0].method, "admit");
    assert.equal(shadow.calls[0].params.contextTokens, 128);
    assert.equal(shadow.calls[0].params.protocolVersion, 1);

    for (const state of [
      parsed({ switch: "Split", resources: "enforce" }),
      parsed({ switch: "Off", resources: "enforce" }),
      parsed({ switch: "Local", resources: "off" }),
    ]) {
      const other = harness(state);
      await other.handlers.turn_start({ turnIndex: 0 }, ctx);
      assert.equal(other.calls.length, 0);
    }

    process.env.PI_SUBAGENT_CHILD = "1";
    const child = harness(parsed({ switch: "Local", resources: "enforce" }));
    await child.handlers.turn_start({ turnIndex: 1 }, ctx);
    assert.equal(child.calls.length, 0);
  } finally {
    if (previous === undefined) delete process.env.PI_SUBAGENT_CHILD;
    else process.env.PI_SUBAGENT_CHILD = previous;
  }
});

test("a cloud chat is never reserved by the local broker", async () => {
  const previous = process.env.PI_SUBAGENT_CHILD;
  delete process.env.PI_SUBAGENT_CHILD;
  try {
    const ctx = {
      model: { provider: "cloud", id: "big" },
      modelRegistry: {
        getAvailable: () => [
          { provider: "cloud", id: "big", cost: { input: 3, output: 9, cacheRead: 0, cacheWrite: 0 } },
        ],
      },
      aborted: false,
      abort() {
        this.aborted = true;
      },
      ui: { notify() {} },
    };
    for (const resources of ["shadow", "enforce"]) {
      const run = harness(parsed({ switch: "Local", resources }));
      await run.handlers.turn_start({ turnIndex: 0 }, ctx);
      assert.equal(run.calls.length, 0, `a cloud chat must not be reserved under ${resources}`);
      assert.equal(ctx.aborted, false);
    }
  } finally {
    if (previous === undefined) delete process.env.PI_SUBAGENT_CHILD;
    else process.env.PI_SUBAGENT_CHILD = previous;
  }
});

test("shadow records a decision but never holds a lease", async () => {
  const { handlers, leases, calls } = harness(parsed({ switch: "Split", resources: "shadow" }), async () => ({
    action: "grant",
    generation: 4,
    lease: { runId: "call-1#0", generation: 4, modelId: LOCAL },
  }));
  const event = seatEvent();
  await handlers.tool_call(event);
  assert.equal(calls[0].params.dryRun, true);
  assert.equal(leases.size, 0, "shadow must not keep a reservation");
});

test("writeState preserves resources and does not treat a newer schema as enablement", () => {
  const dir = mkdtempSync(join(tmpdir(), "shipmates-crew-"));
  try {
    const target = join(dir, "shipmates-crew.json");
    const written = writeState({ switch: "Local", resources: "shadow", ticks: [LOCAL], ceiling: 2 }, target);
    assert.equal(written.schemaVersion, 1);
    assert.equal(written.resources, "shadow");
    const round = readState(target);
    assert.equal(round.resources, "shadow");
    assert.equal(round.switch, "Local");
    writeState({ ...round, schemaVersion: 4, resources: "enforce" }, target);
    const forced = JSON.parse(readFileSync(target, "utf8"));
    assert.equal(forced.schemaVersion, 1);
    assert.equal(forced.resources, "off");
    assert.equal(forced.switch, "Off");
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("runtime pins match package.json and tests do not install them", () => {
  const pkg = JSON.parse(readFileSync(new URL("./package.json", import.meta.url), "utf8"));
  assert.deepEqual(pkg.dependencies, RUNTIME_PINS);
  assert.equal(process.env.NODE_TEST_CONTEXT !== undefined, true);
});

const LOCAL_MODEL = "lmstudio/model-a";
const CLOUD_MODEL = "anthropic/model-b";

function makeRegistry() {
  return {
    getAvailable: () => [
      { provider: "lmstudio", id: "model-a", cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 } },
      { provider: "anthropic", id: "model-b", cost: { input: 3, output: 9, cacheRead: 0, cacheWrite: 0 } },
    ],
  };
}

async function withState(switchState, ceiling, ticks, fn) {
  const dir = mkdtempSync(join(tmpdir(), "shipmates-crew-ext-"));
  const prevDir = process.env.PI_CODING_AGENT_DIR;
  process.env.PI_CODING_AGENT_DIR = dir;
  try {
    writeFileSync(
      join(dir, "shipmates-crew.json"),
      JSON.stringify({ schemaVersion: 1, switch: switchState, roles: {}, ticks, ceiling }),
    );
    return await fn(dir);
  } finally {
    if (prevDir === undefined) delete process.env.PI_CODING_AGENT_DIR;
    else process.env.PI_CODING_AGENT_DIR = prevDir;
  }
}

async function loadHandler() {
  const extension = (await import("./index.ts")).default;
  let handler;
  extension({
    registerCommand() {},
    on(name, fn) {
      if (name === "tool_call") handler = fn;
    },
  });
  return handler;
}

test("subagent({ action: \"status\" }) is not blocked", async () => {
  await withState("Split", 2, [], async () => {
    const handler = await loadHandler();
    const result = await handler({ toolName: "subagent", input: { action: "status", id: "7e0ccd59" } }, {});
    assert.equal(result, undefined);
  });
});

test("subagent management actions pass through", async () => {
  await withState("Split", 2, [], async () => {
    const handler = await loadHandler();
    for (const action of ["list", "guide", "cancel", "message"]) {
      const result = await handler({ toolName: "subagent", input: { action, id: "abc" } }, {});
      assert.equal(result, undefined, `action ${action} should pass through`);
    }
  });
});

test("subagent({ workflow: true }) is not rejected as unknown role", async () => {
  await withState("Split", 2, [], async () => {
    const handler = await loadHandler();
    const result = await handler(
      { toolName: "subagent", input: { workflow: true, async: true, cwd: "/tmp", model: CLOUD_MODEL } },
      {},
    );
    assert.equal(result, undefined);
  });
});

test("non-management subagent spawns are still enforced", async () => {
  await withState("Split", 2, [], async () => {
    const handler = await loadHandler();
    const result = await handler(
      { toolName: "subagent", input: { agent: "intern", model: LOCAL_MODEL, task: "do work" } },
      { modelRegistry: makeRegistry() },
    );
    assert.equal(result.block, true);
    assert.match(result.reason, /not a crew role/);
  });
});

const DOER_ORDER =
  "Outcome: build\nOwned paths: src/x.ts\nDone-when: tests green\nOut of scope: docs\n";

test('model: "inherit" resolves to the session model before tagging', async () => {
  await withState("Local", 2, [LOCAL_MODEL], async () => {
    const handler = await loadHandler();
    const result = await handler(
      {
        toolName: "subagent",
        input: { agent: "senior-engineer", model: "inherit", task: DOER_ORDER },
      },
      {
        modelRegistry: makeRegistry(),
        model: { provider: "lmstudio", id: "model-a" },
      },
    );
    assert.equal(result, undefined, "inherit on a local session model must not refuse a doer");
  });
});

test('model: "inherit" without a session model is refused clearly', async () => {
  await withState("Local", 2, [LOCAL_MODEL], async () => {
    const handler = await loadHandler();
    const result = await handler(
      {
        toolName: "subagent",
        input: { agent: "senior-engineer", model: "inherit", task: DOER_ORDER },
      },
      { modelRegistry: makeRegistry() },
    );
    assert.equal(result.block, true);
    assert.match(result.reason, /inherit needs a session model/);
  });
});

test('model: "inherit" on a cloud session still refuses a local-tagged doer under Local', async () => {
  await withState("Local", 2, [LOCAL_MODEL], async () => {
    const handler = await loadHandler();
    const result = await handler(
      {
        toolName: "subagent",
        input: { agent: "senior-engineer", model: "inherit", task: DOER_ORDER },
      },
      {
        modelRegistry: makeRegistry(),
        model: { provider: "anthropic", id: "model-b" },
      },
    );
    assert.equal(result.block, true);
    assert.match(result.reason, /must be a local model/);
  });
});
