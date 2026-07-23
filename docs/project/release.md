# Release Process

BindPort releases are review-first. Version bumps happen in a normal pull
request. A manually dispatched GitHub Actions workflow verifies reviewed `main`
and can create the stable Git tag and GitHub Release.

Cargo and npm package publishing are manual. The GitHub Release workflow builds
native binaries and npm tarballs; separate publish workflows/scripts push those
reviewed artifacts to crates.io and npm.

GitHub Releases also include `bindport-completions.tar.gz` and
`bindport-manpage.tar.gz`. These are static release assets sourced from
`packaging/`, validated by `scripts/check-cli-assets.sh`, and intended for
package-manager formulas such as the Homebrew tap.

Install-channel user guidance lives in [Install BindPort](../getting-started/install.md). Keep it
in sync when changing release assets, package names, the Homebrew tap formula,
or npm/Cargo publish behavior.

## Current Status

This process is version-neutral. `Cargo.toml` and the npm manifests are the
authority for the source version; public channel availability must be checked
for the specific release rather than inferred from this document. Cargo and npm
become install paths for a version only after their separate publish steps
complete.

Global Cargo install:

```sh
cargo install bindport
```

JavaScript project dependency:

```sh
npm install --save-dev bindport
```

The current release targets Linux and macOS-style local development. Windows is
post-1.0 and is not a supported install target yet. npm publishing uses the
unscoped `bindport` wrapper package plus scoped native binary packages for Linux
and macOS on x64/arm64. See [Platform Support](../reference/platform-support.md) for the
supported OS, package, path, and process behavior.

The minimum release gate is:

1. `bindport -- <command>` allocates or reuses a port.
2. The child process receives `PORT=<assigned>`.
3. The child inherits stdio.
4. SIGINT and SIGTERM are forwarded.
5. BindPort returns a normal child's exit code unchanged; Unix signal
   termination follows the documented `128 + signal` convention.
6. `bindport status --json` reports the latest service plus run history using
   the documented status schema.
7. Configured service commands can pass assigned values through env templates
   or explicit command arguments such as `--port {port}`.
8. `bindport dashboard serve` starts on loopback by default and exposes
   `/api/status`.
9. The dashboard shows active, stopped, and stale services with URL copy/open
   actions.
10. Dashboard cleanup can remove stopped or stale entries, trigger output
   cleanup, and cannot mutate active services.
11. Hooks are disabled until trusted, report pending/approved/denied/changed
    status, and can be inspected from CLI and dashboard surfaces.
12. Health checks report `unknown`, `pending`, `healthy`, or `failing` for
    supported loopback HTTP targets.
13. Local and CI checks pass.

The package-script gate is covered by the
`package_script_runs_bindport_next_dev_flow` integration test. It runs
`npm run --silent dev` in a temporary package whose `dev` script is
`bindport -- next dev`, with a fake `next` executable receiving `PORT` and the
registry recording `next dev`. This proves package-manager script dispatch
without adding a real Next.js dependency to the repository.

`status --json` exposes route-oriented fields (`hostname`, `route_url`,
`health_url`, `health`, `outputs`, and `proxy`). Service config and run options
can populate `hostname`, `route_url`, and `health_url`; rendered output records
populate top-level output summaries and per-service output status. `proxy` is a
compatibility alias for recorded `traefik` output status. Health probes are
limited to loopback `http://` destinations; non-loopback and unsupported
destinations remain `unknown`.

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

The v0.5 service-command/hooks/agent UX gate is covered by integration tests for
configured service commands and arguments, status/open behavior, hook trust and
dry-run flows, stale cleanup confirmation, dashboard parity, and health status.
Manual acceptance should still verify a real local project with a tool that uses
CLI port flags, such as Storybook.

## Local Release Checks

Run the standard local gate before any release prep branch:

```sh
mise run ci
```

If the local shell refuses to load this repo's trusted `mise.toml`, run:

```sh
MISE_TRUSTED_CONFIG_PATHS="$PWD" mise run ci
```

## Local Staged Release Smoke

Build and exercise the release candidate without publishing or installing into
user-owned package-manager state:

