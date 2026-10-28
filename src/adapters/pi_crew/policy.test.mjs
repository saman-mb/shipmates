import assert from "node:assert/strict";
import test from "node:test";
import {
  ORDER_PREAMBLE,
  SHIPPED_TAG,
  brokerWanted,
  captainReserved,
  decideSpawn,
  liveCap,
  offState,
  parseState,
} from "./policy.mjs";

const LOCAL = "local-a";
const LOCAL_B = "local-b";
const LOCAL_C = "local-c";
const CLOUD = "cloud-a";

const ticks = [LOCAL, LOCAL_B, LOCAL_C];
const loaded = [LOCAL, LOCAL_B, LOCAL_C];

function state(over = {}) {
  return parseState({
    schemaVersion: 1,
    switch: "Split",
    roles: {},
    ticks,
    ceiling: 5,
    ...over,
  });
}

const order = "Outcome: x\nOwned paths: a\nDone-when: b\nOut of scope: c";

test("absent and bad files behave as Off and do not refuse", () => {
  for (const raw of ["", "not-json", { switch: "maybe" }, { schemaVersion: 2, switch: "Split", resources: "enforce" }]) {
    const parsed = parseState(raw);
    assert.equal(parsed.switch, "Off");
    assert.equal(parsed.resources, "off");
    assert.equal(brokerWanted(parsed), false);
    assert.equal(decideSpawn({ state: parsed, role: "senior-engineer" }).action, "allow");
  }
});

test("resources defaults off, preserves shadow or enforce, and a newer schema cannot enable it", () => {
  assert.equal(offState().resources, "off");
  assert.equal(parseState({ switch: "Split", ticks, ceiling: 5 }).resources, "off");
  assert.equal(parseState({ switch: "Local", resources: "nope" }).resources, "off");
  assert.equal(parseState({ switch: "Split", resources: "shadow", ticks, ceiling: 5 }).resources, "shadow");
  assert.equal(parseState({ switch: "Local", resources: "enforce", ticks, ceiling: 1 }).resources, "enforce");
  assert.equal(brokerWanted(parseState({ switch: "Off", resources: "enforce" })), false);
  assert.equal(brokerWanted(parseState({ switch: "Split", resources: "off" })), false);
  assert.equal(brokerWanted(parseState({ switch: "Local", resources: "shadow" })), true);
  assert.equal(captainReserved(parseState({ switch: "Local", resources: "enforce" })), true);
  assert.equal(captainReserved(parseState({ switch: "Local", resources: "shadow" })), true);
  assert.equal(captainReserved(parseState({ switch: "Split", resources: "enforce" })), false);
  assert.equal(captainReserved(parseState({ schemaVersion: 2, switch: "Local", resources: "enforce" })), false);
});

test("a null ceiling stays unset and does not force Off", () => {
  const parsed = parseState({ switch: "Split", ticks, ceiling: null });
  assert.equal(parsed.switch, "Split");
  assert.equal(parsed.ceiling, null);
});

test("Split honours a matching tag and refuses the other", () => {
  const ok = decideSpawn({
    state: state(),
    role: "senior-engineer",
    model: LOCAL,
    modelTag: "local",
    loaded,
    busy: [],
    cloudModels: [CLOUD],
    runningLocal: 0,
    runningAll: 0,
    task: order,
  });
  assert.equal(ok.action, "allow");
  assert.equal(ok.honoured, true);
  assert.equal(ok.prepend, ORDER_PREAMBLE);

  const bad = decideSpawn({
    state: state(),
    role: "senior-engineer",
    model: CLOUD,
    modelTag: "cloud",
    loaded,
    busy: [],
    cloudModels: [CLOUD],
    runningLocal: 0,
    runningAll: 0,
    task: order,
  });
  assert.equal(bad.action, "block");
  assert.match(bad.reason, /tagged local/);
  assert.match(bad.reason, /will not substitute/);
});

test("Local refuses a cloud name and does not read the tag table", () => {
  const flipped = state({ switch: "Local", roles: { architect: "cloud" } });
  const refused = decideSpawn({
    state: flipped,
    role: "architect",
    model: CLOUD,
    modelTag: "cloud",
    loaded,
    busy: [],
    cloudModels: [CLOUD],
    runningLocal: 0,
    runningAll: 0,
    task: "review",
  });
  assert.equal(refused.action, "block");
  assert.match(refused.reason, /ticked loaded local/);
  assert.equal(SHIPPED_TAG.architect, "cloud");
});

test("a missing model is refused and not selected", () => {
  const result = decideSpawn({
    state: state(),
    role: "architect",
    model: null,
    modelTag: null,
    loaded,
    busy: [],
    cloudModels: [CLOUD],
    runningLocal: 0,
    runningAll: 0,
  });
  assert.equal(result.action, "block");
  assert.match(result.reason, /will not substitute/);
  assert.equal(result.model, undefined);
});

test("an unknown role is refused", () => {
  const result = decideSpawn({
    state: state(),
    role: "intern",
    model: CLOUD,
    modelTag: "cloud",
    loaded,
    busy: [],
    cloudModels: [CLOUD],
    runningLocal: 0,
    runningAll: 0,
  });
  assert.equal(result.action, "block");
});

