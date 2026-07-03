#!/usr/bin/env node

const { spawnSync } = require("node:child_process");

const EXPECTED = {
  "aarch64-apple-darwin":
    "{ repo }/releases/download/v{ version }/bindport-macos-arm64{ archive-suffix }",
  "aarch64-unknown-linux-gnu":
    "{ repo }/releases/download/v{ version }/bindport-linux-arm64{ archive-suffix }",
  "x86_64-apple-darwin":
    "{ repo }/releases/download/v{ version }/bindport-macos-x64{ archive-suffix }",
  "x86_64-unknown-linux-gnu":
    "{ repo }/releases/download/v{ version }/bindport-linux-x64{ archive-suffix }",
};

function fail(message) {
  console.error(`binstall metadata: ${message}`);
  process.exit(1);
}

const metadataResult = spawnSync(
  "cargo",
  ["metadata", "--no-deps", "--format-version", "1"],
  { encoding: "utf8" },
);

if (metadataResult.error) {
  throw metadataResult.error;
}
if (metadataResult.status !== 0) {
  process.stderr.write(metadataResult.stderr);
  process.exit(metadataResult.status);
}

const metadata = JSON.parse(metadataResult.stdout);
const bindport = metadata.packages.find((pkg) => pkg.name === "bindport");
if (!bindport) {
  fail("bindport package missing from cargo metadata");
}

const binstall = bindport.metadata?.binstall;
if (!binstall) {
  fail("missing [package.metadata.binstall]");
}

if (binstall["pkg-fmt"] !== "bin") {
  fail(`pkg-fmt must be bin, got ${binstall["pkg-fmt"]}`);
}
if (binstall["bin-dir"] !== "{ bin }{ binary-ext }") {
  fail(`bin-dir must be { bin }{ binary-ext }, got ${binstall["bin-dir"]}`);
}

const disabled = binstall["disabled-strategies"] ?? [];
for (const strategy of ["quick-install", "compile"]) {
  if (!disabled.includes(strategy)) {
    fail(`disabled-strategies must include ${strategy}`);
  }
}

const overrides = binstall.overrides ?? {};
for (const [target, pkgUrl] of Object.entries(EXPECTED)) {
  const override = overrides[target];
  if (!override) {
    fail(`missing override for ${target}`);
  }
  if (override["pkg-url"] !== pkgUrl) {
    fail(`${target} pkg-url must be ${pkgUrl}, got ${override["pkg-url"]}`);
  }
}

const extraTargets = Object.keys(overrides).filter((target) => !EXPECTED[target]);
if (extraTargets.length > 0) {
  fail(`unexpected overrides: ${extraTargets.join(", ")}`);
}

console.log("binstall metadata ok");
