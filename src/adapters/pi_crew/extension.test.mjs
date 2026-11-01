import assert from "node:assert/strict";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { parseState } from "./policy.mjs";
import { install, readState, RUNTIME_PINS, runIdsIn, writeState, ensureRuntimePackages } from "./index.ts";

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

test("a seat on the captain's model waits until the turn ends", async () => {
  const previous = process.env.PI_SUBAGENT_CHILD;
  delete process.env.PI_SUBAGENT_CHILD;
  try {
    const run = harness(parsed({ switch: "Local", resources: "enforce", ticks: ["local/a"] }));
    const ctx = {
      model: { provider: "local", id: "a" },
      aborted: false,
      abort() {
        this.aborted = true;
      },
      ui: { notify() {} },
    };
    await run.handlers.turn_start({ turnIndex: 1 }, ctx);
    assert.equal(ctx.aborted, false);
    const blocked = await run.handlers.tool_call(seatEvent({ model: "local/a" }));
    assert.equal(blocked.block, true);
    assert.match(blocked.reason, /Waiting: local\/a is busy/);
    assert.match(blocked.reason, /This seat stays on local\/a\. Not moved\./);
    await run.handlers.turn_end({ turnIndex: 1 });
    assert.equal(run.leases.size, 0);
    const allowed = await run.handlers.tool_call(seatEvent({ model: "local/a" }));
    assert.equal(allowed, undefined);
  } finally {
    if (previous === undefined) delete process.env.PI_SUBAGENT_CHILD;
    else process.env.PI_SUBAGENT_CHILD = previous;
  }
});

