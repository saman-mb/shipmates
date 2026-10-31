#!/usr/bin/env node
// Writes the package.json for the pi-shipmates npm package.
//
// The runtime dependency pins are read from the broker's own manifest
// (src/adapters/pi_crew/package.json) so the package can never disagree with the
// extension it ships. Keeping the pins in one place is the whole point: a bump in
// the adapter shows up here on the next build with nothing to remember.
//
// Usage: write-manifest.js <outPath> <pkgName> <version> <pinsPath>
import { readFileSync, writeFileSync } from "node:fs";

const [outPath, pkgName, version, pinsPath] = process.argv.slice(2);
for (const [name, value] of Object.entries({ outPath, pkgName, version, pinsPath })) {
  if (!value) {
    console.error(`::error::missing argument: ${name}`);
    process.exit(1);
  }
}

// The broker self-provisions these today; declared here, npm installs them once
// with the package and the extension finds them without writing inside
// node_modules — where the next `pi update --extensions` would prune them.
const pins = JSON.parse(readFileSync(pinsPath, "utf8")).dependencies;

const manifest = {
  name: pkgName,
  version,
  description:
    "Shipmates for pi: the full specialist crew, every command and the toolbox, installed as one pi package.",
  keywords: [
    "pi-package",
    "pi",
    "pi-coding-agent",
    "subagents",
    "agents",
    "commands",
    "crew",
  ],
  license: "MIT",
  author: "Shipmates contributors",
  repository: { type: "git", url: "git+https://github.com/saman-mb/shipmates.git" },
  homepage: "https://saman-mb.github.io/shipmates/",
  bugs: { url: "https://github.com/saman-mb/shipmates/issues" },
  // pi core loads extensions, skills, prompts and themes. The crew is discovered by
  // the pi-subagents extension through its documented package convention,
  // pi.subagents.agents — pi core ignores that key rather than rejecting it.
  // Contributor steering is deliberately absent: it is documentation, and a
  // prompts entry would turn it into a slash command.
  pi: {
    extensions: ["./.pi/extensions/shipmates-crew/index.ts"],
    skills: ["./.pi/skills"],
    subagents: { agents: ["./.pi/agents"] },
  },
  dependencies: pins,
  files: [".pi", "docs", "README.md", "LICENSE"],
};

writeFileSync(outPath, `${JSON.stringify(manifest, null, 2)}\n`);
console.log(`wrote ${outPath} for ${pkgName}@${version}`);
