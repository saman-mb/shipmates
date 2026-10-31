import assert from "node:assert/strict";
import test from "node:test";
import { PassThrough } from "node:stream";
import {
  createFrameParser,
  createLinkedPair,
  createMessageConnection,
  createRpcClient,
  encodeMessage,
} from "./protocol.mjs";

test("encodeMessage round-trips through the frame parser", () => {
  const seen = [];
  const errors = [];
  const parser = createFrameParser((m) => seen.push(m), (e) => errors.push(e));
  const message = { jsonrpc: "2.0", id: 1, method: "ping", params: { a: 1 } };
  parser.push(encodeMessage(message));
  assert.deepEqual(seen, [message]);
  assert.equal(errors.length, 0);
});

test("frame parser reassembles a header and body split across pushes", () => {
  const seen = [];
  const parser = createFrameParser((m) => seen.push(m), () => {});
  const frame = encodeMessage({ jsonrpc: "2.0", id: 7, result: "ok" });
  parser.push(frame.subarray(0, 8));
  assert.equal(seen.length, 0);
  parser.push(frame.subarray(8, 20));
  assert.equal(seen.length, 0);
  parser.push(frame.subarray(20));
  assert.equal(seen.length, 1);
  assert.equal(seen[0].id, 7);
  assert.equal(seen[0].result, "ok");
});

test("two frames in one chunk both deliver", () => {
  const seen = [];
  const parser = createFrameParser((m) => seen.push(m), () => {});
  const a = encodeMessage({ jsonrpc: "2.0", id: 1, result: "a" });
  const b = encodeMessage({ jsonrpc: "2.0", id: 2, result: "b" });
  parser.push(Buffer.concat([a, b]));
  assert.equal(seen.length, 2);
  assert.equal(seen[0].id, 1);
  assert.equal(seen[1].id, 2);
});

test("missing Content-Length fails the parser", () => {
  const errors = [];
  const parser = createFrameParser(() => {}, (e) => errors.push(e));
  parser.push(Buffer.from("Not-A-Header: 1\r\n\r\n{}\n", "ascii"));
  assert.equal(errors.length, 1);
  assert.match(String(errors[0].message || errors[0]), /Content-Length|header/i);
});

test("non-JSON body emits one -32700 Parse error then closes the connection", async () => {
  const readable = new PassThrough();
  const writable = new PassThrough();
  const out = [];
  writable.on("data", (c) => out.push(Buffer.from(c)));
  let closed = false;
  const conn = createMessageConnection(readable, writable);
  conn.onClose(() => {
    closed = true;
  });
  // Valid framing, invalid JSON body.
  const body = Buffer.from("{not-json", "utf8");
  const header = Buffer.from(`Content-Length: ${body.length}\r\n\r\n`, "ascii");
  readable.push(Buffer.concat([header, body]));
  // Allow the parser to flush.
  await new Promise((r) => setTimeout(r, 10));
  assert.equal(closed, true);
  const joined = Buffer.concat(out).toString("utf8");
  assert.match(joined, /-32700/);
  assert.match(joined, /Parse error/);
});

test("createRpcClient correlates result by id", async () => {
  const { client, server } = createLinkedPair();
  server.onMessage((message) => {
    if (message.method === "echo") {
      server.send({ jsonrpc: "2.0", id: message.id, result: message.params });
    }
  });
  const rpc = createRpcClient(client);
  const result = await rpc.request("echo", { hello: "world" });
  assert.deepEqual(result, { hello: "world" });
  client.close();
  server.close();
});

test("createRpcClient rejects on error response", async () => {
  const { client, server } = createLinkedPair();
  server.onMessage((message) => {
    server.send({
      jsonrpc: "2.0",
      id: message.id,
      error: { code: -32601, message: "Method not found" },
    });
  });
  const rpc = createRpcClient(client);
  await assert.rejects(() => rpc.request("missing", {}), /Method not found/);
  client.close();
  server.close();
});
