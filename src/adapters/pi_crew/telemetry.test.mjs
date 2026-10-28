import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { probe } from "./telemetry.mjs";

const KEYS = ["observedAtMs", "confidence", "pressure", "pools", "models", "headroomBytes"];
const MODEL_KEYS = ["id", "residentBytes", "configuredContextTokens", "slots", "slotsInUse", "loaded"];

function host(over = {}) {
  return {
    confidence: "measured",
    pressure: "ok",
    pools: [{ id: "host", availableBytes: 4096 }],
    ...over,
  };
}

function engines(over = {}) {
  return {
    confidence: "measured",
    models: [{
      id: "engine/model-a",
      residentBytes: 128,
      configuredContextTokens: 1024,
      slots: 2,
      slotsInUse: 1,
      loaded: true,
    }],
    ...over,
  };
}

function injected(over = {}) {
  return {
    nowMs: 1_700_000_000_000,
    headroomBytes: 64,
    probeHost: async () => host(),
    probeEngines: async () => engines(),
    ...over,
  };
}

test("injected host and engine probes return the capacity snapshot", async () => {
  const snapshot = await probe(injected());
  assert.deepEqual(Object.keys(snapshot), KEYS);
  assert.deepEqual(snapshot, {
    observedAtMs: 1_700_000_000_000,
    confidence: "measured",
    pressure: "ok",
    pools: [{ id: "host", availableBytes: 4096 }],
    models: [{
      id: "engine/model-a",
      residentBytes: 128,
      configuredContextTokens: 1024,
      slots: 2,
      slotsInUse: 1,
      loaded: true,
    }],
    headroomBytes: 64,
  });
  assert.deepEqual(Object.keys(snapshot.models[0]), MODEL_KEYS);
  assert.equal(snapshot.pools[0].availableBytes, 4096);
});

test("the weaker probe confidence wins and duplicate ids are charged once", async () => {
  const snapshot = await probe(injected({
    probeHost: async () => host({
      pools: [{ id: "host", availableBytes: 500 }, { id: "host", availableBytes: 400 }, { id: "gpu", availableBytes: 50 }],
    }),
    probeEngines: async () => engines({
      confidence: "reported",
      pools: [{ id: "gpu", availableBytes: 40 }],
      models: [
        { id: "engine/model-a", residentBytes: 20, configuredContextTokens: 10, slots: 1, slotsInUse: 1, loaded: true },
        { id: "engine/model-a", residentBytes: 30, configuredContextTokens: 12, slots: 2, slotsInUse: 1, loaded: true },
        { id: " ", loaded: true },
      ],
    }),
  }));
  assert.equal(snapshot.confidence, "reported");
  assert.deepEqual(snapshot.pools, [
    { id: "host", availableBytes: 400 },
    { id: "gpu", availableBytes: 40 },
  ]);
  assert.deepEqual(snapshot.models, [{
    id: "engine/model-a",
    residentBytes: 50,
    configuredContextTokens: 12,
    slots: 2,
    slotsInUse: 2,
    loaded: true,
  }]);
});

test("a failed or missing probe returns unknown and does not throw", async () => {
  const cases = [
    { probeHost: async () => { throw new Error("host down"); } },
    { probeEngines: async () => Promise.reject(new Error("engine down")) },
    { probeHost: async () => null },
    { probeEngines: () => undefined },
    { probeHost: "missing" },
    { probeEngines: async () => ["not-an-observation"] },
    { probeHost: async () => host({ pools: [{ id: "host", availableBytes: -1 }, { id: "", availableBytes: 1 }] }) },
  ];
  for (const failure of cases) {
    const snapshot = await probe(injected({ headroomBytes: 32, ...failure }));
    assert.equal(snapshot.confidence, "unknown");
    assert.equal(snapshot.pressure, "ok");
    assert.deepEqual(snapshot.pools, []);
    assert.deepEqual(snapshot.models, []);
    assert.equal(snapshot.headroomBytes, 32);
    assert.equal(snapshot.observedAtMs, 1_700_000_000_000);
  }
});

