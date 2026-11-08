#!/usr/bin/env node
// Writes the package.json for the pi-shipmates npm package.
//
// Usage: write-manifest.js <outPath> <pkgName> <version>
import { writeFileSync } from "node:fs";

const [outPath, pkgName, version] = process.argv.slice(2);
for (const [name, value] of Object.entries({ outPath, pkgName, version })) {
  if (!value) {
    console.error(`::error::missing argument: ${name}`);
    process.exit(1);
  }
}

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
  // pi core loads skills, prompts and themes. The crew is discovered by
  // the pi-subagents extension through its documented package convention,
  // pi.subagents.agents — pi core ignores that key rather than rejecting it.
  // Contributor steering is deliberately absent: it is documentation, and a
  // prompts entry would turn it into a slash command.
  pi: {
    skills: ["./.pi/skills"],
    subagents: { agents: ["./.pi/agents"] },
  },
  files: [".pi", "docs", "README.md", "LICENSE"],
};

writeFileSync(outPath, `${JSON.stringify(manifest, null, 2)}\n`);
console.log(`wrote ${outPath} for ${pkgName}@${version}`);