```sh
MISE_TRUSTED_CONFIG_PATHS="$PWD" mise run release-smoke
```

The dependency-light xtask entry point:

- checks the `cargo install bindport` source package file list without reaching
  crates.io;
- validates all four cargo-binstall URLs, npm native package mappings, raw
  GitHub binary names, checksum sidecars, completion/man archives, and generated
  Homebrew formula URLs;
- reuses the canonical npm, completion/man, binstall, and formula helpers;
- packages the real host release binary into a native npm tarball, installs it
  with the wrapper offline under a temporary `HOME`, and executes it there;
- runs version/help, idempotent `reserve --all`, exact `port`, web-before-api
  configured startup, sibling env wiring, service cwd, JSON output render,
  dashboard start/status/health/stop, and stopped/stale cleanup; and
- uses a canonical temporary project, isolated XDG/registry/npm paths,
  test-owned ports, bounded waits, process cleanup guards, and automatic temp
  removal.

Only the current host's native package is executable. The other three targets
are metadata- and artifact-name checks. The smoke never contacts GitHub
Releases, crates.io, npm, mise/ubi, or the external Homebrew tap, and it does not
claim those live channels are published. Linux and macOS run this entry point in
ordinary CI.

## Release Prep PR

Create a release prep pull request with:

```sh
mise run release-prep
mise run release-prep patch
mise run release-prep minor
mise run release-prep major
VERSION=X.Y.Z
mise run release-prep "v${VERSION}"
mise run release-prep "${VERSION}"
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
- regenerates `CHANGELOG.md` for the target version with `git-cliff`;
- refreshes and validates Cargo metadata;
- runs `scripts/release-check.sh --version X.Y.Z --allow-dirty`;
- stages `Cargo.toml`, `Cargo.lock`, `CHANGELOG.md`, and npm package metadata;
- commits `build: prepare vX.Y.Z release`;
- pushes the branch to `origin`;
- opens a pull request with `gh pr create`.

The script intentionally stops before publishing release metadata. The release
workflow is dispatched only after the release prep PR has been reviewed and
merged.

## Release Check Gate

Release checks are intentionally non-publishing by default. They validate the
version, verify that `CHANGELOG.md` is current, run the local CI gate, dry-run
Cargo package contents, validate all npm package versions and optional
dependencies, and dry-run wrapper/platform npm package tarballs. Full crates.io
publish dry-runs are reserved for the
`--publish-ready` gate because Cargo publishing remains a separate manual
action.

Run it locally after a release-prep branch updates versions and package
artifacts:

```sh
VERSION=X.Y.Z
mise run release-check "${VERSION}"
```

Or call the script directly:

```sh
scripts/release-check.sh --version "${VERSION}"
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
VERSION=X.Y.Z
mise run release-finish "v${VERSION}"
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
mise run release-finish --yes "v${VERSION}"
mise run release-finish --skip-github-release "v${VERSION}"
mise run release-finish --skip-cargo-publish "v${VERSION}"
```

`release-finish` waits for the manual `Release` workflow. If the
`stable-release` environment requires approval, approve the workflow in GitHub;
the local command continues polling until the workflow completes or times out.

The lower-level release dispatch command remains available:

```sh
mise run release-publish --dry-run "v${VERSION}"
mise run release-publish "v${VERSION}"
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
creates the GitHub Release. If a GitHub Release already exists for the tag, the
workflow refuses to mutate existing release assets.

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
- `bindport-linux-x64-X.Y.Z.tgz.sha256`
- `bindport-linux-arm64-X.Y.Z.tgz`
- `bindport-linux-arm64-X.Y.Z.tgz.sha256`
- `bindport-darwin-x64-X.Y.Z.tgz`
- `bindport-darwin-x64-X.Y.Z.tgz.sha256`
- `bindport-darwin-arm64-X.Y.Z.tgz`
- `bindport-darwin-arm64-X.Y.Z.tgz.sha256`
- `bindport-X.Y.Z.tgz`
- `bindport-X.Y.Z.tgz.sha256`
- `bindport-completions.tar.gz`
- `bindport-completions.tar.gz.sha256`
- `bindport-manpage.tar.gz`
- `bindport-manpage.tar.gz.sha256`

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

