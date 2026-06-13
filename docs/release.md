# Release Process

BindPort releases are review-first. Version bumps happen in a normal pull
request. A manually dispatched GitHub Actions workflow verifies reviewed `main`
and can create the stable Git tag and GitHub Release.

Cargo package publishing is manual. The npm wrapper is useful release glue, but
it must not be published until it can install or dispatch to real platform
binaries.

## Current Status

BindPort v0.1.0 is released on GitHub and crates.io. Cargo is the supported
install path:

```sh
cargo install bindport
```

The first release targets Linux and macOS-style local development. Windows
support remains future/beta until the cross-platform hardening milestone. npm is
not published yet because the package still needs native binary dispatch.

The minimum release gate is:

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
mise run release-prep
mise run release-prep patch
mise run release-prep minor
mise run release-prep major
mise run release-prep v0.2.0
mise run release-prep 0.2.0
```

When no argument is provided, `release-prep` defaults to `patch`.

Before prompting, `release-prep`:

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
- updates internal workspace dependency versions to the same version;
- updates `npm/bindport/package.json` to the same version;
- refreshes and validates Cargo metadata;
- runs `scripts/release-check.sh --version X.Y.Z --allow-dirty`;
- stages `Cargo.toml`, `Cargo.lock`, and `npm/bindport/package.json`;
- commits `build: prepare vX.Y.Z release`;
- pushes the branch to `origin`;
- opens a pull request with `gh pr create`.

The script intentionally stops before publishing release metadata. The release
workflow is dispatched only after the release prep PR has been reviewed and
merged.

## Release Check Gate

Release checks are intentionally non-publishing by default. They validate the
version, run the local CI gate, dry-run Cargo package contents, and dry-run the
npm package tarball. Full crates.io publish dry-runs are reserved for the
`--publish-ready` gate because Cargo publishing remains a separate manual
action.

Run it locally after a release-prep branch updates versions and package
artifacts:

```sh
mise run release-check 0.2.0
```

Or call the script directly:

```sh
scripts/release-check.sh --version 0.2.0
```

The same validation gate is available as the manual `Release Check` GitHub
Actions workflow. It never creates tags, publishes npm/Cargo packages, or commits
version bumps. Use its `publish_ready` input only when checking the Cargo package
state immediately before a manual crates.io publish. That input dry-runs the
full workspace publish order through `scripts/cargo-publish.sh`. npm remains a
separate future publish path while `npm/bindport/package.json` is private.

## Stable Release

After the release prep PR is merged, publish the reviewed release metadata with:

```sh
mise run release-publish --dry-run v0.2.0
mise run release-publish v0.2.0
```

When no version is provided, `release-publish` uses the workspace version in
`Cargo.toml`. The script requires a clean `main` branch synced with
`origin/main`, verifies that Cargo and npm versions match, checks that the
release tag does not already exist at another commit, and asks for confirmation
before dispatching the manual `Release` workflow.

The `Release` workflow is manual-only. It verifies that it is running from
`main`, checks that `vX.Y.Z` matches the Cargo and npm package versions, runs the
standard release check gate, verifies that the workflow did not modify source
files, builds release binaries, verifies release Git credentials with a
non-mutating dry-run tag push, creates or reuses annotated Git tag `vX.Y.Z`, and
creates or updates the GitHub Release.

Release artifacts are uploaded to the GitHub Release:

- `bindport-linux-x64`
- `bindport-linux-x64.sha256`
- `bindport-macos-arm64`
- `bindport-macos-arm64.sha256`

Dry runs run the release checks and release Git credential preflight. They do
not create Git tags or GitHub Releases. They still build release binaries so the
artifact matrix is validated before a real release.

The workflow needs `contents: write` to create the release tag and GitHub
Release. Create a `stable-release` environment in GitHub repository settings.
Add required reviewers to that environment if stable publishing should require
manual approval before the job runs.

## Cargo Publishing

Cargo users can install BindPort directly:

```sh
cargo install bindport
```

BindPort publishes one CLI crate plus internal support crates. The helper
publishes or dry-runs those crates in dependency order:

1. `bindport-core`
2. `bindport-adapters`
3. `bindport-runner`
4. `bindport-registry`
5. `bindport`

Run the local dry-run before any crates.io publish:

```sh
mise run cargo-publish 0.2.0
```

Or call the script directly:

```sh
scripts/cargo-publish.sh --version 0.2.0 --dry-run
```

The dry-run requires the Cargo workspace version and `npm/bindport/package.json`
version to match the requested release. From a dirty release-prep branch, pass
`--allow-dirty`; real publishing rejects dirty worktrees.

After the GitHub Release has been created from `main`, publish to crates.io:

```sh
mise run cargo-publish --execute 0.2.0
```

Real publishing additionally requires:

- a clean checkout at `origin/main`;
- release tag `vX.Y.Z` pointing at `HEAD`;
- a local `cargo login` token or `CARGO_REGISTRY_TOKEN`;
- interactive confirmation unless `--yes` is provided.

The script waits between package publishes so the crates.io index can observe
new internal crates before dependent crates are uploaded. Override the wait with
`--wait-seconds N` when needed.

The manual `Cargo Publish` GitHub Actions workflow exposes the same flow:

- `execute=false` runs only the dry-run.
- `execute=true` runs the dry-run first, then publishes in a second job.
- The publish job uses the `crates-io` environment and expects a
  `CARGO_REGISTRY_TOKEN` secret.

Keep `execute=false` until the local command has been exercised for the release.

## Versioning

- `0.0.x`: unreleased bootstrap only.
- `0.1.0`: first working runner release.
- Pre-1.0 minor releases may contain breaking changes.
- A stable release prep commit should update all package versions together.

## Package Names

Package name reservation is an external registry action. Do not do it from CI
and do not do it as part of ordinary bootstrap commits.

For crates.io, the `bindport`, `bindport-core`, `bindport-adapters`,
`bindport-runner`, and `bindport-registry` names are claimed. Published crate
versions are permanent and cannot be overwritten or deleted from the archive.
They can be yanked, but the version number remains used.

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
- Future platform packages should provide native binaries without moving Rust
  code into Node.

BindPort does not need a Node application stack. The runtime remains the Rust
binary; npm is only an install path for JavaScript projects that want to call
`bindport` from `package.json` scripts.

The preferred npm shape is a small wrapper package plus platform packages:

- `bindport`: user-facing wrapper and `bin` entry.
- `@bindport/linux-x64`: Linux x64 native binary.
- `@bindport/linux-arm64`: Linux arm64 native binary.
- `@bindport/darwin-x64`: macOS Intel native binary.
- `@bindport/darwin-arm64`: macOS Apple Silicon native binary.

The wrapper should declare platform packages as optional dependencies and
resolve the installed package for `process.platform` and `process.arch`. Avoid a
postinstall download script unless platform packages prove unworkable.

Before the first real npm publish:

1. Choose the final package name (`bindport` or `@bindport/cli`).
2. Add platform packages for the supported OS/architecture targets.
3. Make the wrapper resolve those packages reliably.
4. Verify `npx`, `npm exec`, and `bunx` against a packed local tarball.
5. Remove `"private": true` from `npm/bindport/package.json`.
6. From `npm/bindport`, verify `npm pack --dry-run`.

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

## Automation Policy

- `release-prep` may create a branch, commit, push, and open a PR after explicit
  confirmation.
- The `Release` workflow may create a Git tag and GitHub Release when
  `dry_run=false`.
- The `Release` workflow uploads native binaries and checksums to GitHub
  Releases.
- The `Cargo Publish` workflow may publish crates.io packages only when manually
  dispatched with `execute=true` and approved through the `crates-io`
  environment.
- No workflow publishes npm packages yet.
- No workflow commits version bumps back to the repository.
- Keep automatic package publishing disabled until the manual workflows have
  shipped cleanly.
