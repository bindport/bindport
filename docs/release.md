# Release Process

BindPort releases are review-first. Version bumps happen in a normal pull
request. A manually dispatched GitHub Actions workflow verifies reviewed `main`
and can create the stable Git tag and GitHub Release.

Cargo and npm package publishing remain manual until the native binary artifact
story is real. The npm wrapper is useful release glue, but it must not be
published until it can install or dispatch to real platform binaries.

## Current Status

The repository version stays at `0.0.0` until the first v0.1 release candidate
can prove the package-script wrapper:

```sh
bindport -- next dev
```

Do not publish any `0.0.x` version to npm or crates.io. The `0.0.x` range is
local bootstrap only.

The first v0.1 release targets Linux and macOS-style local development. Windows
support remains future/beta until the cross-platform hardening milestone.

Before v0.1, the minimum release gate is:

1. `bindport -- <command>` allocates or reuses a port.
2. The child process receives `PORT=<assigned>`.
3. The child inherits stdio.
4. SIGINT and SIGTERM are forwarded.
5. BindPort exits with the child's exit code.
6. `bindport status --json` reports the latest service plus run history.
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

## Release Prep PR

Create a release prep pull request with:

```sh
mise run release-pr -- minor
mise run release-pr -- patch
mise run release-pr -- major
mise run release-pr -- v0.1.0
mise run release-pr -- 0.1.0
```

An argument is required. For the first release from `0.0.0`, use `minor` or an
explicit `v0.1.0`.
The `--` separator passes the release argument through `mise` to the script.

Before prompting, `release-pr`:

- requires a clean `main` branch synced with `origin/main`;
- requires Cargo and npm versions to match;
- requires the current version to be stable `X.Y.Z`;
- rejects `0.0.x` release targets;
- accepts `patch`, `minor`, `major`, `vX.Y.Z`, or `X.Y.Z`;
- rejects explicit target versions that are not greater than the current
  version;
- rejects release branches that already exist locally or on `origin`.

After confirmation, it:

- creates `release/vX.Y.Z` from `main`;
- updates the Cargo workspace version with `cargo set-version --workspace`;
- updates `npm/bindport/package.json` to the same version;
- refreshes and validates Cargo metadata;
- runs `scripts/release-prep.sh --version X.Y.Z`;
- stages `Cargo.toml`, `Cargo.lock`, and `npm/bindport/package.json`;
- commits `build: prepare vX.Y.Z release`;
- pushes the branch to `origin`;
- opens a pull request with `gh pr create`.

The script intentionally stops before publishing release metadata. The release
workflow is dispatched only after the release prep PR has been reviewed and
merged.

## Release Validation Gate

Release prep validation is intentionally non-publishing. It validates the
version, runs the local CI gate, dry-runs Cargo package contents, and dry-runs
the npm package tarball. Registry publish dry-runs are reserved for the
`--publish-ready` gate because npm and crates.io publishing remain manual until
native binary packaging is ready.

Run it locally after a release-prep branch updates versions and package
artifacts:

```sh
RELEASE_VERSION=0.1.0 mise run release-prep
```

Or call the script directly:

```sh
scripts/release-prep.sh --version 0.1.0
```

The same validation gate is available as the manual `Release Prep` GitHub
Actions workflow. It never creates tags, publishes npm/Cargo packages, or commits
version bumps. Use its `publish_ready` input only when checking the final package
state immediately before a manual npm or crates.io publish.

## Stable Release

After the release prep PR is merged, publish the reviewed release metadata with:

```sh
mise run release-publish -- --dry-run v0.1.0
mise run release-publish -- v0.1.0
```

When no version is provided, `release-publish` uses the workspace version in
`Cargo.toml`. The script requires a clean `main` branch synced with
`origin/main`, verifies that Cargo and npm versions match, checks that the
release tag does not already exist at another commit, and asks for confirmation
before dispatching the manual `Release` workflow.

The `Release` workflow is manual-only. It verifies that it is running from
`main`, checks that `vX.Y.Z` matches the Cargo and npm package versions, runs the
standard release-prep gate, verifies that the workflow did not modify source
files, verifies release Git credentials with a non-mutating dry-run tag push,
creates or reuses annotated Git tag `vX.Y.Z`, and creates or updates the GitHub
Release.

Dry runs run the release checks and release Git credential preflight. They do
not create Git tags or GitHub Releases.

The workflow needs `contents: write` to create the release tag and GitHub
Release. Create a `stable-release` environment in GitHub repository settings.
Add required reviewers to that environment if stable publishing should require
manual approval before the job runs.

## Versioning

- `0.0.x`: unreleased bootstrap only.
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

Before the first real npm publish:

1. Choose the final package name (`bindport` or `@bindport/cli`).
2. Build or download native binaries for the supported OS/architecture targets.
3. Make the wrapper resolve those binaries reliably.
4. Remove `"private": true` from `npm/bindport/package.json`.
5. From `npm/bindport`, verify `npm pack --dry-run`.

First public publish, when ready:

```sh
cd npm/bindport
npm publish --access public
```

For the unscoped name `bindport`, `--access public` is unnecessary because
public access is the default. The flag is required only for a scoped public
package such as `@bindport/cli`. Revisit the command once the final package name
is chosen.

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

- `release-pr` may create a branch, commit, push, and open a PR after explicit
  confirmation.
- The `Release` workflow may create a Git tag and GitHub Release when
  `dry_run=false`.
- No workflow publishes npm or crates.io packages yet.
- No workflow commits version bumps back to the repository.
- Add automated package publishing only after the native binary artifact process
  has shipped at least one working release candidate cleanly.
