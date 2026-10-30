// Host and engine observations for the capacity snapshot. No shell, no job text, no model ids.

import { freemem } from "node:os";

const CONFIDENCE = ["measured", "reported", "estimated", "unknown"];
const NO_ENGINE_URLS = Symbol("no-engine-urls");
const MAX_ENGINE_URLS = 8;
const MAX_BODY = 1_000_000;
const MAX_ID = 512;
const ENGINE_PATHS = ["/api/ps", "/api/v0/models", "/v1/models", "/slots", "/props"];

export async function probe(options = {}) {
  const settings = options && typeof options === "object" ? options : {};
  const observedAtMs = observedAt(settings);
  try {
    const [hostResult, engineResult] = await Promise.all([
      settle(resolveProbe(settings.probeHost, defaultProbeHost), settings),
      settle(resolveProbe(settings.probeEngines, defaultProbeEngines), settings),
    ]);
    if (!hostResult.ok || !engineResult.ok) return unknownSnapshot(observedAtMs, settings);
    return assemble(hostResult.value, engineResult.value, observedAtMs, settings);
  } catch {
    return unknownSnapshot(observedAtMs, settings);
  }
}

function resolveProbe(value, fallback) {
  if (value === undefined) return fallback;
  if (typeof value === "function") return value;
  return async () => null;
}

async function settle(fn, settings) {
  try {
    const value = await fn(settings);
    if (value == null || typeof value !== "object" || Array.isArray(value)) return { ok: false };
    return { ok: true, value };
  } catch {
    return { ok: false };
  }
}

function unknownSnapshot(observedAtMs, settings) {
  return {
    observedAtMs,
    confidence: "unknown",
    pressure: "ok",
    pools: [],
    models: [],
    headroomBytes: headroomFrom(settings, null),
  };
}

function assemble(host, engines, observedAtMs, settings) {
  const pools = mergePools(host.pools, engines.pools);
  if (pools.length === 0) return unknownSnapshot(observedAtMs, settings);
  const models = normalizeModels(engines.models);
  let confidence = weaker(asConfidence(host.confidence), engineConfidence(engines));
  if (models.some((model) => model.loaded && !model.residentKnown)) confidence = "unknown";
  return {
    observedAtMs,
    confidence,
    pressure: pressureOf(host, pools),
    pools: pools.map((pool) => ({ id: pool.id, availableBytes: pool.availableBytes })),
    models: models.map(publicModel),
    headroomBytes: headroomFrom(settings, host),
  };
}

function engineConfidence(engines) {
  if (engines[NO_ENGINE_URLS]) return null;
  return asConfidence(engines.confidence);
}

function asConfidence(value) {
  return CONFIDENCE.includes(value) ? value : "unknown";
}

function weaker(left, right) {
  if (right == null) return left;
  return CONFIDENCE[Math.max(CONFIDENCE.indexOf(left), CONFIDENCE.indexOf(right))];
}

function pressureOf(host, pools) {
  if (host.pressure === "high") return "high";
  if (host.pressure === "ok") return "ok";
  if (pools.length > 0 && pools.every((pool) => pool.availableBytes === 0)) return "high";
  return "ok";
}

function headroomFrom(settings, host) {
  const configured = nonNegInt(settings?.headroomBytes);
  if (configured != null) return configured;
  if (host) {
    const reported = nonNegInt(host.headroomBytes);
    if (reported != null) return reported;
  }
  return 0;
}

function observedAt(settings) {
  const now = settings?.nowMs;
  if (typeof now === "number" && Number.isFinite(now)) return Math.trunc(now);
  return Date.now();
}

function mergePools(...groups) {
  const byId = new Map();
  for (const group of groups) {
    if (!Array.isArray(group)) continue;
    for (const raw of group) {
      const pool = normalizePool(raw);
      if (!pool) continue;
      const existing = byId.get(pool.id);
      if (!existing || pool.availableBytes < existing.availableBytes) byId.set(pool.id, pool);
    }
  }
  return [...byId.values()];
}

function normalizePool(raw) {
  if (!raw || typeof raw !== "object") return null;
  const id = stringId(raw.id);
  const availableBytes = nonNegInt(raw.availableBytes);
  if (!id || availableBytes == null) return null;
  return { id, availableBytes };
}

function normalizeModels(rawModels) {
  if (!Array.isArray(rawModels)) return [];
  const byId = new Map();
  for (const raw of rawModels) {
    const model = normalizeModel(raw);
    if (!model) continue;
    const existing = byId.get(model.id);
    byId.set(model.id, existing ? combineModels(existing, model) : model);
  }
  return [...byId.values()];
}

