# Release Process

BindPort releases are review-first. Version bumps happen in a normal pull
request. A manually dispatched GitHub Actions workflow verifies reviewed `main`
and can create the stable Git tag and GitHub Release.

Cargo and npm package publishing are manual. The GitHub Release workflow builds
native binaries and npm tarballs; separate publish workflows/scripts push those
reviewed artifacts to crates.io and npm.

## Current Status

BindPort v0.4.0 is the release described by this document. Cargo and npm are
alternate supported install paths after the release workflow and manual publish
steps complete.

Global Cargo install:

```sh
cargo install bindport
```

JavaScript project dependency:

```sh
npm install --save-dev bindport
```

The current release targets Linux and macOS-style local development. Windows
support remains future/beta until the cross-platform hardening milestone. npm
publishing uses the unscoped `bindport` wrapper package plus scoped native
binary packages for Linux and macOS on x64/arm64.

The minimum release gate is:

1. `bindport -- <command>` allocates or reuses a port.
2. The child process receives `PORT=<assigned>`.
3. The child inherits stdio.
4. SIGINT and SIGTERM are forwarded.
5. BindPort exits with the child's exit code.
6. `bindport status --json` reports the latest service plus run history.
7. `bindport dashboard serve` starts on loopback by default and exposes
   `/api/status`.
8. The dashboard shows active, stopped, and stale services with URL copy/open
   actions.
9. Dashboard cleanup can remove stopped or stale entries, trigger output
   cleanup, and cannot mutate active services.
10. Local and CI checks pass.

The package-script gate is covered by the
`package_script_runs_bindport_next_dev_flow` integration test. It runs
`npm run --silent dev` in a temporary package whose `dev` script is
`bindport -- next dev`, with a fake `next` executable receiving `PORT` and the
registry recording `next dev`. This proves package-manager script dispatch
without adding a real Next.js dependency to the repository.

`status --json` exposes route-oriented fields (`hostname`, `route_url`,
`outputs`, and `proxy`). Service config and run options can populate
`hostname` and `route_url`; rendered output records populate top-level output
summaries and per-service output status. `proxy` is a compatibility alias for
recorded `traefik` output status.

The dashboard gate is covered by integration tests for status API parity,
static asset serving, token auth, stopped/stale cleanup, self-registration,
background service controls, and a 100-service registry snapshot.

The v0.3 template-output gate is covered by integration and unit tests for
output config parsing, local overrides, template lookup/export, MiniJinja strict
rendering, path safety, SQLite ownership, repair behavior, status output,
dashboard status parity, `render --all`, auto-render events, cleanup-triggered
deletion, and `doctor outputs` diagnostics. Real Traefik reload behavior remains
manual acceptance because CI should not depend on a local proxy daemon.

The v0.4 monorepo/config gate is covered by config explain/validate tests,
workspace inference tests, local override layering tests, service env rendering
tests, and the checked-in monorepo fixture. Manual acceptance should verify the
fixture from both service directories and confirm opt-in `.env.local` output
does not write unless configured.

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
mise run release-prep v0.4.0
mise run release-prep 0.4.0
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
- updates npm wrapper and platform package metadata to the same version;
- refreshes and validates Cargo metadata;
- runs `scripts/release-check.sh --version X.Y.Z --allow-dirty`;
- stages `Cargo.toml`, `Cargo.lock`, and npm package metadata;
- commits `build: prepare vX.Y.Z release`;
- pushes the branch to `origin`;
- opens a pull request with `gh pr create`.

The script intentionally stops before publishing release metadata. The release
workflow is dispatched only after the release prep PR has been reviewed and
merged.

## Release Check Gate

Release checks are intentionally non-publishing by default. They validate the
version, run the local CI gate, dry-run Cargo package contents, validate all npm
package versions and optional dependencies, and dry-run wrapper/platform npm
package tarballs. Full crates.io publish dry-runs are reserved for the
`--publish-ready` gate because Cargo publishing remains a separate manual
action.

Run it locally after a release-prep branch updates versions and package
artifacts:

```sh
mise run release-check 0.4.0
```

Or call the script directly:

