// Single-process admission broker. Capacity rules live in capacity.mjs. This module does not open TCP.

import net from "node:net";
import os from "node:os";
import path from "node:path";
import { unlinkSync } from "node:fs";
import { applyGrant, planAdmission, reconcileLedger, releaseReservation } from "./capacity.mjs";
import { createMessageConnection } from "./protocol.mjs";

const DEFAULT_LEASE_MS = 30_000;
const DEFAULT_NAME = "shipmates-admission";
const JOURNAL_FIELDS = [
  "type",
  "atMs",
  "runId",
  "modelId",
  "generation",
  "pendingBytes",
  "contextTokens",
  "outputReserveTokens",
  "slots",
  "queuePosition",
  "enqueuedAtMs",
  "grantedAtMs",
  "expiresAtMs",
  "releasedAtMs",
];

export function ipcListenPath(platform = process.platform, name = DEFAULT_NAME, tmpdir = os.tmpdir()) {
  const safe = typeof name === "string" && /^[a-z0-9-]+$/.test(name) ? name : DEFAULT_NAME;
  if (platform === "win32") return `\\\\.\\pipe\\${safe}`;
  return path.join(tmpdir, `${safe}.sock`);
}

function assertIpcPath(listenPath) {
  if (typeof listenPath !== "string" || listenPath.length === 0 || listenPath.includes("\0")) {
    throw new Error("broker listens on an IPC path, not a TCP port");
  }
  if (/^\d+$/.test(listenPath) || /^[a-z]+:\/\//i.test(listenPath)) {
    throw new Error("broker listens on an IPC path, not a TCP port");
  }
  if (/:\d+$/.test(listenPath) && !listenPath.startsWith("\\\\.\\pipe\\")) {
    throw new Error("broker listens on an IPC path, not a TCP port");
  }
}

function rpcError(code, message, data) {
  const error = new Error(message);
  error.code = code;
  error.data = data;
  return error;
}

function finiteNumber(value) {
  return typeof value === "number" && Number.isFinite(value) ? value : undefined;
}

function journalRecord(type, fields) {
  const record = { type, atMs: fields.atMs };
  for (const key of JOURNAL_FIELDS) {
    if (key === "type" || key === "atMs") continue;
    if (fields[key] !== undefined) record[key] = fields[key];
  }
  return record;
}

function remember(journal, record) {
  if (!journal || typeof journal.append !== "function") return;
  journal.append(record);
}

function adoptLedger(ledger) {
  if (!ledger || !Array.isArray(ledger.reservations)) {
    throw rpcError(-32603, "capacity returned a ledger without reservations");
  }
  return ledger;
}

function admissionRequest(params, waiting, now) {
  if (!params || typeof params !== "object" || Array.isArray(params)) {
    throw rpcError(-32602, "admission params must be an object");
  }
  if (typeof params.runId !== "string" || params.runId.length === 0) {
    throw rpcError(-32602, "runId is required");
  }
  return {
    runId: params.runId,
    sessionId: params.sessionId,
    role: params.role,
    model: params.model,
    pinned: params.pinned,
    contextTokens: params.contextTokens,
    outputReserveTokens: params.outputReserveTokens,
    priority: params.priority,
    enqueuedAtMs: finiteNumber(params.enqueuedAtMs) ?? now,
    waiting,
  };
}

function resourceFields(plan, request) {
  const reservation = plan && plan.reservation && typeof plan.reservation === "object" ? plan.reservation : {};
  return {
    runId: request.runId,
    modelId: typeof reservation.modelId === "string" ? reservation.modelId : request.model,
    pendingBytes: finiteNumber(reservation.pendingBytes),
    contextTokens: finiteNumber(reservation.contextTokens) ?? finiteNumber(request.contextTokens),
    outputReserveTokens: finiteNumber(request.outputReserveTokens),
    slots: finiteNumber(reservation.slots),
    queuePosition: finiteNumber(plan && plan.queuePosition),
    enqueuedAtMs: finiteNumber(request.enqueuedAtMs),
  };
}

function grantResult(lease, plan) {
  return {
    action: "grant",
    code: plan && plan.code ? plan.code : "grant",
    reason: plan && typeof plan.reason === "string" ? plan.reason : "",
    queuePosition: null,
    generation: lease.generation,
    reservation: plan ? plan.reservation ?? null : null,
    lease: {
      runId: lease.runId,
      modelId: lease.modelId,
      generation: lease.generation,
      grantedAtMs: lease.grantedAtMs,
      expiresAtMs: lease.expiresAtMs,
    },
  };
}