test("doer checklist fires for sdet even if flipped to cloud, not for architect in Local", () => {
  const thin = decideSpawn({
    state: state({ roles: { sdet: "cloud" } }),
    role: "sdet",
    model: CLOUD,
    modelTag: "cloud",
    loaded,
    busy: [],
    cloudModels: [CLOUD],
    runningLocal: 0,
    runningAll: 0,
    task: "run the tests",
  });
  assert.equal(thin.action, "block");
  assert.match(thin.reason, /Outcome:/);

  const judge = decideSpawn({
    state: state({ switch: "Local" }),
    role: "architect",
    model: LOCAL,
    modelTag: "local",
    loaded,
    busy: [],
    cloudModels: [CLOUD],
    runningLocal: 0,
    runningAll: 0,
    task: "think",
  });
  assert.equal(judge.action, "allow");
  assert.equal(judge.prepend, undefined);
});

test("cap stays at a captain ceiling above the loaded count, then drops", () => {
  assert.equal(liveCap(5, ticks, loaded), 5);
  assert.equal(liveCap(5, ticks, [LOCAL, LOCAL_B]), 4);
  assert.equal(liveCap(5, ticks, []), 0);
});

test("Split ignores cloud seats and Local counts every crew seat", () => {
  const split = decideSpawn({
    state: state(),
    role: "architect",
    model: CLOUD,
    modelTag: "cloud",
    loaded,
    busy: [],
    cloudModels: [CLOUD],
    runningLocal: 5,
    runningAll: 9,
  });
  assert.equal(split.action, "allow");

  const local = decideSpawn({
    state: state({ switch: "Local" }),
    role: "architect",
    model: LOCAL,
    modelTag: "local",
    loaded,
    busy: [],
    cloudModels: [CLOUD],
    runningLocal: 0,
    runningAll: 5,
  });
  assert.equal(local.action, "block");
  assert.match(local.reason, /live cap is 5, 5 already running/);
});

test("an absent ceiling on Split or Local refuses", () => {
  const result = decideSpawn({
    state: state({ ceiling: null }),
    role: "architect",
    model: CLOUD,
    modelTag: "cloud",
    loaded,
    busy: [],
    cloudModels: [CLOUD],
    runningLocal: 0,
    runningAll: 0,
  });
  assert.equal(result.action, "block");
  assert.match(result.reason, /no ceiling/);
});

test("user-facing reasons do not say login, logout, or official, and the module has no ceiling literal", () => {
  const reasons = [
    decideSpawn({
      state: state(),
      role: "senior-engineer",
      model: CLOUD,
      modelTag: "cloud",
      loaded,
      busy: [],
      cloudModels: [],
      runningLocal: 0,
      runningAll: 0,
      task: order,
    }).reason,
  ];
  for (const reason of reasons) {
    assert.doesNotMatch(reason, /login|logout|official/i);
  }
  assert.equal(JSON.stringify(SHIPPED_TAG).includes("gpt"), false);
});

test("/shipmates-crew menu places the saved switch first so re-running does not highlight Off", async () => {
  const { mkdtempSync, readFileSync, writeFileSync } = await import("node:fs");
  const { tmpdir } = await import("node:os");
  const { join } = await import("node:path");
  const extension = (await import("./index.ts")).default;

  const dir = mkdtempSync(join(tmpdir(), "shipmates-crew-menu-"));
  const prevDir = process.env.PI_CODING_AGENT_DIR;
  process.env.PI_CODING_AGENT_DIR = dir;
  try {
    writeFileSync(
      join(dir, "shipmates-crew.json"),
      JSON.stringify({ schemaVersion: 1, switch: "Off", roles: {}, ticks: [LOCAL], ceiling: 1 }),
    );
    let commandHandler;
    extension({
      registerCommand(name, spec) {
        if (name === "shipmates-crew") commandHandler = spec.handler;
      },
      on() {},
    });
    assert.equal(typeof commandHandler, "function");

    const selects = [];
    const makeCtx = (pickIndex) => ({
      ui: {
        notify() {},
        async select(title, options) {
          selects.push({ title, options });
          return options[pickIndex];
        },
      },
    });

    // First run: state is Off, captain changes switch to Split (index 1).
    await commandHandler("", makeCtx(1));
    assert.equal(parseState(readFileSync(join(dir, "shipmates-crew.json"), "utf8")).switch, "Split");

    // Second run: menu opens with Split already saved. The first option (index 0,
    // pre-highlighted by pi's ExtensionSelectorComponent) must be the saved switch
    // ("Split"), not "Off", the selector title names the current switch, and
    // pressing Enter on index 0 must keep "Split".
    await commandHandler("", makeCtx(0));
    assert.match(selects[1].title, /Split/);
    assert.equal(
      selects[1].options[0],
      "Split",
      `expected saved switch "Split" to be pre-selected first, got options: ${JSON.stringify(selects[1].options)}`,
    );
    assert.deepEqual(selects[1].options, ["Split", "Off", "Local"]);
    assert.equal(parseState(readFileSync(join(dir, "shipmates-crew.json"), "utf8")).switch, "Split");
  } finally {
    if (prevDir === undefined) delete process.env.PI_CODING_AGENT_DIR;
    else process.env.PI_CODING_AGENT_DIR = prevDir;
  }
});