Users with `cargo-binstall` can install the matching GitHub Release binary:

```sh
cargo binstall bindport
```

The `bindport` crate publishes `[package.metadata.binstall]` with exact
Linux/macOS x64/arm64 release asset URLs. Release checks validate that metadata
so `cargo binstall bindport` resolves the same raw binaries uploaded by the
GitHub Release workflow.

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
VERSION=X.Y.Z
mise run cargo-publish "${VERSION}"
```

Or call the script directly:

```sh
scripts/cargo-publish.sh --version "${VERSION}" --dry-run
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
mise run cargo-publish --execute "${VERSION}"
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

## Homebrew Tap

The Homebrew tap formula is generated after the GitHub Release exists because
the formula must point at the reviewed release assets and their `.sha256`
checksums. The tap repository is external to this source tree; do not create or
mutate it from the BindPort release workflow.

After `release-finish` creates the GitHub Release, download the checksum assets
and generate the formula:

```sh
VERSION=X.Y.Z
mkdir -p dist/homebrew
gh release download "v${VERSION}" --repo bindport/bindport --pattern '*.sha256' --dir dist/homebrew
mise run homebrew-formula "${VERSION}" --dist dist/homebrew --output ../homebrew-tap/Formula/bindport.rb
```

Review the generated formula in the tap checkout. It should install the
matching Linux/macOS x64/arm64 binary, bash/zsh/fish completions from
`bindport-completions.tar.gz`, and the `bindport.1` man page from
`bindport-manpage.tar.gz`.

Before opening the tap PR, run the formula check from this repository and the
tap's Homebrew checks:

```sh
mise run homebrew-formula-check
brew update
brew install --build-from-source ../homebrew-tap/Formula/bindport.rb
bindport --version
bindport doctor
```

Once the tap PR merges, verify the public install path:

```sh
brew install bindport/tap/bindport
```

## v0.2.0 Manual Acceptance

Before merging the v0.2.0 release prep PR, smoke test the release branch from a
fresh checkout or clean worktree:

1. Build the release binary with `cargo build --release --locked`.
2. Run a wrapped command and confirm the child receives `PORT`.
3. Confirm `bindport status --json` reports the service and run history.
4. Start `bindport dashboard serve` and verify `/api/status` matches
   `bindport status --json`.
5. Verify the dashboard shows active, stopped, and stale groups as applicable.
6. Verify URL copy/open actions when `hostname` and `route_url` are configured,
   and confirm health is `unknown`, `pending`, `healthy`, or `failing` as
   expected for the service loopback health URL.
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
   service hostname such as `hostname = "{branch}.example-web.localhost"`.
3. Run a wrapped command with that service and confirm BindPort writes a
   generated Traefik file under the configured output root.
4. Point an existing Traefik file provider at the generated directory with
   `watch = true`, then confirm Traefik reloads the generated file.
5. Visit a rendered hostname such as
   `feature-tree.example-web.localhost` and confirm it forwards to the
   correct wrapped process.
6. Repeat with a second project name such as `example-api` to confirm the hostname
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

## v0.6.0 Manual Acceptance

Before merging the v0.6.0 release prep PR, smoke test adoption, install
metadata, service commands, hooks, health, cleanup, and agent-facing
status/open behavior from a fresh checkout or clean worktree:

1. Build the release binary with `cargo build --release --locked`.
2. Create a project config with one service command that passes the assigned
   port as a CLI argument, such as `--port {port}`, and one env template such as
   `NEXT_PUBLIC_BINDPORT_URL = "{route_url}"`.
3. Run the configured service with `bindport run <service>` and confirm the
   child process receives both the assigned port argument and rendered env.
4. Run `bindport status --json` and confirm the payload matches
   [status.schema.json](../status.schema.json), including `services`, `runs`,
   `hooks`, route metadata, health, outputs, and proxy fields.
5. Run `bindport open <service> --print` and confirm it prints `route_url` when
   configured, otherwise the direct loopback URL.