function normalizeModel(raw) {
  if (!raw || typeof raw !== "object") return null;
  const id = stringId(raw.id);
  if (!id) return null;
  const resident = nonNegInt(raw.residentBytes);
  const slots = nonNegInt(raw.slots) ?? 1;
  const slotsInUse = Math.min(nonNegInt(raw.slotsInUse) ?? 0, slots);
  return {
    id,
    residentBytes: resident ?? 0,
    residentKnown: resident != null,
    configuredContextTokens: nonNegInt(raw.configuredContextTokens) ?? 0,
    slots,
    slotsInUse,
    loaded: raw.loaded === true,
  };
}

function combineModels(left, right) {
  const slots = Math.max(left.slots, right.slots);
  return {
    id: left.id,
    residentBytes: left.residentBytes + right.residentBytes,
    residentKnown: left.residentKnown && right.residentKnown,
    configuredContextTokens: Math.max(left.configuredContextTokens, right.configuredContextTokens),
    slots,
    slotsInUse: Math.min(slots, left.slotsInUse + right.slotsInUse),
    loaded: left.loaded || right.loaded,
  };
}

function publicModel(model) {
  return {
    id: model.id,
    residentBytes: model.residentBytes,
    configuredContextTokens: model.configuredContextTokens,
    slots: model.slots,
    slotsInUse: model.slotsInUse,
    loaded: model.loaded,
  };
}

function stringId(value) {
  if (typeof value !== "string") return null;
  const id = value.trim();
  if (!id || id.length > MAX_ID || /[\u0000-\u001f]/.test(id)) return null;
  return id;
}

function nonNegInt(value) {
  if (typeof value !== "number" || !Number.isFinite(value) || value < 0) return null;
  const floored = Math.floor(value);
  if (!Number.isSafeInteger(floored)) return null;
  return floored;
}

async function defaultProbeHost() {
  return hostFromOs();
}

function hostFromOs() {
  try {
    const available = nonNegInt(freemem());
    if (available == null) return null;
    return {
      confidence: "estimated",
      pressure: available === 0 ? "high" : "ok",
      pools: [{ id: "host", availableBytes: available }],
    };
  } catch {
    return null;
  }
}

async function defaultProbeEngines(options) {
  const bases = baseUrls(options?.engineBaseUrls);
  if (bases.length === 0) {
    return { models: [], pools: [], confidence: "measured", [NO_ENGINE_URLS]: true };
  }
  const fetchImpl = typeof options.fetchImpl === "function" ? options.fetchImpl : globalThis.fetch;
  if (typeof fetchImpl !== "function") return null;
  const models = [];
  let confidence = "reported";
  for (const base of bases) {
    const observation = await readEngine(base, fetchImpl, options);
    if (!observation) return null;
    models.push(...observation.models);
    confidence = weaker(confidence, observation.confidence);
  }
  return { models, pools: [], confidence };
}

function baseUrls(value) {
  if (!Array.isArray(value)) return [];
  const urls = [];
  for (const item of value) {
    if (typeof item !== "string" || urls.length >= MAX_ENGINE_URLS) continue;
    if (!ENGINE_PATHS.some((path) => joinUrl(item, path))) continue;
    if (!urls.includes(item)) urls.push(item);
  }
  return urls;
}

async function readEngine(base, fetchImpl, options) {
  const urls = ENGINE_PATHS.map((path) => joinUrl(base, path)).filter(Boolean);
  if (urls.length === 0) return null;
  const timeoutMs = boundedTimeout(options?.timeoutMs);
  const results = await Promise.all(urls.map(async (url) => {
    try {
      const response = await fetchImpl(url, {
        method: "GET",
        redirect: "manual",
        signal: AbortSignal.timeout(timeoutMs),
      });
      if (!response || response.ok !== true) return null;
      const json = await readBody(response);
      if (json == null) return null;
      return { kind: pathKind(url), json };
    } catch {
      return null;
    }
  }));
  const bodies = results.filter(Boolean);
  if (bodies.length === 0) return null;
  return interpret(bodies);
}

function boundedTimeout(value) {
  const parsed = nonNegInt(value);
  if (parsed == null || parsed === 0) return 1000;
  return Math.min(parsed, 5000);
}

function joinUrl(base, path) {
  let url;
  try {
    url = new URL(base);
  } catch {
    return null;
  }
  if (url.protocol !== "http:" && url.protocol !== "https:") return null;
  if (url.username || url.password || !url.hostname) return null;
  const prefix = url.pathname.replace(/\/+$/, "");
  url.pathname = `${prefix}${path}`;
  url.hash = "";
  return url.href;
}

function pathKind(url) {
  const pathname = new URL(url).pathname;
  if (pathname.endsWith("/api/ps")) return "ps";
  if (pathname.endsWith("/api/v0/models")) return "v0";
  if (pathname.endsWith("/v1/models")) return "v1";
  if (pathname.endsWith("/slots")) return "slots";
  if (pathname.endsWith("/props")) return "props";
  return null;
}

