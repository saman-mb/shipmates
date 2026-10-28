// Pure resource admission. No I/O, no model ids, no network.
//
// Callers pass contextTokens and outputReserveTokens already scaled into the
// same integer unit as pool availableBytes and residentBytes. This module does
// not invent a hardware bytes-per-token multiplier.
//
// Measured pool availableBytes already excludes loaded resident weights.
// Those weights are charged once per loaded model (shared by every holder)
// and are not subtracted again. A model that is not loaded adds residentBytes
// once across its holders, on top of each not-yet-visible context charge.
// Reconcile marks consumption visible so that context is not charged again.

const CONFIDENCE = new Set(["measured", "reported", "estimated", "unknown"]);
const PRESSURE = new Set(["ok", "high"]);

export function planAdmission(request, snapshot, ledger) {
  if (!validRequest(request) || !validSnapshot(snapshot) || !validLedger(ledger)) {
    return decision("queue", "refused-invalid", "Refused: admission request is invalid.", null, null);
  }
  if (ledger.reservations.some((row) => row.runId === request.runId)) {
    return decision("queue", "refused-invalid", "Refused: admission request is invalid.", null, null);
  }

  if (snapshot.pressure === "high") {
    return queued("queued-pressure", "Waiting: memory pressure is high. Not started. Existing grants stay.", request);
  }

  const model = snapshot.models.find((row) => row.id === request.model) ?? null;
  if (request.pinned && !model) {
    return queued("queued-pin", `Waiting: ${request.model} is pinned. Not substituted.`, request);
  }
  if (!model) {
    return queued("queued-capacity", `Waiting: ${request.model} does not fit current capacity.`, request);
  }

  const invisible = ledger.reservations.filter((row) => !row.visible).length;
  if (snapshot.confidence === "unknown" && invisible >= 1) {
    return queued(
      "queued-unknown",
      "Waiting: telemetry confidence is unknown. At most one local reservation can be not yet visible.",
      request,
    );
  }

  const contextCharge = request.contextTokens + request.outputReserveTokens;
  if (contextCharge > model.configuredContextTokens) {
    return cannotFit(request);
  }
  if (!slotsFit(model, ledger, request.model)) {
    return cannotFit(request);
  }

  const reservation = {
    runId: request.runId,
    modelId: request.model,
    pendingBytes: pendingFor(model, ledger, request.model, contextCharge),
    contextTokens: contextCharge,
    slots: 1,
    visible: false,
  };
  if (!bytesFit(snapshot, ledger, reservation)) {
    return cannotFit(request);
  }
  return decision("grant", "grant", `Granted: ${request.model} reserved for ${request.runId}.`, reservation, null);
}

export function applyGrant(ledger, plan) {
  const next = cloneLedger(ledger);
  if (!plan || plan.action !== "grant" || plan.code !== "grant" || !plan.reservation) return next;
  if (next.reservations.some((row) => row.runId === plan.reservation.runId)) return next;
  next.reservations.push({ ...plan.reservation });
  return next;
}

export function releaseReservation(ledger, runId) {
  const next = cloneLedger(ledger);
  next.reservations = next.reservations.filter((row) => row.runId !== runId);
  return next;
}

export function reconcileLedger(ledger, snapshot) {
  const next = cloneLedger(ledger);
  if (!validSnapshot(snapshot)) return next;
  const byModel = new Map();
  for (const row of next.reservations) {
    if (!byModel.has(row.modelId)) byModel.set(row.modelId, []);
    byModel.get(row.modelId).push(row);
  }
  for (const [modelId, rows] of byModel) {
    const model = snapshot.models.find((item) => item.id === modelId) ?? null;
    rows.sort((a, b) => String(a.runId).localeCompare(String(b.runId)));
    const visibleCount = model && model.loaded ? model.slotsInUse : 0;
    rows.forEach((row, index) => {
      const contextCharge = row.contextTokens;
      if (!model) return;
      if (model.loaded && index < visibleCount) {
        row.visible = true;
        row.pendingBytes = 0;
        return;
      }
      row.visible = false;
      const carrier = index === 0;
      row.pendingBytes = contextCharge + (model.loaded || !carrier ? 0 : model.residentBytes);
    });
  }
  return next;
}

function cannotFit(request) {
  if (request.pinned) {
    return queued("queued-pin", `Waiting: ${request.model} is pinned. Not substituted.`, request);
  }
  return queued("queued-capacity", `Waiting: ${request.model} does not fit current capacity.`, request);
}

function queued(code, reason, request) {
  return decision("queue", code, reason, null, queuePosition(request));
}

