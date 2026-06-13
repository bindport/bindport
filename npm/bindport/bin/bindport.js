#!/usr/bin/env node

const { existsSync } = require("node:fs");
const { spawnSync } = require("node:child_process");
const { constants } = require("node:os");
const path = require("node:path");

const binaryName = process.platform === "win32" ? "bindport.exe" : "bindport";
const candidates = [
  path.join(__dirname, binaryName),
  path.join(__dirname, "..", "vendor", `${process.platform}-${process.arch}`, binaryName),
];

const binary = candidates.find((candidate) => existsSync(candidate));

if (!binary) {
  console.error("BindPort npm wrapper is a bootstrap placeholder.");
  console.error("Use `cargo install bindport` until npm native binary packaging is wired.");
  process.exit(1);
}

const result = spawnSync(binary, process.argv.slice(2), { stdio: "inherit" });

if (result.error) {
  console.error(result.error.message);
  process.exit(1);
}

if (result.signal) {
  process.exit(128 + (constants.signals[result.signal] ?? 1));
}

process.exit(result.status ?? 1);
