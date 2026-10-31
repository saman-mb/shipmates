#!/usr/bin/env node
// Assert that the tarball npm would actually publish is a deliverable and carries
// no machine state. Reads `npm pack --dry-run --json` on stdin.
//
// This checks the artifact rather than our intentions: the package's `files`
// allowlist is what keeps the render scratch dir and the install receipt out of
// the tarball, and an allowlist is exactly the kind of rule that quietly stops
// matching when someone adds a path.
import { readFileSync } from "node:fs";

const items = JSON.parse(readFileSync(0, "utf8"));
const files = items.flatMap((item) => (item.files || []).map((f) => f.path));
const failed = [];

if (files.length === 0) failed.push("npm reported no files");

const machineState = /(^|\/)(\.built|\.shipmates|receipts|node_modules|__pycache__|\.venv)(\/|$)/;
const leaked = files.filter((p) => machineState.test(p));
if (leaked.length) failed.push(`machine-state paths in tarball: ${leaked.join(", ")}`);

for (const required of [
  "package.json",
  "README.md",
  "LICENSE",
  ".pi/extensions/shipmates-crew/index.ts",
  ".pi/agents/architect.md",
  "docs/contributor-steering.md",
]) {
  if (!files.includes(required)) failed.push(`missing ${required}`);
}

if (failed.length) {
  console.error(`::error::${failed.join(" — ")}`);
  process.exit(1);
}

console.log(`tarball verified: ${files.length} files, no machine-state paths`);