6. Run `bindport open <service> --browser` against an HTTP or HTTPS URL and
   confirm non-HTTP(S) route URLs are rejected before launching a browser.
7. Configure a local hook and run `bindport hooks status`; confirm the hook is
   pending until trusted and appears in `status --json` and the dashboard hooks
   view.
8. Run `bindport hooks trust <name>`, rerun the service, and confirm the trusted
   hook runs only for its configured lifecycle events. Change the hook command or
   target and confirm the status changes to `changed` until retrusted.
9. Run `bindport render --dry-run` with hooks configured and confirm hook
   dry-run output is reported without executing the hook command.
10. Configure a loopback HTTP health URL, run the service, and confirm
    `status --json` and the dashboard move through expected `pending`,
    `healthy`, or `failing` states. Confirm unsupported or non-loopback health
    URLs remain `unknown`.
11. Create stopped and stale registry entries, run
    `bindport clean --dry-run --json`, and confirm the report counts entries
    without removing them.
12. Run stale cleanup without `--yes` in a non-interactive context and confirm it
    is rejected. Rerun with `--yes` and confirm only the intended stale entries
    are removed.
13. Run `bindport doctor` and `bindport doctor outputs` and confirm service
    identity, route/output diagnostics, hook trust visibility, and obvious port
    conflicts are reported.
14. Start `bindport dashboard serve`, confirm `/api/status` matches CLI status,
    and verify dashboard cleanup actions remain blocked for active services.
15. Run `bindport init` in an empty temp project and confirm it creates a
    commit-safe `.bindport.toml` without machine-specific absolute paths.
16. Run `bindport reserve <service>`, confirm `status --json` reports a
    `reserved` service, then run `bindport release <service>` and confirm the
    lease becomes stopped.
17. Run `cargo binstall --dry-run bindport` or inspect package metadata and
    confirm it resolves Linux/macOS x64/arm64 GitHub Release assets.
18. Confirm the release artifacts include bash/zsh/fish completions and
    `bindport.1`, and that the Homebrew formula generator can point at the
    reviewed checksummed release assets.
19. Run `mise run ci` on the release branch before requesting review.

## v0.7.0 Manual Acceptance

Before merging the v0.7.0 release prep PR, smoke test output integrations and
diagnostics from a fresh checkout or clean worktree:

1. Build the release binary with `cargo build --release --locked`.
2. Run `bindport status --json` and confirm `schema_version` is `0.7` and the
   payload matches [status.schema.json](../status.schema.json).
3. Start `bindport dashboard serve` and confirm `/api/status` returns the same
   schema version and payload shape as `bindport status --json`.
4. Run `bindport templates list` and confirm `bindport-caddy`,
   `bindport-haproxy`, `bindport-json-snapshot`, `bindport-nginx`,
   `bindport-traefik`, and `bindport-env-local` appear.
5. Configure Caddy, nginx, HAProxy, and JSON snapshot outputs, run
   `bindport doctor outputs`, and confirm all templates resolve without writing
   files.
6. Run a service with route metadata, then run `bindport render --dry-run`,
   `bindport render --diff`, and `bindport render`; confirm dry-run and diff do
   not write files, while render writes DB-owned output files.
7. Run `bindport status --json` and confirm top-level output summaries,
   per-service output state, and the legacy `proxy` alias for Traefik output
   are present when relevant.
8. Run `bindport list` and `bindport list --json`; confirm registry-wide
   grouping includes services from more than one project or config root when
   present.
9. Run `bindport registry export` and confirm output ownership scope fields are
   present, including output root, scope, config root, and worktree context when
   available.
10. Render the same output name from two worktrees or temp config roots into
    separate generated directories and confirm neither worktree overwrites the
    other's ownership rows.
11. Delete or move a generated output root, then run `bindport doctor outputs`
    and confirm stale or foreign ownership rows are reported as diagnostics
    without blocking current-scope rendering.
12. Set `BINDPORT_LOG=debug` before `bindport run <service>` and confirm render
    diagnostics identify selected outputs, roots, route counts, ownership rows,
    and lifecycle cleanup without printing child env or hook payloads.