function requireGeneration(params) {
  if (!params || typeof params !== "object" || typeof params.runId !== "string" || params.runId.length === 0) {
    throw rpcError(-32602, "runId is required");
  }
  if (!Number.isInteger(params.generation)) {
    throw rpcError(-32602, "generation is required");
  }
  return params;
}

function createRuntime(options) {
  if (!options || typeof options.probe !== "function") throw new Error("probe is required");
  const now = typeof options.now === "function" ? options.now : Date.now;
  let tail = Promise.resolve();
  const runtime = {
    probe: options.probe,
    journal: options.journal ?? null,
    now,
    leaseMs: finiteNumber(options.leaseMs) ?? DEFAULT_LEASE_MS,
    ledger: { reservations: [] },
    leases: new Map(),
    queue: [],
    spentGenerations: new Set(),
    nextGeneration: 1,
    stopped: false,
    exclusive(fn) {
      const run = tail.then(() => fn());
      tail = run.then(
        () => {},
        () => {},
      );
      return run;
    },
  };
  return runtime;
}

async function snapshotOf(runtime) {
  const snapshot = await runtime.probe();
  if (!snapshot || typeof snapshot !== "object") throw rpcError(-32603, "probe returned no snapshot");
  return snapshot;
}

function issueGrant(runtime, request, plan) {
  runtime.ledger = adoptLedger(applyGrant(runtime.ledger, plan));
  const grantedAtMs = runtime.now();
  const generation = runtime.nextGeneration++;
  const lease = {
    runId: request.runId,
    modelId: resourceFields(plan, request).modelId,
    generation,
    grantedAtMs,
    expiresAtMs: grantedAtMs + runtime.leaseMs,
    lastHeartbeatMs: grantedAtMs,
  };
  runtime.leases.set(request.runId, lease);
  const resources = resourceFields(plan, request);
  remember(
    runtime.journal,
    journalRecord("grant", {
      ...resources,
      atMs: grantedAtMs,
      generation,
      grantedAtMs,
      expiresAtMs: lease.expiresAtMs,
    }),
  );
  return grantResult(lease, plan);
}

function spendLease(runtime, lease, type) {
  runtime.ledger = adoptLedger(releaseReservation(runtime.ledger, lease.runId));
  runtime.spentGenerations.add(lease.generation);
  runtime.leases.delete(lease.runId);
  const atMs = runtime.now();
  remember(
    runtime.journal,
    journalRecord(type, {
      atMs,
      runId: lease.runId,
      modelId: lease.modelId,
      generation: lease.generation,
      releasedAtMs: atMs,
    }),
  );
}

function fence(runtime, params) {
  const { runId, generation } = requireGeneration(params);
  if (runtime.spentGenerations.has(generation)) {
    throw rpcError(-32001, "stale generation");
  }
  const lease = runtime.leases.get(runId);
  if (!lease) throw rpcError(-32002, "unknown run");
  if (lease.generation !== generation) throw rpcError(-32001, "stale generation");
  return lease;
}

async function sweep(runtime, deliveries) {
  const now = runtime.now();
  const expired = [...runtime.leases.values()].filter((lease) => now >= lease.expiresAtMs);
  if (expired.length === 0) return;
  for (const lease of expired) spendLease(runtime, lease, "expire");
  await drain(runtime, deliveries);
}

async function drain(runtime, deliveries) {
  while (runtime.queue.length > 0) {
    const snapshot = await snapshotOf(runtime);
    runtime.ledger = adoptLedger(reconcileLedger(runtime.ledger, snapshot));
    const head = runtime.queue[0];
    const waiting = runtime.queue.slice(1).map((entry) => entry.request);
    const request = { ...head.request, waiting };
    const plan = planAdmission(request, snapshot, runtime.ledger);
    if (!plan || plan.code === "refused-invalid") {
      runtime.queue.shift();
      const error = rpcError(-32602, plan && plan.reason ? plan.reason : "invalid admission request", {
        code: "refused-invalid",
      });
      deliveries.push(() => head.parked.reject(error));
      continue;
    }
    if (plan.action !== "grant") return;
    runtime.queue.shift();
    const result = issueGrant(runtime, request, plan);
    deliveries.push(() => head.deliver(result));
  }
}

function compareQueue(left, right) {
  const a = left.request;
  const b = right.request;
  if (a.priority !== b.priority) return b.priority - a.priority;
  if (a.enqueuedAtMs !== b.enqueuedAtMs) return a.enqueuedAtMs - b.enqueuedAtMs;
  return String(a.runId).localeCompare(String(b.runId));
}

