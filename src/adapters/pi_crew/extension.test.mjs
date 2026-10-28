import assert from "node:assert/strict";
import test from "node:test";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const LOCAL_MODEL = "lmstudio/model-a";
const CLOUD_MODEL = "anthropic/model-b";
const STATE_NAME = "shipmates-crew.json";

function makeRegistry() {
  return {
    getAvailable: () => [
      {
        provider: "lmstudio",
        id: "model-a",
        cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
      },
      {
        provider: "anthropic",
        id: "model-b",
        cost: { input: 3, output: 9, cacheRead: 0, cacheWrite: 0 },
      },
    ],
  };
}

async function withState(switchState, ceiling, ticks, fn) {
  const dir = mkdtempSync(join(tmpdir(), "shipmates-crew-ext-"));
  const prevDir = process.env.PI_CODING_AGENT_DIR;
  process.env.PI_CODING_AGENT_DIR = dir;
  try {
    writeFileSync(
      join(dir, STATE_NAME),
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
    const result = handler({ toolName: "subagent", input: { action: "status", id: "7e0ccd59" } }, {});
    assert.equal(result, undefined);
  });
});

test("subagent management actions pass through", async () => {
  await withState("Split", 2, [], async () => {
    const handler = await loadHandler();
    for (const action of ["list", "guide", "cancel", "message"]) {
      const result = handler({ toolName: "subagent", input: { action, id: "abc" } }, {});
      assert.equal(result, undefined, `action ${action} should pass through`);
    }
  });
});

test("subagent({ workflow: true }) is not rejected as unknown role", async () => {
  await withState("Split", 2, [], async () => {
    const handler = await loadHandler();
    const result = handler(
      { toolName: "subagent", input: { workflow: true, async: true, cwd: "/tmp", model: CLOUD_MODEL } },
      {},
    );
    assert.equal(result, undefined);
  });
});

test("non-management subagent spawns are still enforced", async () => {
  await withState("Split", 2, [], async () => {
    const handler = await loadHandler();
    const result = handler(
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
    const result = handler(
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
    const result = handler(
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
    const result = handler(
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