13. Run `mise run ci` on the release branch before requesting review.

## v0.8.0 Local Staged Acceptance

The required Linux/macOS release acceptance is now the automated
`mise run release-smoke` entry point described above. Run it at least twice on
the release checkout to catch leaked dashboard processes, reused registry
state, fixed-port assumptions, or incomplete temporary cleanup. A pass verifies
local source/package shape and host-compatible staged artifacts only. It is not
evidence that any public channel contains the version.

## Authorized `v1.0.0-rc.1` Live-Channel Checklist

Do **not** run this checklist without separate explicit authorization. Tags,
GitHub Releases, crates.io versions, and npm versions are external mutations;
crate/package versions cannot safely be reused after publication. The Homebrew
tap is another repository and must be changed only from its own reviewed
checkout.

The current release automation intentionally accepts stable `X.Y.Z` only,
creates a normal GitHub Release, publishes npm without a dist-tag override, and
updates only the stable Homebrew formula. Therefore `v1.0.0-rc.1` must not be
passed to the current scripts as if it were a stable release. Before the live
exercise, land a separate reviewed change that:

- accepts SemVer prereleases consistently in release-prep, release-check,
  release-publish, release-finish, Cargo/npm publish helpers, and workflows;
- creates `v1.0.0-rc.1` as a GitHub prerelease;
- publishes all five npm packages with `--tag next`, never `latest`; and
- defines an owner-approved RC tap strategy without replacing the stable
  `bindport` formula (for example, a reviewed temporary `bindport-rc` formula).

After that prerequisite is merged, the authorized operator performs and records
these steps in order:

1. Confirm the clean, synced `main` checkout contains the reviewed
   `1.0.0-rc.1` Cargo/npm metadata and changelog, and confirm all six Cargo
   crate names plus all five npm package names are intended to receive this
   irreversible version.
2. Run `mise run release-smoke` twice on Linux and twice on macOS, then run
   `mise run ci`, `mise run docs-build`, and `git diff --check`.
3. Dispatch `release.yml` with `version=1.0.0-rc.1` and `dry_run=true`. Confirm
   all four native build jobs pass and the assembled dry-run artifacts match
   the documented binary, npm, completion, manpage, and checksum names.
4. With a second approval, dispatch the real workflow. Confirm tag
   `v1.0.0-rc.1` points at the reviewed commit, the GitHub Release is marked as
   a prerelease, every `.sha256` verifies, and downloaded Linux/macOS binaries
   report exactly `1.0.0-rc.1`. Exercise x64 and arm64 artifacts on matching
   runners where available.
5. Run the Cargo publish dry-run, then publish the six crates in dependency
   order. In a temporary `CARGO_HOME` and install root, run
   `cargo install bindport --version '=1.0.0-rc.1' --locked` and execute the
   complete v0.8 flow with that installed binary. Separately run
   `cargo binstall bindport --version 1.0.0-rc.1` in an isolated root and repeat
   version/help plus the flow.
6. Run the npm publish workflow dry-run from the reviewed GitHub tarballs.
   Publish the four native packages and wrapper with the `next` tag, verify
   `npm view bindport@next version` is `1.0.0-rc.1`, then install
   `bindport@next` in an empty temporary project on Linux and macOS. Confirm the
   wrapper selects the host native package and run the complete flow.
7. Generate the RC Homebrew formula from the published checksums in a separate
   tap checkout. Review all four URLs and hashes, install the owner-approved RC
   formula on macOS (and Linuxbrew if claimed for the RC), verify completions
   and `man 1 bindport`, and run the complete flow. Do not merge an RC over the
   stable `bindport` formula.
8. In an isolated mise config/data/cache directory, pin
   `"ubi:bindport/bindport" = "1.0.0-rc.1"`, run `mise install`, verify the
   selected GitHub asset and version, and repeat the flow. This is the first
   point at which mise/ubi may be called live-channel verified.