```sh
scripts/release-check.sh --version 0.4.0
```

The same validation gate is available as the manual `Release Check` GitHub
Actions workflow. It never creates tags, publishes npm/Cargo packages, or commits
version bumps. Use its `publish_ready` input only when checking the Cargo package
state immediately before a manual crates.io publish. That input runs the
publish helper in dry-run mode. For a new workspace version, crates that depend
on unpublished internal crate versions may be skipped until their dependencies
exist on crates.io. npm publishing remains a separate manual action after the
GitHub Release artifacts have been created.

## Stable Release

After the release prep PR is merged, finish the release from clean, synced
`main` with:

```sh
mise run release-finish v0.4.0
```

`release-finish` is the normal post-merge path. It verifies the checkout, asks
for one confirmation, dispatches the real `Release` workflow, waits for that
workflow to pass, verifies the GitHub Release and tag, and then publishes the
Cargo crates. If the GitHub Release already exists at the current `main` commit,
it skips that step and proceeds to Cargo publishing. If Cargo publishing was
interrupted, rerun the same command; already-published crate versions are
skipped and the remaining crates continue in order.

Use these options for recovery or non-interactive runs:

```sh
mise run release-finish --yes v0.4.0
mise run release-finish --skip-github-release v0.4.0
mise run release-finish --skip-cargo-publish v0.4.0
```

`release-finish` waits for the manual `Release` workflow. If the
`stable-release` environment requires approval, approve the workflow in GitHub;
the local command continues polling until the workflow completes or times out.

The lower-level release dispatch command remains available:

```sh
mise run release-publish --dry-run v0.4.0
mise run release-publish v0.4.0
```

When no version is provided, `release-publish` and `release-finish` use the
workspace version in `Cargo.toml`. Both require a clean `main` branch synced
with `origin/main`, verify that Cargo and npm versions match, and check that
the release tag does not already exist at another commit.

The `Release` workflow is manual-only. It verifies that it is running from
`main`, checks that `vX.Y.Z` matches the Cargo and npm package versions, runs the
standard release check gate, verifies that the workflow did not modify source
files, builds release binaries, verifies release Git credentials with a
non-mutating dry-run tag push, creates or reuses annotated Git tag `vX.Y.Z`, and
creates or updates the GitHub Release.

Release artifacts are uploaded to the GitHub Release:

- `bindport-linux-x64`
- `bindport-linux-x64.sha256`
- `bindport-linux-arm64`
- `bindport-linux-arm64.sha256`
- `bindport-macos-x64`
- `bindport-macos-x64.sha256`
- `bindport-macos-arm64`
- `bindport-macos-arm64.sha256`
- `bindport-linux-x64-X.Y.Z.tgz`
- `bindport-linux-arm64-X.Y.Z.tgz`
- `bindport-darwin-x64-X.Y.Z.tgz`
- `bindport-darwin-arm64-X.Y.Z.tgz`
- `bindport-X.Y.Z.tgz`

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
5. `bindport-dashboard`
6. `bindport`

`release-finish` runs Cargo publishing automatically after the GitHub Release
exists. To exercise the Cargo publish helper directly, run the local dry-run:

```sh
mise run cargo-publish 0.4.0
```

Or call the script directly:

```sh
scripts/cargo-publish.sh --version 0.4.0 --dry-run
```

The dry-run requires the Cargo workspace version and `npm/bindport/package.json`
version to match the requested release. From a dirty release-prep branch, pass
`--allow-dirty`; real publishing rejects dirty worktrees. Because
`cargo publish --dry-run` resolves dependencies from the current crates.io
index, a new release may not be able to dry-run every crate before its internal
dependencies are published. The helper reports those skipped dry-runs instead
of failing the whole preflight.

After the GitHub Release has been created from `main`, publish to crates.io
directly with:

```sh
mise run cargo-publish --execute 0.4.0
```

Real publishing additionally requires:

- a clean checkout at `origin/main`;
- release tag `vX.Y.Z` pointing at `HEAD`;
- a local `cargo login` token or `CARGO_REGISTRY_TOKEN`;
- interactive confirmation unless `--yes` is provided. `release-finish` already
  performs its own confirmation and calls the Cargo publish helper with `--yes`.

