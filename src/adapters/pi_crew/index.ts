import { brokerWanted, captainReserved, decideSpawn, offState, parseState, switchChoices, ORDER_PREAMBLE } from "./policy.mjs";
import { brokerEndpoint, ipcListenPath, startBroker } from "./broker.mjs";
import { createMessageConnection, createRpcClient } from "./protocol.mjs";
import { probe } from "./telemetry.mjs";
import net from "node:net";
import { spawn } from "node:child_process";
import { randomUUID } from "node:crypto";
import { closeSync, mkdirSync, openSync, readFileSync, renameSync, unlinkSync, writeFileSync, writeSync } from "node:fs";
import { freemem, homedir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const STATE_NAME = "shipmates-crew.json";

// Exact pins. The runtime self-provisions these; tests never install them.
export const RUNTIME_PINS = {
  "systeminformation": "5.33.18",
  "vscode-jsonrpc": "10.0.0",
};

function agentDir() {
  return process.env.PI_CODING_AGENT_DIR || join(homedir(), ".pi", "agent");
}

function statePath() {
  return join(agentDir(), STATE_NAME);
}

export function readState(from = statePath()) {
  try {
    return parseState(readFileSync(from, "utf8"));
  } catch {
    return offState();
  }
}

export function writeState(state, target = statePath()) {
  const incoming = state && typeof state === "object" ? state : {};
  const parsed = incoming.schemaVersion > 1
    ? offState()
    : parseState({ ...offState(), ...incoming, schemaVersion: 1 });
  mkdirSync(dirname(target), { recursive: true });
  const tmp = `${target}.tmp`;
  writeFileSync(tmp, JSON.stringify(parsed, null, 2));
  renameSync(tmp, target);
  return parsed;
}

function modelId(model) {
  if (!model) return "";
  if (typeof model === "string") return model;
  const provider = model.provider ?? "";
  const id = model.id ?? "";
  return provider && id ? `${provider}/${id}` : String(id || provider);
}

function isZeroCost(cost) {
  return (
    cost &&
    cost.input === 0 &&
    cost.output === 0 &&
    cost.cacheRead === 0 &&
    cost.cacheWrite === 0
  );
}

function classifyModels(models) {
  const local = [];
  const cloud = [];
  for (const m of models ?? []) {
    const id = modelId(m);
    if (!id) continue;
    if (isZeroCost(m.cost)) local.push(id);
    else cloud.push(id);
  }
  return { local, cloud };
}

function modelTag(model, localIds) {
  if (!model) return null;
  return localIds.has(model) ? "local" : "cloud";
}

function chatModel(ctx) {
  if (!ctx?.model?.provider || !ctx.model.id) return null;
  return `${ctx.model.provider}/${ctx.model.id}`;
}

/** Resolve spawn model identity. `inherit` / empty means the parent session model. */
function resolveModel(model, sessionModel) {
  if (model == null || model === "" || model === "inherit") {
    return modelId(sessionModel) || null;
  }
  return typeof model === "string" ? model : modelId(model);
}

function spawnInputs(input, sessionModel) {
  const seats = [];
  if (input && typeof input === "object") {
    if (input.agent || input.model) {
      seats.push({
        role: input.agent,
        model: resolveModel(input.model, sessionModel),
        task: input.task,
        rawModel: input.model,
      });
    }
    for (const key of ["tasks", "runs"]) {
      const items = input.args?.[key] || input[key];
      if (Array.isArray(items)) {
        for (const item of items) {
          if (item && typeof item === "object") {
            seats.push({
              role: item.agent,
              model: resolveModel(item.model, sessionModel),
              task: item.task,
              rawModel: item.model,
            });
          }
        }
      }
    }
  }
  return seats;
}

function localSeat(state, model, localIds) {
  if (state.switch === "Local") return true;
  return modelTag(model, localIds) === "local";
}

function pushId(ids, value) {
  if (typeof value === "string" && value.length > 0 && !ids.includes(value)) ids.push(value);
}

export function runIdsIn(result) {
  const ids = [];
  const visit = (value, depth) => {
    if (!value || depth > 5 || typeof value !== "object") return;
    if (Array.isArray(value)) {
      for (const item of value) visit(item, depth + 1);
      return;
    }
    pushId(ids, value.runId);
    if (Array.isArray(value.runIds)) {
      for (const item of value.runIds) pushId(ids, item);
    }
    for (const key of ["details", "result", "structuredContent"]) visit(value[key], depth + 1);
    if (Array.isArray(value.content)) {
      for (const part of value.content) {
        if (part && typeof part.text === "string") {
          try {
            visit(JSON.parse(part.text), depth + 1);
          } catch {
            // Tool text is not a run id unless it is JSON with that field.
          }
        }
      }
    }
  };
  visit(result, 0);
  return ids;
}

function provisioningAllowed() {
  if (process.env.NODE_TEST_CONTEXT || process.env.SHIPMATES_CREW_NO_PROVISION === "1") return false;
  return !import.meta.url.includes("/src/adapters/pi_crew/");
}

function runtimeRoot() {
  return join(dirname(fileURLToPath(import.meta.url)), ".shipmates-runtime");
}

function fileJournal(dir) {
  const file = join(dir, "journal.jsonl");
  mkdirSync(dir, { recursive: true });
  return {
    append(record) {
      try {
        writeFileSync(file, `${JSON.stringify(record)}\n`, { flag: "a" });
      } catch {
        // A missing journal does not admit extra work.
      }
    },
  };
}

function packagesInstalled(root) {
  return Object.keys(RUNTIME_PINS).every((name) => {
    try {
      readFileSync(join(root, "node_modules", name, "package.json"));
      return true;
    } catch {
      return false;
    }
  });
}

function ensureRuntimePackages() {
  const root = runtimeRoot();
  if (!provisioningAllowed()) return Promise.resolve(root);
  mkdirSync(root, { recursive: true });
  const desired = JSON.stringify({ private: true, dependencies: RUNTIME_PINS }, null, 2);
  const manifest = join(root, "package.json");
  let current = "";
  try {
    current = readFileSync(manifest, "utf8");
  } catch {
    current = "";
  }
  if (current !== desired) writeFileSync(manifest, desired);
  if (packagesInstalled(root)) return Promise.resolve(root);
  const npm = process.platform === "win32" ? "npm.cmd" : "npm";
  return new Promise((resolve) => {
    let child;
    const finish = () => resolve(root);
    try {
      child = spawn(npm, ["install", "--ignore-scripts", "--no-audit", "--no-fund", "--prefix", root], {
        stdio: "ignore",
      });
    } catch {
      finish();
      return;
    }
    const timer = setTimeout(() => {
      child.kill();
      finish();
    }, 20000);
    child.on("error", () => {
      clearTimeout(timer);
      finish();
    });
    child.on("close", () => {
      clearTimeout(timer);
      finish();
    });
  });
}

async function vendored(name, entry) {
  try {
    const root = await ensureRuntimePackages();
    const spec = pathToFileURL(join(root, "node_modules", name, entry)).href;
    return await import(spec);
  } catch {
    return null;
  }
}

async function runtimeProbeHost() {
  const mod = await vendored("systeminformation", "lib/index.js");
  try {
    const mem = typeof mod?.mem === "function" ? mod.mem : mod?.default?.mem;
    if (typeof mem === "function") {
      const info = await mem();
      const raw = Number.isFinite(info?.available) ? info.available : info?.free;
      const available = Math.floor(raw);
      if (Number.isSafeInteger(available) && available >= 0) {
        return {
          confidence: "reported",
          pressure: available === 0 ? "high" : "ok",
          pools: [{ id: "host", availableBytes: available }],
        };
      }
    }
  } catch {
    // Fall through to the OS reading.
  }
  const available = freemem();
  if (!Number.isSafeInteger(available) || available < 0) return null;
  return {
    confidence: "estimated",
    pressure: available === 0 ? "high" : "ok",
    pools: [{ id: "host", availableBytes: available }],
  };
}

async function rpcOver(socket) {
  const vendoredRpc = await vendored("vscode-jsonrpc", "lib/node/main.js");
  if (vendoredRpc?.createMessageConnection && vendoredRpc.StreamMessageReader && vendoredRpc.StreamMessageWriter) {
    const connection = vendoredRpc.createMessageConnection(
      new vendoredRpc.StreamMessageReader(socket),
      new vendoredRpc.StreamMessageWriter(socket),
    );
    connection.listen();
    return {
      request(method, params) {
        return connection.sendRequest(method, params);
      },
    };
  }
  const connection = createMessageConnection(socket, socket);
  const client = createRpcClient(connection);
  return {
    request(method, params) {
      return client.request(method, params);
    },
  };
}

function spawnDaemon(endpoint) {
  const child = spawn(process.execPath, [
    "--experimental-strip-types",
    fileURLToPath(import.meta.url),
    "--shipmates-broker-daemon",
  ], {
    detached: true,
    stdio: "ignore",
    env: {
      ...process.env,
      SHIPMATES_BROKER_HOME: endpoint.home,
      SHIPMATES_BROKER_PATH: endpoint.listenPath,
    },
  });
  child.unref();
}

async function waitForBroker(listenPath) {
  for (let attempt = 0; attempt < 20; attempt += 1) {
    const client = await tryConnect(listenPath);
    if (client) return client;
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  return null;
}

function lockIsLive(lockPath) {
  try {
    const pid = Number(readFileSync(lockPath, "utf8"));
    if (!Number.isInteger(pid) || pid <= 0) return false;
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

export async function startDaemon() {
  const home = process.env.SHIPMATES_BROKER_HOME;
  const listenPath = process.env.SHIPMATES_BROKER_PATH;
  if (!home || !listenPath) throw new Error("broker daemon is missing its endpoint");
  mkdirSync(home, { recursive: true });
  const lockPath = join(home, "broker.lock");
  if (!takeLock(lockPath)) return;
  writeFileSync(join(home, "endpoint.json"), JSON.stringify({
    protocolVersion: 1,
    listenPath,
    pid: process.pid,
  }));
  await startBroker({
    path: listenPath,
    journal: fileJournal(home),
    probe: () => probe({
      probeHost: runtimeProbeHost,
      engineBaseUrls: readState().engineBaseUrls,
      headroomBytes: readState().headroomBytes,
    }),
  });
  setInterval(() => {}, 1 << 30);
}

function takeLock(lockPath) {
  try {
    const fd = openSync(lockPath, "wx");
    writeSync(fd, String(process.pid));
    closeSync(fd);
    return true;
  } catch (error) {
    if (error?.code !== "EEXIST") return false;
    if (lockIsLive(lockPath)) return false;
    try {
      unlinkSync(lockPath);
    } catch {
      return false;
    }
    return takeLock(lockPath);
  }
}

function tryConnect(listenPath) {
  return new Promise((resolve) => {
    const socket = net.connect(listenPath);
    const fail = () => {
      socket.removeAllListeners();
      socket.destroy();
      resolve(null);
    };
    socket.once("error", fail);
    socket.once("connect", () => {
      socket.off("error", fail);
      socket.on("error", () => {});
      resolve(rpcOver(socket));
    });
  });
}

function admitParams(runId, sessionId, role, model, waiting) {
  return {
    runId,
    sessionId,
    role: role || "crew",
    model,
    pinned: true,
    protocolVersion: 1,
    contextTokens: 0,
    outputReserveTokens: 0,
    priority: 0,
    enqueuedAtMs: Date.now(),
    waiting,
    wait: waiting === false ? false : undefined,
  };
}

export function install(pi, deps = {}) {
  let seatAbortSignals = deps.seatAbortSignals ?? new Map();
  const read = deps.readState ?? readState;
  const connect = deps.connect ?? defaultConnect;
  const notes = deps.notes ?? [];
  const leases = deps.leases ?? new Map();
  const seats = new Map();
  const timers = new Map();
  const sessionId = deps.sessionId ?? randomUUID();
  let captainKey = null;
  let clientPromise = null;
  let brokerHandle = null;

  async function defaultConnect() {
    if (process.env.NODE_TEST_CONTEXT) {
      throw new Error("refusing to start a broker from the test runner");
    }
    if (clientPromise) return clientPromise;
    clientPromise = openClient().catch((error) => {
      clientPromise = null;
      throw error;
    });
    return clientPromise;
  }

  async function openClient() {
    const endpoint = brokerEndpoint(agentDir());
    const existing = await tryConnect(endpoint.listenPath);
    if (existing) return existing;
    await ensureRuntimePackages();
    spawnDaemon(endpoint);
    const started = await waitForBroker(endpoint.listenPath);
    if (started) return started;
    throw new Error("broker did not accept a connection");
  }

  function busyModels() {
    return [...new Set([...seats.values()].map((seat) => seat.model).filter(Boolean))];
  }

  function runningLocal() {
    return [...seats.values()].filter((seat) => seat.tag === "local").length;
  }

  function rememberSeat(seatKey, toolCallId, model, tag) {
    seats.set(seatKey, { seatKey, toolCallId, model, tag });
  }

  function rememberLease(record) {
    leases.set(record.seatKey, record);
    if (deps.heartbeat !== false && Number.isInteger(record.generation)) {
      const timer = setInterval(() => {
        connect()
          .then((client) => client.request("heartbeat", { runId: record.brokerRunId, generation: record.generation }))
          .catch(() => {});
      }, 10000);
      timer.unref?.();
      timers.set(record.seatKey, timer);
    }
  }

  function rekeyLease(record, runId) {
    if (!record || typeof runId !== "string" || runId.length === 0 || runId === record.seatKey) return;
    record.runId = runId;
    leases.set(runId, record);
    notes.push({ type: "rekey", from: record.seatKey, to: runId, brokerRunId: record.brokerRunId });
  }

  async function releaseRecord(record) {
    if (!record) return;
    const timer = timers.get(record.seatKey);
    if (timer) clearInterval(timer);
    timers.delete(record.seatKey);
    leases.delete(record.seatKey);
    if (record.runId) leases.delete(record.runId);
    seats.delete(record.seatKey);
    if (!Number.isInteger(record.generation)) return;
    try {
      const client = await connect();
      await client.request("release", { runId: record.brokerRunId, generation: record.generation });
    } catch {
      // No later hold hook is published. A missed release expires on the broker.
    }
  }

  async function releaseToolCall(toolCallId, result) {
    const owned = [...leases.values()].filter((record) => record.toolCallId === toolCallId);
    const runIds = runIdsIn(result);
    if (owned.length === 1 && runIds.length === 1) rekeyLease(owned[0], runIds[0]);
    else if (owned.length > 1 && owned.length === runIds.length) {
      owned.forEach((record, index) => rekeyLease(record, runIds[index]));
    }
    for (const record of owned) await releaseRecord(record);
    for (const [seatKey, seat] of [...seats.entries()]) {
      if (seat.toolCallId === toolCallId) seats.delete(seatKey);
    }
  }

  async function askBroker(state, runId, role, model, signal) {
    const shadow = state.resources === "shadow";
    let client;
    try {
      client = await connect();
    } catch (error) {
      if (shadow) {
        notes.push({ type: "shadow", action: "unreachable", model, runId, reason: "broker unreachable" });
        return { ok: true };
      }
      return {
        ok: false,
        reason: "Refused: the resource broker cannot be reached. Not started. I will not substitute a model.",
      };
    }
    try {
      const response = client.request("admit", {
        ...admitParams(runId, sessionId, role, model),
        contextTokens: state.contextTokens,
        outputReserveTokens: state.contextTokens,
        wait: !shadow,
        dryRun: shadow,
      });
      const result = await (signal
        ? Promise.race([
            response,
            new Promise((_, reject) => {
              if (signal.aborted) return reject(new Error("aborted"));
              signal.addEventListener("abort", () => reject(new Error("aborted")), { once: true });
            }),
          ])
        : response);
      if (signal?.aborted) throw new Error("aborted");
      if (shadow) {
        notes.push({
          type: "shadow",
          action: result?.action ?? "unknown",
          model,
          runId,
          queuePosition: result?.queuePosition ?? null,
          reason: typeof result?.reason === "string" ? result.reason : "",
        });
        // Shadow observes. It never holds a lease, so it cannot consume
        // capacity or leave a reservation behind if the run dies.
        return { ok: true };
      }
      if (result?.action === "grant" && result.lease && Number.isInteger(result.lease.generation ?? result.generation)) {
        return { ok: true, lease: result.lease, generation: result.lease.generation ?? result.generation };
      }
      const reason = typeof result?.reason === "string" && result.reason
        ? result.reason
        : "Waiting: the resource broker did not grant this seat.";
      return { ok: false, reason: `${reason} This seat stays on ${model}. I will not substitute a model.` };
    } catch (error) {
      if (signal?.aborted) {
        client.request("cancel", { runId }).catch(() => {});
        return { ok: false, reason: "Aborted: this seat was cancelled while waiting." };
      }
      if (shadow) {
        notes.push({ type: "shadow", action: "unreachable", model, runId, reason: "broker unreachable" });
        return { ok: true };
      }
      return {
        ok: false,
        reason: "Refused: the resource broker cannot be reached. Not started. I will not substitute a model.",
      };
    }
  }

  pi.registerCommand("shipmates-crew", {
    description: "Connect local servers and choose Off, Split, or Local for the crew.",
    handler: async (_args, ctx) => {
      const current = read();
      ctx.ui.notify(`Switch is ${current.switch}. Setup does not turn it on.`, "info");
      const next = await ctx.ui.select(`Switch (current: ${current.switch})`, switchChoices(current.switch));
      if (!next || next === current.switch) {
        ctx.ui.notify(`Switch is ${current.switch}. This chat's model is unchanged.`, "info");
        return;
      }
      if (next === "Local" && ctx.model) {
        const currentId = `${ctx.model.provider}/${ctx.model.id}`;
        const { local: registryLocal } = classifyModels(ctx.modelRegistry?.getAvailable?.());
        const choices = current.ticks.length ? current.ticks : registryLocal;
        if (!current.ticks.includes(currentId)) {
          const choice = await ctx.ui.select("Which local model should this chat move to?", choices);
          if (!choice) {
            ctx.ui.notify("No local model to move to. Switch unchanged. /model unchanged.", "warning");
            return;
          }
          writeState({ ...current, switch: "Local" });
          if (ctx.modelRegistry?.find) {
            const [provider, ...rest] = choice.split("/");
            const found = ctx.modelRegistry.find(provider, rest.join("/"));
            if (found) await ctx.setModel(found);
          }
          ctx.ui.notify(`Switch is Local. This chat is now on ${choice}.`, "info");
          return;
        }
      }
      writeState({ ...current, switch: next }, deps.statePath);
      ctx.ui.notify(`Saved. Switch is ${next}.`, "info");
    },
  });

  pi.on("session_start", async () => {
    if (process.env.PI_SUBAGENT_CHILD) return;
    try {
      const { registerRequiredChildExtensions } = await import("pi-subagents/required-child-extensions");
      registerRequiredChildExtensions({ id: "shipmates-crew", path: import.meta.url });
    } catch {
      // pi-subagents may be absent. The command still registers. A hidden script spawn is refused.
    }
  });

  pi.on("session_shutdown", async () => {
    for (const record of [...leases.values()]) await releaseRecord(record);
    seats.clear();
    captainKey = null;
    // Do not close/unlink the machine-wide broker here. Another pi session may
    // still be connected to the same admission socket; tearing it down races them
    // (ENOENT/ECONNREFUSED on heartbeat and admit). Process exit drops the listen
    // handle; a later startBroker only unlinks a path after a failed dial proves
    // it is stale.
    brokerHandle = null;
    clientPromise = null;
  });

  pi.on("turn_start", async (event, ctx) => {
    if (process.env.PI_SUBAGENT_CHILD) return;
    const state = read();
    if (!captainReserved(state)) return;
    const model = chatModel(ctx);
    if (!model) {
      ctx.ui?.notify?.("Refused: no model named for this chat. I will not pick one.", "error");
      ctx.abort?.();
      return;
    }
    // The local broker governs local work. A cloud chat is not a local seat, so
    // reserving it would park a model the broker never holds and, under
    // enforce, stop the captain's own chat for no reason. Only a model the
    // registry positively reports as cloud is skipped; an unreadable registry
    // keeps the previous behaviour.
    const { cloud } = classifyModels(ctx?.modelRegistry?.getAvailable?.());
    if (cloud.includes(model) && !state.ticks.includes(model)) return;
    if (captainKey) await releaseRecord(leases.get(captainKey));
    const seatKey = `captain:${sessionId}:${event?.turnIndex ?? 0}`;
    const admission = await askBroker(state, seatKey, "captain", model);
    if (!admission.ok) {
      ctx.ui?.notify?.(admission.reason, "error");
      ctx.abort?.();
      return;
    }
    captainKey = seatKey;
    if (admission.lease) {
      rememberLease({
        seatKey,
        toolCallId: seatKey,
        brokerRunId: admission.lease.runId || seatKey,
        generation: admission.generation,
        model,
        role: "captain",
      });
    }
  });

  pi.on("turn_end", async () => {
    if (!captainKey) return;
    const key = captainKey;
    captainKey = null;
    await releaseRecord(leases.get(key));
  });

  pi.on("tool_call", async (event, ctx) => {
    if (event.toolName !== "subagent") return;
    const state = read();
    const action = event.input?.action;
    if (typeof action === "string" && ["status", "list", "guide", "cancel", "message"].includes(action)) {
      return;
    }
    if (event.input?.workflow) return;

    const available = ctx?.modelRegistry?.getAvailable?.();
    let registryLocal;
    let registryCloud;
    if (available) {
      ({ local: registryLocal, cloud: registryCloud } = classifyModels(available));
    } else {
      registryLocal = state.ticks;
      registryCloud = [];
    }
    const localIds = new Set(registryLocal);
    const sessionModel = ctx?.model;
    const seatsInCall = spawnInputs(event.input, sessionModel);
    if (seatsInCall.length === 0) {
      if (state.switch === "Off") return;
      return { block: true, reason: "Refused: no model named. I will not pick one." };
    }
    const toolCallId = typeof event.toolCallId === "string" && event.toolCallId ? event.toolCallId : randomUUID();
    const granted = [];
    for (let index = 0; index < seatsInCall.length; index += 1) {
      const seat = seatsInCall[index];
      if ((seat.rawModel === "inherit" || seat.rawModel == null || seat.rawModel === "") && !seat.model) {
        return {
          block: true,
          reason:
            "Refused: model inherit needs a session model, and this session has none. Name a concrete model and retry. I will not pick one.",
        };
      }
      const decision = decideSpawn({
        state,
        role: seat.role,
        model: seat.model,
        modelTag: modelTag(seat.model, localIds),
        loaded: registryLocal,
        busy: busyModels(),
        cloudModels: registryCloud,
        runningLocal: runningLocal(),
        runningAll: seats.size,
        task: seat.task,
      });
      if (decision.action === "block") {
        for (const record of granted) await releaseRecord(record);
        return { block: true, reason: decision.reason };
      }
      if (decision.prepend && typeof event.input?.task === "string" && !event.input.task.startsWith(ORDER_PREAMBLE)) {
        event.input.task = `${ORDER_PREAMBLE}\n${event.input.task}`;
      }
      const named = seat.model;
      const kept = named;
      const seatKey = `${toolCallId}#${index}`;
      if (brokerWanted(state) && localSeat(state, named, localIds)) {
        const before = notes.length;
        const signal = seatAbortSignals.get(toolCallId);
        const admission = await askBroker(state, seatKey, seat.role, named, signal);
        if (state.resources === "shadow" && notes.length > before) {
          const note = notes[notes.length - 1];
          const where = note.queuePosition == null ? "not queued" : `queue position ${note.queuePosition}`;
          ctx?.ui?.notify?.(
            `Shadow: ${named} would ${note.action} (${where}). This seat stays on ${named}. Not blocked. Not moved.`,
            "info",
          );
        }
        if (!admission.ok) {
          for (const record of granted) await releaseRecord(record);
          return { block: true, reason: admission.reason };
        }
        if (event.input && event.input.model && event.input.model !== kept) event.input.model = kept;
        if (admission.lease) {
          const record = {
            seatKey,
            toolCallId,
            brokerRunId: admission.lease.runId || seatKey,
            generation: admission.generation,
            model: named,
            role: seat.role,
          };
          rememberLease(record);
          granted.push(record);
        }
      }
      if (named) rememberSeat(seatKey, toolCallId, named, decision.honoured ? modelTag(named, localIds) : "local");
    }
  });

  // pi publishes tool_execution_end. pi-subagents does not publish a hold hook
  // this extension can see, so a lease is not described as covering more than that.
  pi.on("tool_execution_start", async (event, ctx) => {
    if (event.toolName !== "subagent") return;
    if (!event.toolCallId || !ctx?.signal) return;
    seatAbortSignals.set(event.toolCallId, ctx.signal);
    ctx.signal.addEventListener("abort", () => {
      seatAbortSignals.delete(event.toolCallId);
    }, { once: true });
  });

  pi.on("tool_execution_end", async (event) => {
    if (event.toolName !== "subagent") return;
    if (!event.toolCallId) return;
    seatAbortSignals.delete(event.toolCallId);
    await releaseToolCall(event.toolCallId, event.result);
  });

  return { notes, leases, seats };
}

export default function (pi) {
  return install(pi);
}

if (process.argv.includes("--shipmates-broker-daemon")) {
  startDaemon().catch((error) => {
    console.error(error);
    process.exit(1);
  });
}
