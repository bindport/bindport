#!/usr/bin/env node

const { existsSync } = require("node:fs");
const { spawnSync } = require("node:child_process");
const { constants } = require("node:os");
const path = require("node:path");

const binaryName = process.platform === "win32" ? "bindport.exe" : "bindport";
const platformKey = `${process.platform}-${process.arch}`;
const platformPackages = {
  "darwin-arm64": "@bindport/darwin-arm64",
  "darwin-x64": "@bindport/darwin-x64",
  "linux-arm64": "@bindport/linux-arm64",
  "linux-x64": "@bindport/linux-x64",
};

function packageBinary(packageName) {
  try {
    const packageJson = require.resolve(`${packageName}/package.json`);
    return path.join(path.dirname(packageJson), "bin", binaryName);
  } catch (_) {
    return null;
  }
}

const candidates = [
  process.env.BINDPORT_BINARY,
  packageBinary(platformPackages[platformKey]),
  path.join(__dirname, binaryName),
  path.join(__dirname, "..", "vendor", `${process.platform}-${process.arch}`, binaryName),
].filter(Boolean);

const binary = candidates.find((candidate) => existsSync(candidate));

if (!binary) {
  const supported = Object.keys(platformPackages).sort().join(", ");
  console.error(`BindPort does not provide an npm binary for ${platformKey}.`);
  console.error(`Supported npm platforms: ${supported}.`);
  console.error("Set BINDPORT_BINARY to a local bindport binary, or install with `cargo install bindport`.");
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