test("a captain admission failure leaves no phantom busy model", async () => {
  const previous = process.env.PI_SUBAGENT_CHILD;
  delete process.env.PI_SUBAGENT_CHILD;
  try {
    let denyCaptain = false;
    const run = harness(parsed({ switch: "Local", resources: "enforce", ticks: ["local/a"] }), async (_method, params) => {
      if (params.role === "captain" && denyCaptain) return { action: "deny", reason: "Denied: full." };
      return { action: "grant", generation: 3, lease: { runId: params.runId, generation: 3, modelId: params.model } };
    });
    const ctx = {
      model: { provider: "local", id: "a" },
      aborted: false,
      abort() {
        this.aborted = true;
      },
      ui: { notify() {} },
    };
    // A live captain turn holds the model.
    await run.handlers.turn_start({ turnIndex: 0 }, ctx);
    assert.equal((await run.handlers.tool_call(seatEvent({ model: "local/a" }))).block, true);
    assert.equal(run.leases.size, 1);
    // The turn is aborted before turn_end and the next admission fails: the
    // previous lease is released and the model must not stay busy.
    denyCaptain = true;
    await run.handlers.turn_start({ turnIndex: 1 }, ctx);
    assert.equal(ctx.aborted, true);
    assert.equal(run.leases.size, 0);
    assert.equal(await run.handlers.tool_call(seatEvent({ model: "local/a" })), undefined);
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

function fakeDep(root, name, version) {
  const depDir = join(root, "node_modules", name);
  mkdirSync(depDir, { recursive: true });
  writeFileSync(join(depDir, "package.json"), JSON.stringify({ name, version }));
}

function scratchDir() {
  return mkdtempSync(join(realpathSync(tmpdir()), "shipmates-crew-"));
}

function fakeInstalledRuntimeRoot(base) {
  const root = join(base, ".shipmates-runtime");
  for (const name of Object.keys(RUNTIME_PINS)) fakeDep(root, name, RUNTIME_PINS[name]);
  return root;
}

test("an exact pin resolvable from a package node_modules resolves without provisioning", async () => {
  const scratch = scratchDir();
  const pkgRoot = join(scratch, "pkg");
  fakeDep(pkgRoot, "systeminformation", RUNTIME_PINS["systeminformation"]);
  fakeDep(pkgRoot, "vscode-jsonrpc", RUNTIME_PINS["vscode-jsonrpc"]);
  try {
    const resolved = await ensureRuntimePackages(pkgRoot, true);
    assert.equal(resolved, pkgRoot);
    assert.equal(existsSync(join(pkgRoot, ".shipmates-runtime")), false);
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
});

test("a present-but-wrong-version copy is rejected and provisioning still happens", async () => {
  const scratch = scratchDir();
  const badRoot = join(scratch, "bad");
  fakeDep(badRoot, "systeminformation", "0.0.1");
  const runtimeRootDir = fakeInstalledRuntimeRoot(badRoot);
  try {
    const resolved = await ensureRuntimePackages(badRoot, true);
    assert.equal(resolved, runtimeRootDir);
    assert.equal(existsSync(join(badRoot, ".shipmates-runtime", "package.json")), true);
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
});

test("nothing resolvable falls back to the existing self-provisioning behaviour", async () => {
  const scratch = scratchDir();
  const emptyRoot = join(scratch, "empty");
  const runtimeRootDir = fakeInstalledRuntimeRoot(emptyRoot);
  try {
    const resolved = await ensureRuntimePackages(emptyRoot, true);
    assert.equal(resolved, runtimeRootDir);
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
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
  const actions = [
    "status",
    "list",
    "guide",
    "cancel",
    "message",
    "stop",
    "steer",
    "resume",
    "validate",
    "pending",
    "reply",
  ];
  for (const switchState of ["Split", "Local"]) {
    await withState(switchState, 2, [], async () => {
      const handler = await loadHandler();
      for (const action of actions) {
        const result = await handler(
          { toolName: "subagent", input: { action, id: "abc" } },
          { modelRegistry: makeRegistry() },
        );
        assert.equal(result, undefined, `${switchState} action ${action} should pass through`);
      }
    });
  }
});

test("an unknown subagent action with no spawn intent passes the gate", async () => {
  await withState("Split", 2, [], async () => {
    const handler = await loadHandler();
    const result = await handler(
      { toolName: "subagent", input: { action: "an-action-this-file-has-never-heard-of", id: "abc" } },
      { modelRegistry: makeRegistry() },
    );
    assert.equal(result, undefined);
  });
});

test("spawn intent with no model is still refused", async () => {
  for (const switchState of ["Split", "Local"]) {
    await withState(switchState, 2, [], async () => {
      const handler = await loadHandler();
      const inputs = [
        { action: "spawn", agent: "senior-engineer", task: "do work" },
        { tasks: [{ agent: "senior-engineer", task: "do work" }] },
        { args: { runs: [{ agent: "senior-engineer", task: "do work" }] } },
      ];
      for (const input of inputs) {
        const result = await handler({ toolName: "subagent", input }, { modelRegistry: makeRegistry() });
        assert.equal(result?.block, true, `${switchState} spawn intent without a model should block: ${JSON.stringify(input)}`);
        assert.match(result.reason, /model/i);
      }
    });
  }
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

test("session_start is a no-op when PI_SUBAGENT_CHILD is set and does not throw when pi-subagents is absent", async () => {
  const prev = process.env.PI_SUBAGENT_CHILD;
  process.env.PI_SUBAGENT_CHILD = "1";
  try {
    const { handlers } = harness(parsed({ switch: "Off" }));
    assert.equal(typeof handlers.session_start, "function");
    await assert.doesNotReject(() => handlers.session_start());
  } finally {
    if (prev === undefined) delete process.env.PI_SUBAGENT_CHILD;
    else process.env.PI_SUBAGENT_CHILD = prev;
  }
  // Absent package path: clear the env and still must not throw.
  const { handlers } = harness(parsed({ switch: "Off" }));
  await assert.doesNotReject(() => handlers.session_start());
});

test("session_shutdown clears seats and nulls the broker handle without throwing", async () => {
  const { handlers, leases } = harness(parsed({ switch: "Split", resources: "enforce" }));
  leases.set("seat-1", {
    seatKey: "seat-1",
    toolCallId: "call-1",
    brokerRunId: "run-1",
    generation: 1,
    model: LOCAL,
    role: "senior-engineer",
  });
  assert.equal(typeof handlers.session_shutdown, "function");
  await assert.doesNotReject(() => handlers.session_shutdown());
  assert.equal(leases.size, 0);
});

test("shipmates-crew menu: Off to Split saves and notifies; cancelled Local picker leaves state unchanged", async () => {
  const dir = mkdtempSync(join(tmpdir(), "shipmates-menu-"));
  const stateFile = join(dir, "shipmates-crew.json");
  writeFileSync(stateFile, JSON.stringify(parsed({ switch: "Off" }), null, 2));

  const notes = [];
  let command = null;
  const pi = {
    registerCommand(name, def) {
      command = def;
    },
    on() {},
  };
  install(pi, {
    readState: () => readState(stateFile),
    connect: async () => null,
    notes,
    leases: new Map(),
    heartbeat: false,
    sessionId: "s-menu",
    statePath: stateFile,
  });
  assert.ok(command, "shipmates-crew command must register");

  // Off → Split.
  await command.handler([], {
    ui: {
      notify(msg) {
        notes.push({ type: "notify", msg });
      },
      async select() {
        return "Split";
      },
    },
    model: null,
    modelRegistry: null,
  });
  assert.equal(readState(stateFile).switch, "Split");
  assert.ok(notes.some((n) => /Saved|Split/i.test(n.msg || "")));

  // Selecting Off again is a no-op write of Off (still Saved).
  await command.handler([], {
    ui: {
      notify(msg) {
        notes.push({ type: "notify", msg });
      },
      async select() {
        return "Off";
      },
    },
    model: null,
    modelRegistry: null,
  });
  assert.equal(readState(stateFile).switch, "Off");

  // Local without a tick matching chat model → cancel picker → unchanged.
  writeState({ ...parsed({ switch: "Off", ticks: [LOCAL] }) }, stateFile);
  const before = readState(stateFile).switch;
  await command.handler([], {
    ui: {
      notify(msg) {
        notes.push({ type: "notify", msg });
      },
      async select(label) {
        if (/Switch/i.test(label)) return "Local";
        return null; // cancel model picker
      },
    },
    model: { provider: "cloud", id: "x" },
    modelRegistry: {
      getAvailable() {
        return [{ provider: "local", id: "a", cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 } }];
      },
      find() {
        return null;
      },
    },
    setModel: async () => {},
  });
  assert.equal(readState(stateFile).switch, before);
  assert.ok(notes.some((n) => /unchanged|No local model/i.test(n.msg || "")));
  rmSync(dir, { recursive: true, force: true });
});
