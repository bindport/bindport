#!/usr/bin/env node

const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { spawnSync } = require("node:child_process");

const ROOT = path.resolve(__dirname, "..");
const NPM_ROOT = path.join(ROOT, "npm");
const WRAPPER_DIR = path.join(NPM_ROOT, "bindport");
const NPM_ENV = { ...process.env };
if (!NPM_ENV.npm_config_cache) {
  NPM_ENV.npm_config_cache = path.join(os.tmpdir(), "bindport-npm-cache");
}
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
  run("npm", args, { cwd: packageDir, env: NPM_ENV });
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
    if (optionalDependencies[platformPackage.name] !== version) {
      throw new Error(`bindport optional dependency ${platformPackage.name} must be ${version}`);
    }
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

function packAll({ destination, dryRun }) {
  validateVersions(currentWrapperVersion());
  packPackage(WRAPPER_DIR, destination, dryRun);

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
  npm-package-utils.js validate <x.y.z>
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
      case "validate":
        if (args.length !== 1) {
          usage();
          process.exit(2);
        }
        validateVersions(args[0]);
        break;
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
    process.exit(1);
  }
}

if (require.main === module) {
  main();
}
