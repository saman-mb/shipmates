import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import {
  existsSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
  mkdirSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { brokerEndpoint } from "./broker.mjs";
import {
  fileJournal,
  lockIsLive,
  takeLock,
  waitForBroker,
} from "./index.ts";

const here = dirname(fileURLToPath(import.meta.url));
const indexPath = join(here, "index.ts");

function tempDir(prefix = "shipmates-daemon-") {
  return mkdtempSync(join(tmpdir(), prefix));
}

test("brokerEndpoint is stable for the same agent dir and distinct across homes", () => {
  const a = brokerEndpoint("/tmp/agent-a", process.platform, "uid-1");
  const b = brokerEndpoint("/tmp/agent-a", process.platform, "uid-1");
  const c = brokerEndpoint("/tmp/agent-b", process.platform, "uid-1");
  assert.equal(a.home, b.home);
  assert.equal(a.listenPath, b.listenPath);
  assert.notEqual(a.home, c.home);
  assert.ok(a.home.endsWith("shipmates-resource-broker"));
});

test("takeLock is exclusive and reclaimable after a stale lock", () => {
  const dir = tempDir();
  const lock = join(dir, "broker.lock");
  assert.equal(takeLock(lock), true);
  assert.equal(takeLock(lock), false);
  // Stale: dead pid text.
  writeFileSync(lock, "999999");
  assert.equal(lockIsLive(lock), false);
  assert.equal(takeLock(lock), true);
  rmSync(dir, { recursive: true, force: true });
});

test("lockIsLive is false for non-numeric garbage", () => {
  const dir = tempDir();
  const lock = join(dir, "broker.lock");
  writeFileSync(lock, "not-a-pid");
  assert.equal(lockIsLive(lock), false);
  rmSync(dir, { recursive: true, force: true });
});

test("fileJournal appends one JSON line per record and tolerates an unwritable dir", () => {
  const dir = tempDir();
  const journal = fileJournal(dir);
  journal.append({ type: "test", atMs: 1 });
  journal.append({ type: "test", atMs: 2 });
  const text = readFileSync(join(dir, "journal.jsonl"), "utf8");
  const lines = text.trim().split("\n");
  assert.equal(lines.length, 2);
  assert.equal(JSON.parse(lines[0]).atMs, 1);
  assert.equal(JSON.parse(lines[1]).atMs, 2);
  // Unwritable journal file: replace the jsonl with a directory so append cannot write.
  // Admission must not throw — the journal is best-effort.
  const journalPath = join(dir, "journal.jsonl");
  rmSync(journalPath, { force: true });
  mkdirSync(journalPath);
  assert.doesNotThrow(() => journal.append({ type: "x" }));
  rmSync(dir, { recursive: true, force: true });
});

test("waitForBroker returns null when nothing listens", async () => {
  const dir = tempDir();
  const sock = join(dir, "missing.sock");
  const client = await waitForBroker(sock);
  assert.equal(client, null);
  rmSync(dir, { recursive: true, force: true });
});

test("startDaemon writes endpoint.json and a second start does not double-bind", async () => {
  if (process.platform === "win32") return;
  const agent = tempDir("shipmates-agent-");
  const endpoint = brokerEndpoint(agent, process.platform, "test-uid");
  mkdirSync(endpoint.home, { recursive: true });

  function spawnDaemonChild() {
    return spawn(
      process.execPath,
      ["--experimental-strip-types", indexPath, "--shipmates-broker-daemon"],
      {
        stdio: "ignore",
        env: {
          ...process.env,
          SHIPMATES_BROKER_HOME: endpoint.home,
          SHIPMATES_BROKER_PATH: endpoint.listenPath,
          SHIPMATES_CREW_NO_PROVISION: "1",
          // Keep NODE_TEST_CONTEXT so provisioning stays off in the child.
          NODE_TEST_CONTEXT: process.env.NODE_TEST_CONTEXT || "1",
        },
        detached: true,
      },
    );
  }

  const first = spawnDaemonChild();
  first.unref();

  // Wait for endpoint.json.
  let endpointFile = null;
  for (let i = 0; i < 50; i += 1) {
    const path = join(endpoint.home, "endpoint.json");
    if (existsSync(path)) {
      try {
        endpointFile = JSON.parse(readFileSync(path, "utf8"));
        if (endpointFile.pid) break;
      } catch {
        // retry
      }
    }
    await new Promise((r) => setTimeout(r, 100));
  }
  assert.ok(endpointFile, "first daemon must write endpoint.json");
  assert.equal(endpointFile.protocolVersion, 1);
  assert.equal(endpointFile.listenPath, endpoint.listenPath);
  assert.ok(Number.isInteger(endpointFile.pid) && endpointFile.pid > 1);

  const client = await waitForBroker(endpoint.listenPath);
  assert.ok(client, "waitForBroker must connect to the live daemon");

  // Second concurrent start must not steal the lock / rewrite pid.
  const second = spawnDaemonChild();
  await new Promise((r) => setTimeout(r, 500));
  const after = JSON.parse(readFileSync(join(endpoint.home, "endpoint.json"), "utf8"));
  assert.equal(after.pid, endpointFile.pid, "second start must not double-bind");
  try {
    second.kill("SIGTERM");
  } catch {
    // already exited
  }

  // Tear down first daemon.
  try {
    process.kill(endpointFile.pid, "SIGTERM");
  } catch {
    // already gone
  }
  // Wait for exit.
  for (let i = 0; i < 30; i += 1) {
    try {
      process.kill(endpointFile.pid, 0);
      await new Promise((r) => setTimeout(r, 100));
    } catch {
      break;
    }
  }
  rmSync(agent, { recursive: true, force: true });
}, { timeout: 15000 });
