// Pure decisions for the pi crew switch. No I/O, no model ids, no ceiling constant.

export const DOERS = ["senior-engineer", "sdet"];

export const JUDGES = [
  "architect",
  "product-manager",
  "principal-engineer",
  "security-engineer",
  "ux-ui-designer",
  "art-director",
  "performance-engineer",
  "site-reliability-engineer",
  "data-scientist",
  "devops-engineer",
  "technical-writer",
];

export const ROLES = [...DOERS, ...JUDGES];

export const SHIPPED_TAG = Object.fromEntries([
  ...DOERS.map((role) => [role, "local"]),
  ...JUDGES.map((role) => [role, "cloud"]),
]);

export const ORDER_HEADINGS = ["Outcome:", "Owned paths:", "Done-when:", "Out of scope:"];

export const ORDER_PREAMBLE =
  "Follow the order. Do not widen it. Stop and report if it is ambiguous.";

export function offState() {
  return { schemaVersion: 1, switch: "Off", roles: {}, ticks: [], ceiling: null };
}

export function parseState(raw) {
  let obj = raw;
  if (typeof raw === "string") {
    if (!raw.trim()) return offState();
    try {
      obj = JSON.parse(raw);
    } catch {
      return offState();
    }
  }
  if (typeof obj !== "object" || obj === null) return offState();
  if (obj.schemaVersion !== undefined && obj.schemaVersion > 1) return offState();
  if (!["Off", "Split", "Local"].includes(obj.switch)) return offState();

  const roles = {};
  if (obj.roles && typeof obj.roles === "object") {
    for (const [role, tag] of Object.entries(obj.roles)) {
      if (SHIPPED_TAG[role] && (tag === "local" || tag === "cloud")) roles[role] = tag;
    }
  }
  const ticks = Array.isArray(obj.ticks) ? obj.ticks.filter((item) => typeof item === "string") : [];
  let ceiling = null;
  if (Number.isInteger(obj.ceiling) && obj.ceiling >= 0) ceiling = obj.ceiling;

  return { schemaVersion: 1, switch: obj.switch, roles, ticks, ceiling };
}

export function isDoer(role) {
  return DOERS.includes(role);
}

export function isKnownRole(role) {
  return Object.hasOwn(SHIPPED_TAG, role);
}

export function effectiveTag(state, role) {
  if (state.switch !== "Split") return null;
  const flipped = state.roles[role];
  if (flipped === "local" || flipped === "cloud") return flipped;
  return SHIPPED_TAG[role] ?? null;
}

export function liveCap(ceiling, ticks, loaded) {
  if (!Number.isInteger(ceiling) || ceiling < 0) return null;
  const up = new Set(loaded);
  const missing = ticks.filter((model) => !up.has(model));
  if (missing.length === 0) return ceiling;
  if (missing.length === ticks.length) return 0;
  return Math.max(0, ceiling - missing.length);
}

export function missingHeadings(task) {
  const text = typeof task === "string" ? task : "";
  return ORDER_HEADINGS.filter((heading) => !text.includes(heading));
}

function listed(models) {
  return models.length ? models.join(", ") : "";
}

/**
 * @param {object} input
 * @param {ReturnType<typeof parseState>} input.state
 * @param {string} input.role
 * @param {string | null | undefined} input.model
 * @param {"local" | "cloud" | null} input.modelTag where the named model is served
 * @param {string[]} input.loaded models the servers report as loaded
 * @param {string[]} input.busy plugin in-flight model ids
 * @param {string[]} input.cloudModels cloud models pi already lists
 * @param {number} input.runningLocal in-flight seats that count in Split
 * @param {number} input.runningAll in-flight crew seats
 * @param {string} [input.task]
 */