function enqueue(runtime, request, connection, respond) {
  const parked = {};
  parked.promise = new Promise((resolve, reject) => {
    parked.resolve = resolve;
    parked.reject = reject;
  });
  parked.promise.catch(() => {});
  const entry = {
    request,
    connection,
    respond,
    parked,
    deliver(result) {
      if (respond) {
        parked.resolve(result);
        return;
      }
      connection.send({ jsonrpc: "2.0", method: "admission/granted", params: result });
      parked.resolve(result);
    },
  };
  runtime.queue.push(entry);
  runtime.queue.sort(compareQueue);
  return entry;
}

async function admit(runtime, connection, params, deliveries) {
  const now = runtime.now();
  const request = admissionRequest(params, [], now);
  const existing = runtime.leases.get(request.runId);
  if (existing && runtime.now() < existing.expiresAtMs) {
    return grantResult(existing, { code: "grant", reason: "grant already issued", reservation: null });
  }
  if (runtime.queue.some((entry) => entry.request.runId === request.runId)) {
    throw rpcError(-32602, "run already queued");
  }
  const snapshot = await snapshotOf(runtime);
  runtime.ledger = adoptLedger(reconcileLedger(runtime.ledger, snapshot));
  const waiting = runtime.queue.map((entry) => entry.request);
  const planned = { ...request, waiting };
  const plan = planAdmission(planned, snapshot, runtime.ledger);
  if (!plan || plan.code === "refused-invalid") {
    throw rpcError(-32602, plan && plan.reason ? plan.reason : "invalid admission request", {
      code: "refused-invalid",
    });
  }
  if (plan.action === "grant") return issueGrant(runtime, planned, plan);
  if (plan.action !== "queue") throw rpcError(-32603, "capacity returned an unknown action");

  const queued = { ...request, waiting: [] };
  const wait = params.wait !== false;
  const entry = enqueue(runtime, queued, connection, wait);
  const resources = resourceFields(plan, queued);
  remember(
    runtime.journal,
    journalRecord("queue", {
      ...resources,
      atMs: now,
      queuePosition: finiteNumber(plan.queuePosition),
    }),
  );
  if (!wait) {
    return {
      action: "queue",
      code: plan.code,
      reason: typeof plan.reason === "string" ? plan.reason : "",
      queuePosition: plan.queuePosition ?? null,
      generation: null,
    };
  }
  return { deferred: true, promise: entry.parked.promise };
}

async function release(runtime, params, deliveries) {
  const lease = fence(runtime, params);
  spendLease(runtime, lease, "release");
  await drain(runtime, deliveries);
  return { released: true, runId: lease.runId, generation: lease.generation };
}

async function cancel(runtime, params, deliveries) {
  if (!params || typeof params.runId !== "string" || params.runId.length === 0) {
    throw rpcError(-32602, "runId is required");
  }
  const queuedIndex = runtime.queue.findIndex((entry) => entry.request.runId === params.runId);
  if (queuedIndex !== -1) {
    const [entry] = runtime.queue.splice(queuedIndex, 1);
    const atMs = runtime.now();
    remember(
      runtime.journal,
      journalRecord("cancel", { atMs, runId: params.runId, modelId: entry.request.model, releasedAtMs: atMs }),
    );
    deliveries.push(() => entry.parked.reject(rpcError(-32004, "cancelled")));
    return { cancelled: true, runId: params.runId, queued: true };
  }
  const lease = fence(runtime, params);
  spendLease(runtime, lease, "cancel");
  await drain(runtime, deliveries);
  return { cancelled: true, runId: lease.runId, queued: false };
}

async function heartbeat(runtime, params) {
  const lease = fence(runtime, params);
  const atMs = runtime.now();
  if (atMs >= lease.expiresAtMs) {
    throw rpcError(-32003, "lease expired");
  }
  lease.lastHeartbeatMs = atMs;
  lease.expiresAtMs = atMs + runtime.leaseMs;
  remember(
    runtime.journal,
    journalRecord("heartbeat", {
      atMs,
      runId: lease.runId,
      modelId: lease.modelId,
      generation: lease.generation,
      expiresAtMs: lease.expiresAtMs,
    }),
  );
  return { runId: lease.runId, generation: lease.generation, expiresAtMs: lease.expiresAtMs };
}

