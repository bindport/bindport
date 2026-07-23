#!/usr/bin/env node

const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { spawnSync } = require("node:child_process");

const ROOT = path.resolve(__dirname, "..");
const NPM_ROOT = path.join(ROOT, "npm");
const WRAPPER_DIR = path.join(NPM_ROOT, "bindport");
const NPM_ENV = { ...process.env };
let npmCacheRoot = null;

function npmEnvironment() {
  if (!NPM_ENV.npm_config_cache && !NPM_ENV.NPM_CONFIG_CACHE && !npmCacheRoot) {
    npmCacheRoot = fs.mkdtempSync(path.join(os.tmpdir(), "bindport-npm-cache-"));
    NPM_ENV.npm_config_cache = path.join(npmCacheRoot, "cache");
  }
  return NPM_ENV;
}

const WORKSPACE_DEPENDENCIES = [
  "bindport-adapters",
  "bindport-core",
  "bindport-dashboard",
  "bindport-registry",
  "bindport-runner",
];
const PLATFORM_PACKAGES = [
  {
    key: "darwin-arm64",
    dir: "bindport-darwin-arm64",
    name: "@bindport/darwin-arm64",
  },
  {
    key: "darwin-x64",
    dir: "bindport-darwin-x64",
    name: "@bindport/darwin-x64",
  },
  {
    key: "linux-arm64",
    dir: "bindport-linux-arm64",
    name: "@bindport/linux-arm64",
  },
  {
    key: "linux-x64",
    dir: "bindport-linux-x64",
    name: "@bindport/linux-x64",
  },
];

function packagePath(packageDir) {
  return path.join(packageDir, "package.json");
}

function readJson(file) {
  return JSON.parse(fs.readFileSync(file, "utf8"));
}