The script waits between package publishes so the crates.io index can observe
new internal crates before dependent crates are uploaded. Override the wait with
`--wait-seconds N` when needed. If a publish is interrupted, rerun the same
command after the failure is corrected; already-published crate versions are
skipped and the remaining crates continue in order.

The manual `Cargo Publish` GitHub Actions workflow exposes the same flow:

- `execute=false` runs only the dry-run.
- `execute=true` runs the dry-run first, then publishes in a second job.
- The publish job uses the `crates-io` environment and expects a
  `CARGO_REGISTRY_TOKEN` secret.

Keep `execute=false` until the local command has been exercised for the release.

## v0.2.0 Manual Acceptance

Before merging the v0.2.0 release prep PR, smoke test the release branch from a
fresh checkout or clean worktree:

1. Build the release binary with `cargo build --release --locked`.
2. Run a wrapped command and confirm the child receives `PORT`.
3. Confirm `bindport status --json` reports the service and run history.
4. Start `bindport dashboard serve` and verify `/api/status` matches
   `bindport status --json`.
5. Verify the dashboard shows active, stopped, and stale groups as applicable.
6. Verify URL copy/open actions when `hostname` and `route_url` are configured.
7. Remove stopped or stale entries from the dashboard and confirm active entries
   remain.
8. Run `bindport dashboard start`, `bindport dashboard status`, and
   `bindport dashboard stop`.
9. For remote browser testing, run the dashboard on `0.0.0.0` with token auth
   and verify an unauthenticated request is rejected.
10. Run `mise run dev-dashboard-remote` and confirm Rust server changes restart
    the dev server while static asset changes refresh the browser.

## v0.3.0 Manual Acceptance

Before merging the v0.3.0 release prep PR, smoke test template outputs from a
fresh checkout or clean worktree:

1. Build the release binary with `cargo build --release --locked`.
2. Create a project config with a `bindport-traefik` output and a branch-based
   service hostname such as `hostname = "{branch}.orderful-website.localhost"`.
3. Run a wrapped command with that service and confirm BindPort writes a
   generated Traefik file under the configured output root.
4. Point an existing Traefik file provider at the generated directory with
   `watch = true`, then confirm Traefik reloads the generated file.
5. Visit a rendered hostname such as
   `feature-tree.orderful-website.localhost` and confirm it forwards to the
   correct wrapped process.
6. Repeat with a second project name such as `hoststamp` to confirm the hostname
   template keeps local app domains distinct.
7. Run `bindport render --dry-run`, `bindport render --all`, and
   `bindport doctor outputs` and confirm the planned output paths match the
   generated files.
8. Export the built-in template, point an output at the custom template name,
   and confirm the custom MiniJinja template renders without code changes.
9. Stop the wrapped command and confirm the generated Traefik file becomes
   comment-only YAML by default.
10. Run `bindport clean` and confirm DB-owned generated files are removed for
    removed routes when `delete_on = ["removed"]`.
11. Modify a DB-owned generated file, then run `bindport render --repair` and
    confirm the file is preserved and status reports `external_modified`.
12. If the dashboard itself is configured with a hostname/output entry, confirm
    the dashboard can be reached through Traefik and still rejects unauthenticated
    requests when dashboard auth is required.

## v0.4.0 Manual Acceptance

Before merging the v0.4.0 release prep PR, smoke test monorepo config behavior
from a fresh checkout or clean worktree:

1. Build the release binary with `cargo build --release --locked`.
2. From `examples/monorepo`, run `bindport config validate` and confirm
   `validation: ok`.
3. From `examples/monorepo/apps/web`, run `bindport config explain` and confirm
   the service resolves to `web` from `[[services]].path`.
4. From `examples/monorepo/apps/api`, run `bindport config explain` and confirm
   the service resolves to `api` from `[[services]].path`.
5. Run `bindport run web -- sh -c 'printf "%s\n" "$PORT"'` and confirm the
   wrapped process receives the assigned port.
6. Run a service with configured env templates and confirm `HOSTNAME`,
   `NEXT_PUBLIC_BINDPORT_URL`, or equivalent service env values are injected.