test("high pressure is preserved and an explicit ok is not rewritten", async () => {
  const high = await probe(injected({ probeHost: async () => host({ pressure: "high" }) }));
  assert.equal(high.pressure, "high");
  assert.equal(high.confidence, "measured");
  const ok = await probe(injected({
    probeHost: async () => host({ pressure: "ok", pools: [{ id: "host", availableBytes: 0 }] }),
  }));
  assert.equal(ok.pressure, "ok");
  assert.equal(ok.pools[0].availableBytes, 0);
});

test("a loaded model without a known footprint lowers confidence and does not throw", async () => {
  const snapshot = await probe(injected({
    probeEngines: async () => engines({
      models: [{ id: "engine/model-a", loaded: true, configuredContextTokens: 8, slots: 1, slotsInUse: 0 }],
    }),
  }));
  assert.equal(snapshot.confidence, "unknown");
  assert.equal(snapshot.models[0].residentBytes, 0);
  assert.equal(snapshot.models[0].loaded, true);
  assert.equal(snapshot.pools[0].availableBytes, 4096);
});

test("no injected engine urls do not fetch and do not weaken a measured host", async () => {
  let called = 0;
  const snapshot = await probe(injected({
    probeEngines: undefined,
    task: "secret-prompt-text",
    fetchImpl: async () => {
      called += 1;
      throw new Error("network");
    },
  }));
  assert.equal(called, 0);
  assert.equal(snapshot.confidence, "measured");
  assert.deepEqual(snapshot.models, []);
});

test("engine collection fetches only injected http base urls and drops job text", async () => {
  const seen = [];
  const snapshot = await probe({
    nowMs: 20,
    headroomBytes: -4,
    task: "secret-prompt-text",
    probeHost: async () => host(),
    engineBaseUrls: [
      "http://user:pass@127.0.0.1:9",
      "file:///tmp/engine",
      "http://127.0.0.1:9/base",
      "not a url",
    ],
    fetchImpl: async (url, init) => {
      seen.push({ url: String(url), method: init?.method, body: init?.body ?? null });
      if (String(url).endsWith("/api/ps")) {
        return Response.json({
          models: [{
            name: "engine/model-a",
            size: 80,
            size_vram: 70,
            context_length: 256,
            prompt: "secret-prompt-text",
          }],
        });
      }
      return new Response("no", { status: 404 });
    },
  });
  assert.equal(snapshot.headroomBytes, 0);
  assert.equal(snapshot.confidence, "reported");
  assert.deepEqual(snapshot.models, [{
    id: "engine/model-a",
    residentBytes: 80,
    configuredContextTokens: 256,
    slots: 1,
    slotsInUse: 0,
    loaded: true,
  }]);
  assert.ok(seen.length > 0);
  for (const call of seen) {
    const url = new URL(call.url);
    assert.equal(url.origin, "http://127.0.0.1:9");
    assert.equal(url.pathname.startsWith("/base/"), true);
    assert.equal(call.method, "GET");
    assert.equal(call.body, null);
    assert.equal(call.url.includes("secret-prompt-text"), false);
    assert.equal(call.url.includes("user:pass"), false);
  }
  assert.equal(JSON.stringify(snapshot).includes("secret-prompt-text"), false);
});

test("a slot payload keeps model fields and never returns prompt text", async () => {
  const snapshot = await probe({
    nowMs: 21,
    probeHost: async () => host({ confidence: "reported" }),
    engineBaseUrls: ["https://engine.example:9"],
    fetchImpl: async (url) => {
      if (String(url).endsWith("/slots")) {
        return Response.json([
          {
            id: 0,
            model: "engine/model-a",
            n_ctx: 128,
            is_processing: true,
            prompt: "secret-prompt-text",
            resident_bytes: 15,
          },
          { id: 1, model: "engine/model-a", n_ctx: 128, is_processing: false, resident_bytes: 15 },
        ]);
      }
      if (String(url).endsWith("/props")) {
        return Response.json({ total_slots: 4, default_generation_settings: { n_ctx: 64 }, prompt: "secret-prompt-text" });
      }
      return new Response("no", { status: 404 });
    },
  });
  assert.equal(snapshot.confidence, "reported");
  assert.deepEqual(snapshot.models, [{
    id: "engine/model-a",
    residentBytes: 30,
    configuredContextTokens: 128,
    slots: 2,
    slotsInUse: 1,
    loaded: true,
  }]);
  assert.equal(JSON.stringify(snapshot).includes("secret-prompt-text"), false);
});