async function readBody(response) {
  const length = Number(response.headers?.get?.("content-length"));
  if (Number.isFinite(length) && length > MAX_BODY) return null;
  try {
    if (typeof response.text === "function") {
      const text = await response.text();
      if (typeof text !== "string" || text.length > MAX_BODY) return null;
      return JSON.parse(text);
    }
    if (typeof response.json === "function") return await response.json();
  } catch {
    return null;
  }
  return null;
}

function interpret(bodies) {
  const byKind = new Map(bodies.map((body) => [body.kind, body.json]));
  const props = byKind.get("props");
  const loaded = fromPs(byKind.get("ps")) || fromSlots(byKind.get("slots")) || fromV0(byKind.get("v0"));
  if (!loaded) return { models: [], confidence: "unknown" };
  if (loaded.incomplete) return { models: loaded.models, confidence: "unknown" };
  const models = loaded.models.map((model) => supplement(model, props, loaded.models.length));
  const confidence = models.some((model) => model.loaded && model.residentBytes == null) ? "unknown" : "reported";
  return { models, confidence };
}

function fromPs(body) {
  if (!body || typeof body !== "object" || Array.isArray(body) || !Array.isArray(body.models)) return null;
  const objects = body.models.filter((item) => item && typeof item === "object");
  const models = objects.map((item) => runningModel(item)).filter(Boolean);
  if (models.length !== objects.length) return { models, incomplete: true };
  return { models };
}

function runningModel(item) {
  if (!item || typeof item !== "object") return null;
  const id = stringId(item.name) || stringId(item.model) || stringId(item.id);
  if (!id) return null;
  return {
    id,
    residentBytes: maxKnown(item.size_vram, item.size, item.resident_bytes),
    configuredContextTokens: nonNegInt(item.context_length) ?? nonNegInt(item.n_ctx),
    slots: nonNegInt(item.slots),
    slotsInUse: nonNegInt(item.slots_in_use),
    loaded: true,
  };
}

function maxKnown(...values) {
  const known = values.map(nonNegInt).filter((value) => value != null);
  if (known.length === 0) return null;
  return Math.max(...known);
}

function fromSlots(body) {
  if (!Array.isArray(body)) return null;
  const slots = body.filter((slot) => slot && typeof slot === "object");
  if (slots.some((slot) => !slotModelId(slot))) return { models: [], incomplete: true };
  const byId = new Map();
  for (const slot of body) {
    if (!slot || typeof slot !== "object") continue;
    const id = slotModelId(slot);
    if (!id) continue;
    const existing = byId.get(id) || {
      id,
      residentBytes: null,
      configuredContextTokens: null,
      slots: 0,
      slotsInUse: 0,
      loaded: true,
    };
    existing.slots += 1;
    if (slot.is_processing === true || slot.state === "processing") existing.slotsInUse += 1;
    const context = nonNegInt(slot.n_ctx);
    if (context != null) existing.configuredContextTokens = context;
    const resident = nonNegInt(slot.resident_bytes) ?? nonNegInt(slot.size);
    if (resident != null) existing.residentBytes = (existing.residentBytes ?? 0) + resident;
    byId.set(id, existing);
  }
  return { models: [...byId.values()] };
}

function slotModelId(slot) {
  return stringId(slot.model) || stringId(slot.model_name) || stringId(slot.model_path);
}

function fromV0(body) {
  const data = modelList(body);
  if (!data) return null;
  const states = data.some((item) => item && typeof item === "object" && ("state" in item || "loaded" in item));
  if (!states) return null;
  return {
    models: data.filter(isListedLoaded).map((item) => ({
      id: stringId(item.id) || stringId(item.name),
      residentBytes: nonNegInt(item.resident_bytes) ?? nonNegInt(item.size),
      configuredContextTokens: nonNegInt(item.loaded_context_length) ?? nonNegInt(item.max_context_length),
      slots: nonNegInt(item.slots),
      slotsInUse: nonNegInt(item.slots_in_use),
      loaded: true,
    })).filter((item) => item.id),
  };
}

function modelList(body) {
  if (!body || typeof body !== "object" || !Array.isArray(body.data)) return null;
  return body.data;
}

function isListedLoaded(item) {
  if (!item || typeof item !== "object") return false;
  if (item.loaded === true || item.state === "loaded" || item.state === "running") return true;
  return false;
}

function supplement(model, props, count) {
  if (!props || typeof props !== "object" || Array.isArray(props) || count !== 1) return model;
  const context = nonNegInt(props.default_generation_settings?.n_ctx);
  const slots = nonNegInt(props.total_slots);
  return {
    ...model,
    configuredContextTokens: model.configuredContextTokens ?? context,
    slots: model.slots ?? slots,
  };
}