function decision(action, code, reason, reservation, queuePosition) {
  return { action, code, reason, reservation, queuePosition };
}

function queuePosition(request) {
  const waiting = request.waiting.filter((row) => row && row.runId !== request.runId);
  const rows = waiting.map((row) => ({
    runId: row.runId,
    priority: row.priority,
    enqueuedAtMs: row.enqueuedAtMs,
  }));
  rows.push({ runId: request.runId, priority: request.priority, enqueuedAtMs: request.enqueuedAtMs });
  rows.sort((a, b) => {
    if (a.priority !== b.priority) return b.priority - a.priority;
    if (a.enqueuedAtMs !== b.enqueuedAtMs) return a.enqueuedAtMs - b.enqueuedAtMs;
    return String(a.runId).localeCompare(String(b.runId));
  });
  return rows.findIndex((row) => row.runId === request.runId) + 1;
}

function slotsFit(model, ledger, modelId) {
  const pendingSlots = ledger.reservations
    .filter((row) => row.modelId === modelId && !row.visible)
    .reduce((sum, row) => sum + row.slots, 0);
  return model.slotsInUse + pendingSlots + 1 <= model.slots;
}

function pendingFor(model, ledger, modelId, contextCharge) {
  if (model.loaded) return contextCharge;
  const held = ledger.reservations.some((row) => row.modelId === modelId);
  return contextCharge + (held ? 0 : model.residentBytes);
}

function bytesFit(snapshot, ledger, candidate) {
  const pending = notVisibleBytes(snapshot, ledger.reservations) + candidate.pendingBytes;
  return snapshot.pools.some((pool) => pool.availableBytes - snapshot.headroomBytes - pending >= 0);
}

function notVisibleBytes(snapshot, reservations) {
  let bytes = 0;
  const seen = new Set();
  for (const row of reservations) {
    if (!row.visible) bytes += row.contextTokens;
    if (seen.has(row.modelId)) continue;
    seen.add(row.modelId);
    const model = snapshot.models.find((item) => item.id === row.modelId);
    if (!model || model.loaded) continue;
    bytes += model.residentBytes;
  }
  return bytes;
}

function cloneLedger(ledger) {
  const reservations = Array.isArray(ledger?.reservations) ? ledger.reservations.map((row) => ({ ...row })) : [];
  return { reservations };
}

function validRequest(request) {
  if (!request || typeof request !== "object") return false;
  if (!nonEmpty(request.runId) || !nonEmpty(request.sessionId) || !nonEmpty(request.role)) return false;
  if (!nonEmpty(request.model) || typeof request.pinned !== "boolean") return false;
  if (!nonNegative(request.contextTokens) || !nonNegative(request.outputReserveTokens)) return false;
  if (!Number.isInteger(request.priority) || !Number.isInteger(request.enqueuedAtMs)) return false;
  if (!Array.isArray(request.waiting)) return false;
  return request.waiting.every((row) => row
    && nonEmpty(row.runId)
    && Number.isInteger(row.priority)
    && Number.isInteger(row.enqueuedAtMs));
}

function validSnapshot(snapshot) {
  if (!snapshot || typeof snapshot !== "object") return false;
  if (!Number.isInteger(snapshot.observedAtMs) || snapshot.observedAtMs < 0) return false;
  if (!CONFIDENCE.has(snapshot.confidence) || !PRESSURE.has(snapshot.pressure)) return false;
  if (!nonNegative(snapshot.headroomBytes)) return false;
  if (!Array.isArray(snapshot.pools) || !Array.isArray(snapshot.models)) return false;
  if (!snapshot.pools.every((pool) => pool && nonEmpty(pool.id) && nonNegative(pool.availableBytes))) return false;
  return snapshot.models.every((model) => model
    && nonEmpty(model.id)
    && nonNegative(model.residentBytes)
    && nonNegative(model.configuredContextTokens)
    && nonNegative(model.slots)
    && nonNegative(model.slotsInUse)
    && typeof model.loaded === "boolean");
}

function validLedger(ledger) {
  if (!ledger || !Array.isArray(ledger.reservations)) return false;
  const ids = new Set();
  for (const row of ledger.reservations) {
    if (!row || !nonEmpty(row.runId) || !nonEmpty(row.modelId)) return false;
    if (!nonNegative(row.pendingBytes) || !nonNegative(row.contextTokens) || !nonNegative(row.slots)) return false;
    if (typeof row.visible !== "boolean" || ids.has(row.runId)) return false;
    ids.add(row.runId);
  }
  return true;
}

function nonEmpty(value) {
  return typeof value === "string" && value.length > 0;
}

function nonNegative(value) {
  return Number.isInteger(value) && value >= 0;
}
