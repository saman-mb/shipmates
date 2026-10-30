// JSON-RPC 2.0 over Content-Length framing, the same framing vscode-jsonrpc uses.

import { PassThrough } from "node:stream";

const MAX_HEADER_BYTES = 64 * 1024;
const MAX_BODY_BYTES = 1024 * 1024;
const HEADER_SEPARATOR = Buffer.from("\r\n\r\n");

export function encodeMessage(message) {
  const body = Buffer.from(JSON.stringify(message), "utf8");
  const header = Buffer.from(`Content-Length: ${body.length}\r\n\r\n`, "ascii");
  return Buffer.concat([header, body]);
}

export function createFrameParser(onMessage, onError) {
  let buffer = Buffer.alloc(0);
  let failed = false;

  function fail(error) {
    if (failed) return;
    failed = true;
    buffer = Buffer.alloc(0);
    onError(error);
  }

  return {
    push(chunk) {
      if (failed) return;
      const next = Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk);
      buffer = Buffer.concat([buffer, next]);
      while (!failed) {
        const separator = buffer.indexOf(HEADER_SEPARATOR);
        if (separator === -1) {
          if (buffer.length > MAX_HEADER_BYTES) fail(new Error("header exceeds limit"));
          return;
        }
        const header = buffer.subarray(0, separator).toString("ascii");
        const match = /(?:^|\r\n)content-length:\s*(\d+)\s*(?:\r\n|$)/i.exec(header);
        if (!match) {
          fail(new Error("missing Content-Length"));
          return;
        }
        const length = Number(match[1]);
        if (!Number.isSafeInteger(length) || length > MAX_BODY_BYTES) {
          fail(new Error("invalid Content-Length"));
          return;
        }
        const start = separator + HEADER_SEPARATOR.length;
        if (buffer.length < start + length) return;
        const body = buffer.subarray(start, start + length).toString("utf8");
        buffer = Buffer.from(buffer.subarray(start + length));
        try {
          onMessage(JSON.parse(body));
        } catch (error) {
          fail(error);
          return;
        }
      }
    },
  };
}

export function createMessageConnection(readable, writable) {
  const messageHandlers = new Set();
  const closeHandlers = new Set();
  let closed = false;

  function markClosed() {
    if (closed) return;
    closed = true;
    for (const handler of closeHandlers) handler();
  }

  const parser = createFrameParser(
    (message) => {
      for (const handler of messageHandlers) handler(message);
    },
    () => {
      if (closed) return;
      try {
        writable.write(
          encodeMessage({
            jsonrpc: "2.0",
            id: null,
            error: { code: -32700, message: "Parse error" },
          }),
        );
      } catch {
        // The peer is already gone.
      }
      markClosed();
    },
  );

  readable.on("data", (chunk) => parser.push(chunk));
  readable.on("end", markClosed);
  readable.on("close", markClosed);
  readable.on("error", markClosed);
  writable.on("error", markClosed);

  return {
    send(message) {
      if (closed) return;
      writable.write(encodeMessage(message));
    },
    onMessage(handler) {
      messageHandlers.add(handler);
      return () => messageHandlers.delete(handler);
    },
    onClose(handler) {
      closeHandlers.add(handler);
      if (closed) handler();
      return () => closeHandlers.delete(handler);
    },
    close() {
      markClosed();
      if (typeof writable.end === "function") writable.end();
    },
  };
}

export function createLinkedPair() {
  const clientToServer = new PassThrough();
  const serverToClient = new PassThrough();
  return {
    client: createMessageConnection(serverToClient, clientToServer),
    server: createMessageConnection(clientToServer, serverToClient),
  };
}

export function createRpcClient(connection) {
  let nextId = 0;
  const pending = new Map();
  connection.onMessage((message) => {
    if (!message || message.method !== undefined || message.id == null || !pending.has(message.id)) return;
    const slot = pending.get(message.id);
    pending.delete(message.id);
    if (message.error) {
      const error = new Error(message.error.message || "RPC error");
      error.code = message.error.code;
      error.data = message.error.data;
      slot.reject(error);
      return;
    }
    slot.resolve(message.result);
  });
  return {
    request(method, params) {
      const id = ++nextId;
      return new Promise((resolve, reject) => {
        pending.set(id, { resolve, reject });
        connection.send({ jsonrpc: "2.0", id, method, params });
      });
    },
  };
}