test("an engine that cannot be read returns unknown and does not throw", async () => {
  const failed = await probe({
    nowMs: 22,
    probeHost: async () => host(),
    engineBaseUrls: ["http://127.0.0.1:9"],
    fetchImpl: async () => {
      throw new Error("offline");
    },
  });
  assert.equal(failed.confidence, "unknown");
  assert.deepEqual(failed.models, []);
  assert.deepEqual(failed.pools, []);

  const catalogOnly = await probe({
    nowMs: 23,
    probeHost: async () => host(),
    engineBaseUrls: ["http://127.0.0.1:9"],
    fetchImpl: async (url) => {
      if (String(url).endsWith("/v1/models")) return Response.json({ data: [{ id: "engine/model-a" }] });
      return new Response("no", { status: 404 });
    },
  });
  assert.equal(catalogOnly.confidence, "unknown");
  assert.deepEqual(catalogOnly.pools, [{ id: "host", availableBytes: 4096 }]);
  assert.deepEqual(catalogOnly.models, []);
});

test("a listed loaded model without bytes is unknown rather than measured", async () => {
  const snapshot = await probe({
    nowMs: 24,
    probeHost: async () => host({ confidence: "reported" }),
    engineBaseUrls: ["http://127.0.0.1:9"],
    fetchImpl: async (url) => {
      if (String(url).endsWith("/api/v0/models")) {
        return Response.json({
          data: [
            { id: "engine/model-a", state: "loaded", max_context_length: 512 },
            { id: "engine/model-b", state: "not-loaded", max_context_length: 512 },
          ],
        });
      }
      return new Response("no", { status: 404 });
    },
  });
  assert.equal(snapshot.confidence, "unknown");
  assert.deepEqual(snapshot.models, [{
    id: "engine/model-a",
    residentBytes: 0,
    configuredContextTokens: 512,
    slots: 1,
    slotsInUse: 0,
    loaded: true,
  }]);
});

test("the real host path returns a snapshot without a network probe", async () => {
  let called = 0;
  const snapshot = await probe({
    nowMs: 25,
    fetchImpl: async () => {
      called += 1;
      throw new Error("network");
    },
  });
  assert.equal(called, 0);
  assert.equal(snapshot.observedAtMs, 25);
  assert.ok(["reported", "estimated"].includes(snapshot.confidence));
  assert.equal(snapshot.pressure, "ok");
  assert.equal(snapshot.models.length, 0);
  assert.equal(snapshot.pools.length, 1);
  assert.equal(snapshot.pools[0].id, "host");
  assert.equal(Number.isSafeInteger(snapshot.pools[0].availableBytes), true);
  assert.ok(snapshot.pools[0].availableBytes > 0);
  assert.equal(snapshot.headroomBytes, 0);
});

test("slots without a model id are unknown and do not throw", async () => {
  const snapshot = await probe({
    nowMs: 26,
    probeHost: async () => host(),
    engineBaseUrls: ["http://127.0.0.1:9"],
    fetchImpl: async (url) => {
      if (String(url).endsWith("/slots")) {
        return Response.json([{ id: 0, n_ctx: 32, prompt: "secret-prompt-text" }]);
      }
      return new Response("no", { status: 404 });
    },
  });
  assert.equal(snapshot.confidence, "unknown");
  assert.deepEqual(snapshot.models, []);
  assert.equal(JSON.stringify(snapshot).includes("secret-prompt-text"), false);
});

test("telemetry does not shell out or name a model", () => {
  const source = readFileSync(new URL("./telemetry.mjs", import.meta.url), "utf8");
  assert.equal(source.includes("child_process"), false);
  assert.equal(source.includes("execSync"), false);
  assert.equal(source.includes("spawn("), false);
  assert.equal(source.includes("nvidia-smi"), false);
  assert.equal(/qwen|llama-3|gpt-4|claude/i.test(source), false);
});