export function decideSpawn(input) {
  const state = input.state ?? offState();
  if (state.switch === "Off") return { action: "allow" };

  if (!isKnownRole(input.role)) {
    return { action: "block", reason: `Refused: ${input.role || "that seat"} is not a crew role. I will not guess a tag.` };
  }

  if (state.ceiling === null) {
    return { action: "block", reason: "Refused: no ceiling is set. Run /shipmates-crew. I will not invent a number." };
  }

  const loaded = new Set(input.loaded ?? []);
  const busy = new Set(input.busy ?? []);
  const ticks = state.ticks;
  const dropped = ticks.filter((model) => !loaded.has(model));
  const cap = liveCap(state.ceiling, ticks, input.loaded ?? []);
  const freeLocal = ticks.filter((model) => loaded.has(model) && !busy.has(model));
  const freeCloud = (input.cloudModels ?? []).filter((model) => !busy.has(model));

  const dropNote = dropped.length
    ? `${dropped.join(", ")} is no longer loaded, so it is skipped, not replaced. Live cap is now ${cap}. `
    : "";

  if (state.switch === "Local") {
    if (input.modelTag !== "local" || !ticks.includes(input.model) || !loaded.has(input.model)) {
      return {
        action: "block",
        reason: `${dropNote}Refused: ${input.role} must be a ticked loaded local model. Free models: ${listed(freeLocal) || "none"}. I will not pick one.`,
      };
    }
    if (busy.has(input.model)) {
      return { action: "block", reason: `Waiting: ${input.model} is busy. This seat stays on ${input.model}. Not moved. Run it again when that model is free. I will not substitute a model.` };
    }
    if (input.runningAll >= cap) {
      return { action: "block", reason: `Waiting: live cap is ${cap}, ${input.runningAll} already running. Not started. Run this seat again when one returns. I will not send it to the cloud or to an unticked model.` };
    }
    return finish(input, dropNote, cap, input.runningAll);
  }

  const tag = effectiveTag(state, input.role);
  if (!input.model || input.modelTag !== tag) {
    const free = tag === "local" ? freeLocal : freeCloud;
    return {
      action: "block",
      reason: `${dropNote}Refused: ${input.role} is tagged ${tag}. ${input.model || "No model"} is the wrong tag. Free ${tag} models: ${listed(free) || "none"}. Name one and retry. I will not substitute a model.`,
    };
  }
  if (tag === "local" && !loaded.has(input.model)) {
    return { action: "block", reason: `${dropNote}Refused: ${input.model} is not loaded. It is skipped, not replaced. Free local models: ${listed(freeLocal) || "none"}.` };
  }
  if (tag === "local" && busy.has(input.model)) {
    return { action: "block", reason: `Waiting: ${input.model} is busy. This seat stays on ${input.model}. Not moved. Run it again when that model is free. I will not substitute a model.` };
  }
  const running = tag === "local" ? input.runningLocal : 0;
  if (tag === "local" && running >= cap) {
    return { action: "block", reason: `Waiting: live cap is ${cap}, ${input.runningLocal} already running. Not started. Run this seat again when one returns. I will not send it to the cloud or to an unticked model.` };
  }
  return finish(input, dropNote, cap, running);
}

function finish(input, dropNote, cap, running) {
  const slots = Math.max(0, cap - running);
  const mode = input.state.switch;
  const line = mode === "Local"
    ? `${dropNote}Local. Live cap ${cap}. Slots free ${slots}. Crew seats only. You do not count. Honoured: ${input.role} stays on ${input.model}.`
    : `${dropNote}Split. Live cap ${cap}. Slots free ${slots}. Local seats only. Honoured: ${input.role} stays on ${input.model}.`;
  if (input.state.switch !== "Off" && isDoer(input.role)) {
    const missing = missingHeadings(input.task);
    if (missing.length) {
      return {
        action: "block",
        reason: `Refused: this doer order is missing ${missing.join(", ")}. Rewrite it so it contains Outcome:, Owned paths:, Done-when:, and Out of scope:. Then retry. I will not ask the model to fill the gap.`,
      };
    }
    return { action: "allow", honoured: true, status: line, prepend: ORDER_PREAMBLE };
  }
  return { action: "allow", honoured: true, status: line };
}