function writeJson(file, value) {
  fs.writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`);
}

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    cwd: options.cwd ?? ROOT,
    env: options.env ?? process.env,
    stdio: options.stdio ?? "inherit",
  });

  if (result.error) {
    throw result.error;
  }
  if (result.status !== 0) {
    throw new Error(`${command} ${args.join(" ")} failed with ${result.status}`);
  }
}

function copyPackageForPacking(packageDir) {
  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "bindport-npm-pack-"));
  const tempPackage = path.join(tempRoot, path.basename(packageDir));
  fs.cpSync(packageDir, tempPackage, {
    recursive: true,
    filter: (source) => !source.includes(`${path.sep}node_modules${path.sep}`),
  });
  fs.copyFileSync(path.join(ROOT, "LICENSE"), path.join(tempPackage, "LICENSE"));
  return { tempRoot, tempPackage };
}

function ensureFakeBinary(packageDir) {
  const binDir = path.join(packageDir, "bin");
  fs.mkdirSync(binDir, { recursive: true });
  const binary = path.join(binDir, process.platform === "win32" ? "bindport.exe" : "bindport");
  fs.writeFileSync(binary, "#!/usr/bin/env sh\nprintf 'bindport npm smoke %s\\n' \"$*\"\n");
  fs.chmodSync(binary, 0o755);
  return binary;
}

function packPackage(packageDir, destination, dryRun) {
  const args = ["pack"];
  if (dryRun) {
    args.push("--dry-run");
  }
  if (destination) {
    fs.mkdirSync(destination, { recursive: true });
    args.push("--pack-destination", destination);
  }
  run("npm", args, { cwd: packageDir, env: npmEnvironment() });
}

function validateVersions(version) {
  const wrapper = readJson(packagePath(WRAPPER_DIR));
  if (wrapper.name !== "bindport") {
    throw new Error(`wrapper package name must be bindport, got ${wrapper.name}`);
  }
  if (wrapper.private === true) {
    throw new Error("wrapper package must not be private for npm publishing");
  }
  if (wrapper.version !== version) {
    throw new Error(`wrapper version ${wrapper.version} does not match ${version}`);
  }
  if (JSON.stringify(wrapper.os) !== JSON.stringify(["darwin", "linux"])) {
    throw new Error("wrapper os must restrict installation to darwin and linux");
  }
  if (!wrapper.files?.includes("LICENSE")) {
    throw new Error("wrapper files must include LICENSE");
  }

  const optionalDependencies = wrapper.optionalDependencies ?? {};
  for (const platformPackage of PLATFORM_PACKAGES) {
    const packageDir = path.join(NPM_ROOT, platformPackage.dir);
    const packageJson = readJson(packagePath(packageDir));
    if (packageJson.name !== platformPackage.name) {
      throw new Error(`${platformPackage.dir} name must be ${platformPackage.name}`);
    }
    if (packageJson.private === true) {
      throw new Error(`${platformPackage.name} must not be private`);
    }
    if (packageJson.version !== version) {
      throw new Error(`${platformPackage.name} version ${packageJson.version} does not match ${version}`);
    }
    if (!packageJson.files?.includes("LICENSE")) {
      throw new Error(`${platformPackage.name} files must include LICENSE`);
    }
    if (optionalDependencies[platformPackage.name] !== version) {
      throw new Error(`bindport optional dependency ${platformPackage.name} must be ${version}`);
    }
  }
}

function rewriteWorkspaceDependencyVersions(contents, version) {
  const lines = contents.split("\n");
  for (const crate of WORKSPACE_DEPENDENCIES) {
    const prefix = `${crate} = {`;
    const indexes = lines
      .map((line, index) => line.startsWith(prefix) ? index : -1)
      .filter(index => index >= 0);
    if (indexes.length !== 1) {
      throw new Error(`expected exactly one workspace dependency entry for ${crate}`);
    }
    const index = indexes[0];
    const line = lines[index];
    if (!line.includes(`path = "crates/${crate}"`)) {
      throw new Error(`${crate} workspace dependency has an unexpected path`);
    }
    const versions = line.match(/\bversion\s*=\s*"[^"]*"/g) ?? [];
    if (versions.length !== 1) {
      throw new Error(`${crate} workspace dependency must have exactly one version key`);
    }
    lines[index] = line.replace(
      /(\bversion\s*=\s*")[^"]*(")/,
      `$1${version}$2`,
    );
  }
  return lines.join("\n");
}

function updateWorkspaceDependencyVersions(version) {
  const manifest = path.join(ROOT, "Cargo.toml");
  const contents = fs.readFileSync(manifest, "utf8");
  fs.writeFileSync(manifest, rewriteWorkspaceDependencyVersions(contents, version));
}

function testWorkspaceDependencyRewrite() {
  const fixture = WORKSPACE_DEPENDENCIES.map((crate, index) =>
    `${crate} = { path = "crates/${crate}", version = "0.7.0", feature-${index} = true }`
  ).join("\n");
  const rewritten = rewriteWorkspaceDependencyVersions(fixture, "0.8.0");
  for (const [index, crate] of WORKSPACE_DEPENDENCIES.entries()) {
    if (!rewritten.includes(
      `${crate} = { path = "crates/${crate}", version = "0.8.0", feature-${index} = true }`,
    )) {
      throw new Error(`${crate} workspace dependency rewrite dropped an existing key`);
    }
  }
  let rejected = false;
  try {
    rewriteWorkspaceDependencyVersions(fixture.replace('version = "0.7.0", ', ""), "0.8.0");
  } catch {
    rejected = true;
  }
  if (!rejected) {
    throw new Error("workspace dependency rewrite accepted a missing version key");
  }
}

function updateVersions(version) {
  const wrapperPath = packagePath(WRAPPER_DIR);
  const wrapper = readJson(wrapperPath);
  wrapper.version = version;
  wrapper.optionalDependencies = wrapper.optionalDependencies ?? {};

  for (const platformPackage of PLATFORM_PACKAGES) {
    wrapper.optionalDependencies[platformPackage.name] = version;
    const platformPath = packagePath(path.join(NPM_ROOT, platformPackage.dir));
    const platformJson = readJson(platformPath);
    platformJson.version = version;
    writeJson(platformPath, platformJson);
  }

  writeJson(wrapperPath, wrapper);
}

function packWrapper(destination, dryRun) {
  const wrapper = copyPackageForPacking(WRAPPER_DIR);
  try {
    packPackage(wrapper.tempPackage, destination, dryRun);
  } finally {
    fs.rmSync(wrapper.tempRoot, { recursive: true, force: true });
  }
}

function packAll({ destination, dryRun }) {
  validateVersions(currentWrapperVersion());
  packWrapper(destination, dryRun);

  for (const platformPackage of PLATFORM_PACKAGES) {
    const sourceDir = path.join(NPM_ROOT, platformPackage.dir);
    const { tempRoot, tempPackage } = copyPackageForPacking(sourceDir);
    try {
      ensureFakeBinary(tempPackage);
      packPackage(tempPackage, destination, dryRun);
    } finally {
      fs.rmSync(tempRoot, { recursive: true, force: true });
    }
  }
}

function currentWrapperVersion() {
  return readJson(packagePath(WRAPPER_DIR)).version;
}

function usage() {
  console.error(`Usage:
  npm-package-utils.js list-platforms
  npm-package-utils.js current-version
  npm-package-utils.js update-version <x.y.z>
  npm-package-utils.js update-workspace-dependencies <x.y.z>
  npm-package-utils.js test-workspace-dependency-rewrite
  npm-package-utils.js validate <x.y.z>
  npm-package-utils.js pack-wrapper --destination <dir>
  npm-package-utils.js pack-check [--dry-run] [--destination <dir>]`);
}

function main() {
  const [command, ...args] = process.argv.slice(2);
  try {
    switch (command) {
      case "list-platforms":
        for (const platformPackage of PLATFORM_PACKAGES) {
          console.log(`${platformPackage.key} ${platformPackage.dir} ${platformPackage.name}`);
        }
        break;
      case "current-version":
        console.log(currentWrapperVersion());
        break;
      case "update-version":
        if (args.length !== 1) {
          usage();
          process.exit(2);
        }
        updateVersions(args[0]);
        break;
      case "update-workspace-dependencies":
        if (args.length !== 1) {
          usage();
          process.exit(2);
        }
        updateWorkspaceDependencyVersions(args[0]);
        break;
      case "test-workspace-dependency-rewrite":
        if (args.length !== 0) {
          usage();
          process.exit(2);
        }
        testWorkspaceDependencyRewrite();
        break;
      case "validate":
        if (args.length !== 1) {
          usage();
          process.exit(2);
        }
        validateVersions(args[0]);
        break;
      case "pack-wrapper": {
        if (args.length !== 2 || args[0] !== "--destination" || args[1].startsWith("--")) {
          usage();
          process.exit(2);
        }
        const destination = path.resolve(args[1]);
        validateVersions(currentWrapperVersion());
        packWrapper(destination, false);
        break;
      }
      case "pack-check": {
        let dryRun = false;
        let destination = null;
        for (let index = 0; index < args.length; index += 1) {
          const arg = args[index];
          if (arg === "--dry-run") {
            dryRun = true;
          } else if (arg === "--destination") {
            const destinationArg = args[index + 1];
            if (!destinationArg || destinationArg.startsWith("--")) {
              usage();
              process.exit(2);
            }
            destination = path.resolve(destinationArg);
            index += 1;
          } else {
            usage();
            process.exit(2);
          }
        }
        packAll({ destination, dryRun });
        break;
      }
      default:
        usage();
        process.exit(2);
    }
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  } finally {
    if (npmCacheRoot) {
      fs.rmSync(npmCacheRoot, { recursive: true, force: true });
    }
  }
}

if (require.main === module) {
  main();
}
