# Release Process

BindPort releases are not wired yet. This document records the intended release
shape so bootstrap work does not accidentally create a publish path before the
runner and package artifacts are real.

## Current Status

The repository version stays at `0.0.0` until the first v0.1 release candidate
can prove the package-script wrapper:

```sh
bindport -- next dev
```

Do not publish `0.0.0` to npm or crates.io. It is a local bootstrap version, not
a release artifact.

The first v0.1 release targets Linux and macOS-style local development. Windows
support remains future/beta until the cross-platform hardening milestone.

Before v0.1, the minimum release gate is:

1. `bindport -- <command>` allocates or reuses a port.
2. The child process receives `PORT=<assigned>`.
3. The child inherits stdio.
4. SIGINT and SIGTERM are forwarded.
5. BindPort exits with the child's exit code.
6. `bindport status --json` reports the run.
7. Local and CI checks pass.

The package-script gate is covered by the
`package_script_runs_bindport_next_dev_flow` integration test. It runs
`npm run --silent dev` in a temporary package whose `dev` script is
`bindport -- next dev`, with a fake `next` executable receiving `PORT` and the
registry recording `next dev`. This proves package-manager script dispatch
without adding a real Next.js dependency to the repository.

`status --json` exposes route-oriented fields (`hostname`, `route_url`, and
`proxy`) as `null` until the Traefik adapter begins rendering routes. That keeps
the v0.1 agent-facing shape explicit without claiming v0.2 proxy behavior.

## Local Release Checks

Run the standard local gate before any release prep branch:

```sh
mise run ci
```

If the local shell refuses to load this repo's trusted `mise.toml`, run:

```sh
MISE_TRUSTED_CONFIG_PATHS=$PWD mise run ci
```

## Versioning

- `0.0.0`: unreleased bootstrap only.
- `0.1.0`: first working runner release.
- Pre-1.0 minor releases may contain breaking changes.
- A stable release prep commit should update all package versions together.

## Package Names

Package name reservation is an external registry action. Do not do it from CI
and do not do it as part of ordinary bootstrap commits.

For crates.io, there is no separate reservation command in Cargo. The name is
claimed by publishing the first crate version. Because published crate versions
are permanent and cannot be overwritten or deleted from the archive, wait until
the Rust package is ready to publish.

For npm, the name is claimed by publishing the first package version. Unscoped
packages such as `bindport` are public. Scoped packages such as `@bindport/cli`
are private by default and require `--access public` for public visibility.
Because a used `package@version` cannot be reused even after unpublish, wait
until the wrapper can install or dispatch to a real native binary.

## npm Package Shape

The npm package lives in `npm/bindport`. Keep Node code limited to release and
install glue:

- `package.json` declares the `bindport` bin entry.
- `bin/bindport.js` finds and executes the platform native binary.
- Future platform packages or downloaded binaries can live below the npm package
  without moving Rust code into Node.

BindPort does not need a Node application stack. The runtime remains the Rust
binary; npm is only an install path for JavaScript projects that want to call
`bindport` from `package.json` scripts.

## Bun / bunx Workflow

`bunx` runs npm package executables declared in `package.json`'s `bin` field.
Because this package declares:

```json
{
  "bin": {
    "bindport": "bin/bindport.js"
  }
}
```

the unscoped package can be run as:

```sh
bunx bindport --help
bunx bindport -- doctor
bunx bindport -- next dev
```

If the final package name is scoped, for example `@bindport/cli`, use Bun's
package flag because the package name and executable name differ:

```sh
bunx --package @bindport/cli bindport --help
bunx -p @bindport/cli bindport -- next dev
```

Arguments after the executable name are passed through to BindPort. Bun flags
must appear before the package or executable name.

By default, Bun respects the shim's `#!/usr/bin/env node` shebang and runs it
with Node. Bun-only users can force Bun to run the JavaScript shim instead:

```sh
bunx --bun bindport --help
bunx --bun bindport -- next dev
```

Keep the Node shebang unless we deliberately drop npm/npx compatibility. The
shim is small CommonJS glue and works under both Node and Bun.

For committed project scripts, prefer installing BindPort as a development
dependency and calling the local executable instead of auto-installing on every
run:

```json
{
  "scripts": {
    "dev": "bindport -- next dev"
  },
  "devDependencies": {
    "bindport": "^0.1.0"
  }
}
```

Before publishing, dry-run the package:

```sh
cd npm/bindport
npm pack --dry-run
```

Before the first real publish:

1. Choose the final package name (`bindport` or `@bindport/cli`).
2. Remove `"private": true` from `npm/bindport/package.json`.
3. Bump the npm package version from `0.0.0` to the release version.
4. Verify the package includes a real native binary install/dispatch path.

First public publish, when ready:

```sh
cd npm/bindport
npm publish --access public
```

For the unscoped name `bindport`, `--access public` is unnecessary because
public access is the default. The flag is required only for a scoped public
package such as `@bindport/cli`. Revisit the command once the final package name
is chosen.

## Cargo Package Shape

The Cargo package is the Rust CLI crate `bindport`. Before publishing:

```sh
cargo publish -p bindport --dry-run
cargo package -p bindport --list
```

First publish, when ready:

```sh
cargo publish -p bindport
```

## Automation Policy

Stable publishing should stay manual until v0.1 has a real artifact story:

- no workflow should create tags yet;
- no workflow should publish to npm or crates.io yet;
- no workflow should commit version bumps back to the repository;
- release prep should happen through a normal reviewed pull request.

Add automated publishing only after the manual process has shipped at least one
working release candidate cleanly.
