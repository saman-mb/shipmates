import { decideSpawn, offState, parseState, switchChoices, ORDER_PREAMBLE } from "./policy.mjs";
import { mkdirSync, readFileSync, renameSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join } from "node:path";

const STATE_NAME = "shipmates-crew.json";
const inFlight = new Map();

function agentDir() {
  return process.env.PI_CODING_AGENT_DIR || join(homedir(), ".pi", "agent");
}

function statePath() {
  return join(agentDir(), STATE_NAME);
}

function readState() {
  try {
    return parseState(readFileSync(statePath(), "utf8"));
  } catch {
    return offState();
  }
}

function writeState(state) {
  const path = statePath();
  mkdirSync(dirname(path), { recursive: true });
  const tmp = `${path}.tmp`;
  writeFileSync(tmp, JSON.stringify({ ...state, schemaVersion: 1 }, null, 2));
  renameSync(tmp, path);
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

export default function (pi) {
  pi.registerCommand("shipmates-crew", {
    description: "Connect local servers and choose Off, Split, or Local for the crew.",
    handler: async (_args, ctx) => {
      const current = readState();
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
      writeState({ ...current, switch: next });
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

  pi.on("session_shutdown", () => {
    inFlight.clear();
  });

  pi.on("tool_call", (event, ctx) => {
    if (event.toolName !== "subagent") return;
    const action = event.input?.action;
    if (typeof action === "string" && ["status", "list", "guide", "cancel", "message"].includes(action)) {
      return;
    }
    if (event.input?.workflow) return;

    const state = readState();
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
    const seats = spawnInputs(event.input, sessionModel);
    if (seats.length === 0) {
      if (state.switch === "Off") return;
      return { block: true, reason: "Refused: no model named. I will not pick one." };
    }
    const runningAll = inFlight.size;
    const runningLocal = [...inFlight.values()].filter((tag) => tag === "local").length;
    for (const seat of seats) {
      // inherit / empty must resolve to the session model before tagging.
      // Otherwise "inherit" is treated as a cloud id and doer seats are refused.
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
        busy: [...inFlight.keys()],
        cloudModels: registryCloud,
        runningLocal,
        runningAll,
        task: seat.task,
      });
      if (decision.action === "block") return { block: true, reason: decision.reason };
      if (decision.prepend && typeof event.input?.task === "string" && !event.input.task.startsWith(ORDER_PREAMBLE)) {
        event.input.task = `${ORDER_PREAMBLE}\n${event.input.task}`;
      }
      if (seat.model) inFlight.set(seat.model, decision.honoured ? modelTag(seat.model, localIds) : "local");
    }
  });

  pi.on("tool_execution_end", (event) => {
    if (event.toolName !== "subagent") return;
    const model = event.input?.model;
    if (model) inFlight.delete(model);
  });
}