function status(runtime) {
  return {
    queueLength: runtime.queue.length,
    reservations: runtime.ledger.reservations.map((reservation) => ({
      runId: reservation.runId,
      modelId: reservation.modelId,
      pendingBytes: reservation.pendingBytes,
      contextTokens: reservation.contextTokens,
      slots: reservation.slots,
      visible: reservation.visible,
    })),
    leases: [...runtime.leases.values()].map((lease) => ({
      runId: lease.runId,
      modelId: lease.modelId,
      generation: lease.generation,
      grantedAtMs: lease.grantedAtMs,
      expiresAtMs: lease.expiresAtMs,
    })),
    queue: runtime.queue.map((entry, index) => ({
      runId: entry.request.runId,
      modelId: entry.request.model,
      priority: entry.request.priority,
      enqueuedAtMs: entry.request.enqueuedAtMs,
      queuePosition: index + 1,
    })),
  };
}

function dropConnection(runtime, connection, deliveries) {
  const retained = [];
  for (const entry of runtime.queue) {
    if (entry.connection !== connection) {
      retained.push(entry);
      continue;
    }
    deliveries.push(() => entry.parked.reject(rpcError(-32004, "connection closed")));
  }
  runtime.queue = retained;
}

export function createBroker(options = {}) {
  const runtime = createRuntime(options);

  function finish(deliveries) {
    for (const deliver of deliveries) deliver();
  }

  async function dispatch(connection, method, params) {
    const deliveries = [];
    try {
      return await runtime.exclusive(async () => {
        if (runtime.stopped) throw rpcError(-32603, "broker stopped");
        await sweep(runtime, deliveries);
        if (method === "admit") return admit(runtime, connection, params ?? {}, deliveries);
        if (method === "release") return release(runtime, params ?? {}, deliveries);
        if (method === "cancel") return cancel(runtime, params ?? {}, deliveries);
        if (method === "heartbeat") return heartbeat(runtime, params ?? {});
        if (method === "status") return status(runtime);
        throw rpcError(-32601, "method not found");
      });
    } finally {
      finish(deliveries);
    }
  }

  function attach(connection) {
    if (!connection || typeof connection.onMessage !== "function" || typeof connection.send !== "function") {
      throw new Error("connection must send and receive framed messages");
    }
    connection.onMessage((message) => {
      if (!message || message.jsonrpc !== "2.0" || typeof message.method !== "string") {
        if (message && message.id != null) {
          connection.send({
            jsonrpc: "2.0",
            id: message.id,
            error: { code: -32600, message: "Invalid Request" },
          });
        }
        return;
      }
      if (message.id == null) return;
      dispatch(connection, message.method, message.params)
        .then((result) => {
          if (result && result.deferred) {
            result.promise.then(
              (value) => connection.send({ jsonrpc: "2.0", id: message.id, result: value }),
              (error) =>
                connection.send({
                  jsonrpc: "2.0",
                  id: message.id,
                  error: { code: error.code ?? -32603, message: error.message },
                }),
            );
            return;
          }
          connection.send({ jsonrpc: "2.0", id: message.id, result });
        })
        .catch((error) => {
          connection.send({
            jsonrpc: "2.0",
            id: message.id,
            error: { code: error.code ?? -32603, message: error.message },
          });
        });
    });
    if (typeof connection.onClose === "function") {
      connection.onClose(() => {
        const deliveries = [];
        runtime
          .exclusive(async () => {
            dropConnection(runtime, connection, deliveries);
          })
          .catch(() => {})
          .finally(() => finish(deliveries));
      });
    }
  }

  if (options.connection) attach(options.connection);
  return {
    attach,
    close() {
      runtime.stopped = true;
    },
  };
}

export async function startBroker(options = {}) {
  const listenPath = options.path ?? ipcListenPath(options.platform, options.name, options.tmpdir);
  assertIpcPath(listenPath);
  const broker = createBroker(options);
  const server = net.createServer((socket) => {
    socket.on("error", () => {});
    broker.attach(createMessageConnection(socket, socket));
  });
  server.on("error", () => {});
  if (process.platform !== "win32") {
    try {
      unlinkSync(listenPath);
    } catch {
      // No stale socket.
    }
  }
  return new Promise((resolve, reject) => {
    const fail = (error) => reject(error);
    server.once("error", fail);
    server.listen(listenPath, () => {
      server.off("error", fail);
      resolve({
        path: listenPath,
        address() {
          return server.address();
        },
        close() {
          broker.close();
          return new Promise((done, deny) => {
            server.close((error) => {
              if (process.platform !== "win32") {
                try {
                  unlinkSync(listenPath);
                } catch {
                  // Already removed.
                }
              }
              if (error) deny(error);
              else done();
            });
          });
        },
      });
    });
  });
}