7. Copy `.bindport.local.toml.sample` to `.bindport.local.toml`, adjust a local
   field such as `output_defaults.target_host`, and confirm
   `bindport config explain` reports the local override source.
8. Run `bindport doctor outputs` with a temp registry path and confirm both
   `bindport-traefik` and `bindport-env-local` resolve.
9. Run `bindport render --dry-run` and confirm both configured outputs plan
   without writing files.
10. Run `bindport render env-local` only after opting into that output and
    confirm DB-owned `.env.local` files are written under the intended package
    paths, while unowned existing files are not overwritten.
11. Clone or create a second worktree, run the same service from both
    worktrees, and confirm `bindport status --json` reports distinct identities
    without port collisions.

## Versioning

- `0.0.x`: unreleased bootstrap only.
- `0.1.0`: first working runner release.
- `0.2.0`: local dashboard API, embedded UI, and stopped/stale cleanup.
- `0.3.0`: template output rendering and built-in Traefik file-provider output.
- `0.4.0`: monorepo config depth, validation, local overrides, and env outputs.
- Pre-1.0 minor releases may contain breaking changes.
- A stable release prep commit should update all package versions together.

## Package Names

Package name reservation is an external registry action. Do not do it from CI
and do not do it as part of ordinary bootstrap commits.

For crates.io, the `bindport`, `bindport-core`, `bindport-adapters`,
`bindport-dashboard`, `bindport-runner`, and `bindport-registry` names are
claimed. Published crate versions are permanent and cannot be overwritten or
deleted from the archive. They can be yanked, but the version number remains
used.

For npm, the `bindport` wrapper package name is final. The native packages use
the `@bindport/*` scope and must be published with public access. Because a used
`package@version` cannot be reused even after unpublish, publish npm only from
reviewed GitHub Release tarballs. The `@bindport` npm scope must exist before
the native packages can be published.

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

The npm shape is a small wrapper package plus platform packages:

- `bindport`: user-facing wrapper and `bin` entry.
- `@bindport/linux-x64`: Linux x64 native binary.
- `@bindport/linux-arm64`: Linux arm64 native binary.
- `@bindport/darwin-x64`: macOS Intel native binary.
- `@bindport/darwin-arm64`: macOS Apple Silicon native binary.

The wrapper declares platform packages as optional dependencies and resolves the
installed package for `process.platform` and `process.arch`. There is no
postinstall download script; release-built platform package tarballs contain the
native binaries.

Before a real npm publish:

1. Run the GitHub `Release` workflow for the version.
2. Confirm the GitHub Release contains all raw binaries, checksums, and npm
   tarballs.
3. Run the `npm Publish` workflow with `execute=false`.
4. Run the `npm Publish` workflow with `execute=true` after the dry-run passes.

Local npm publish dry-run from downloaded release tarballs:

```sh
gh release download v0.4.0 --pattern "*.tgz" --dir dist/npm
mise run npm-publish v0.4.0 --dist dist/npm
```

Real local npm publish, when intentionally bypassing the workflow:

```sh
mise run npm-publish v0.4.0 --dist dist/npm --execute
```

Publish order matters: native platform packages are published first, then the
`bindport` wrapper. The `npm-publish` script enforces that order.

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
    "bindport": "^0.4.0"
  }
}
```

## Automation Policy

- `release-prep` may create a branch, commit, push, and open a PR after explicit
  confirmation.
- `release-finish` may dispatch the real `Release` workflow and publish Cargo
  crates after explicit confirmation.
- The `Release` workflow may create a Git tag and GitHub Release when
  `dry_run=false`.
- The `Release` workflow uploads native binaries and checksums to GitHub
  Releases.
- The `Cargo Publish` workflow may publish crates.io packages only when manually
  dispatched with `execute=true` and approved through the `crates-io`
  environment.
- The `npm Publish` workflow may publish npm packages only when manually
  dispatched with `execute=true` and approved through the `npm` environment.
- No workflow commits version bumps back to the repository.
- Keep automatic package publishing disabled until the manual workflows have
  shipped cleanly.