9. For every installed channel, record OS, architecture, exact command/version,
   artifact checksum, and results for version/help, wrapper execution where
   applicable, `reserve --all`, exact `port`, web-before-api startup, sibling
   env wiring, render, dashboard start/status/stop, and stopped/stale cleanup.
10. Announce a channel as verified only after its live install and flow pass.
    Record failures without retagging or attempting to reuse a published Cargo
    or npm version.

## Versioning

- `0.0.x`: unreleased bootstrap only.
- `0.1.0`: first working runner release.
- `0.2.0`: local dashboard API, embedded UI, and stopped/stale cleanup.
- `0.3.0`: template output rendering and built-in Traefik file-provider output.
- `0.4.0`: monorepo config depth, validation, local overrides, and env outputs.
- `0.5.0`: service command config, hooks, health checks, cleanup hardening, and
  agent-facing status/open workflows.
- `0.5.1`: clone-and-run trust-boundary hardening, release artifact checksum
  verification, and generic public examples.
- `0.6.0`: adoption, Homebrew, shell completions, man page, cargo-binstall,
  reserve/release leases, pressure cleanup, and platform hardening.
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

The npm package lives in `npm/bindport`. Keep JavaScript limited to release
tooling; the published runtime wrapper should stay small and platform-native:

- `package.json` declares the `bindport` bin entry.
- `bin/bindport` finds and executes the platform native binary without
  requiring Node at runtime.
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
installed package for the current `uname` OS and architecture. There is no
postinstall download script; release-built platform package tarballs contain the
native binaries.

Before a real npm publish:

1. Run the GitHub `Release` workflow for the version.
2. Confirm the GitHub Release contains all raw binaries, checksums, npm
   tarballs, and npm tarball checksums.
3. Run the `npm Publish` workflow with `execute=false`.
4. Run the `npm Publish` workflow with `execute=true` after the dry-run passes.

Local npm publish dry-run from downloaded release tarballs:

```sh
VERSION=X.Y.Z
gh release download "v${VERSION}" --pattern "*.tgz" --dir dist/npm
gh release download "v${VERSION}" --pattern "*.tgz.sha256" --dir dist/npm
mise run npm-publish "${VERSION}" --dist dist/npm
```

Real local npm publish, when intentionally bypassing the workflow:

```sh
mise run npm-publish "${VERSION}" --dist dist/npm --execute
```

Publish order matters: native platform packages are published first, then the
`bindport` wrapper. The `npm-publish` script enforces that order.

## Homebrew Tap

The Homebrew tap formula should source only reviewed GitHub Release artifacts.
After the `Release` workflow has created the GitHub Release for a stable tag:

1. Update `bindport/homebrew-tap` with the new version and checksums.
2. Point the formula at the matching platform binary artifact.
3. Install shell completions from `bindport-completions.tar.gz`.
4. Install the man page from `bindport-manpage.tar.gz`.
5. Run the tap smoke test:

```sh
brew update
brew install bindport/tap/bindport
bindport --version
bindport doctor
```

Homebrew core submission is deferred until after v1.0.

## Bun / bunx Workflow

`bunx` runs npm package executables declared in `package.json`'s `bin` field.
Because this package declares:

```json
{
  "bin": {
    "bindport": "bin/bindport"
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

The npm bin is a POSIX shell wrapper on Linux and macOS. It exists only to find
the matching native package and `exec` the Rust binary, so package-manager Node
shims are not part of normal command execution.

For committed project scripts, prefer installing BindPort as a development
dependency and calling the local executable instead of auto-installing on every
run:

```json
{
  "scripts": {
    "dev": "bindport -- next dev"
  },
  "devDependencies": {
    "bindport": "X.Y.Z"
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
- The `Release` workflow uploads shell completion and manpage tarballs plus
  checksums to GitHub Releases.
- The `Cargo Publish` workflow may publish crates.io packages only when manually
  dispatched with `execute=true` and approved through the `crates-io`
  environment.
- The `npm Publish` workflow may publish npm packages only when manually
  dispatched with `execute=true` and approved through the `npm` environment.
- No workflow commits version bumps back to the repository.
- Keep automatic package publishing disabled until the manual workflows have
  shipped cleanly.
