# Platform And MSRV Support

BindPort supports local development on Linux and macOS through v1. Windows is
post-v1 and is not a supported build, install, CI, or runtime target.

## Rust Version Policy

The supported Rust build floor (MSRV policy) is **Rust 1.96.0**. That is the
version pinned by `mise.toml`, used by Linux CI through the locked mise toolset,
installed explicitly by macOS CI, and used for all release artifact builds.
Release and compatibility changes must continue to compile and test on 1.96.0
until an announced MSRV increase.

The workspace currently uses Rust edition 2024 but does not set Cargo's
`package.rust-version`; `cargo metadata` therefore reports no enforced
`rust_version`. Cargo may attempt a build with an older compiler, but such a
build is outside the support contract even if it happens to succeed. This page
and the pinned CI/release toolchains are the current policy authority.

## Operating Systems, Architectures, And Distributions

| Area | Support contract |
|---|---|
| Runtime operating systems | Linux and macOS |
| Prebuilt architectures | x86_64 (`x64`) and AArch64 (`arm64`) |
| GitHub Release binaries | `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `x86_64-apple-darwin`, and `aarch64-apple-darwin` |
| npm native packages | Linux/macOS x64/arm64, selected by a POSIX shell wrapper |
| cargo-binstall, Homebrew, mise/ubi | The same four GitHub Release binary targets |
| Cargo install | Source build on a supported Linux or macOS host using Rust 1.96.0 or newer |
| Windows and other operating systems | Unsupported through v1 |

Architecture support for prebuilt channels is narrower than source-build
possibility. Cargo may compile on another Linux/macOS architecture, but without
a corresponding CI runner or release asset it is not a supported architecture.

Linux release binaries use the GNU target and are built on Ubuntu 24.04 x64 and
arm64 runners. Ubuntu is the full Linux CI environment. Other glibc-based Linux
distributions may run those binaries when their runtime ABI is compatible, but
BindPort does not publish a per-distribution compatibility matrix. No musl
binary, static-Linux guarantee, container image, or Windows asset is currently
published. A source build on an untested distribution is not equivalent to a
release-asset support guarantee.

macOS release assets are built separately on macOS 15 Intel and arm64 runners.
The regular macOS compatibility job runs clippy, all-target tests, a locked
release build, and the npm wrapper smoke test. BindPort does not currently
publish a minimum macOS release number beyond what those build artifacts and
runners support.

For channel-specific installation commands, use
[Install BindPort](../getting-started/install.md). This page defines support
boundaries rather than duplicating release procedures.

## Shell And External Command Requirements

The native Rust binary does not require a shell for normal subcommands.
`bindport -- <command>`, configured service commands, and hooks execute
structured argv directly. Shell syntax works only when the user explicitly
runs a shell such as `sh -c`.

The npm launcher is a POSIX `sh` script. It uses common Unix tools including
`uname`, `dirname`, and `readlink`, then executes the matching native package.
The npm distribution is therefore not a native Windows launcher. Project
scripts and documentation examples that use `sh` also require a compatible
shell, even when the underlying BindPort command itself does not.

Optional platform commands are:

- `git` for worktree, branch, and package identity enrichment; BindPort falls
  back when git metadata is unavailable;
- `ps` on macOS for best-effort command-line verification; and
- `open` on macOS or `xdg-open` on Linux for the explicit `open --browser`
  action.

## Process, Signal, And Port Differences

BindPort probes TCP loopback on IPv4 and IPv6, then releases the probe listener
before starting a child. Reservations are SQLite coordination and likewise do
not retain an OS listener. Another process can claim the port during that gap.
An ordinary allocation can retry once after an immediate occupied-port startup
failure; an unavailable reserved port fails without silently renumbering it.
UDP availability is not checked.

Linux and macOS are Unix platforms for runner purposes. The wrapper forwards
SIGINT and SIGTERM to its child. A normal child exit is passed through; a child
terminated by a signal is represented as the conventional `128 + signal`
numeric status. See [CLI Stability Contract](cli-stability.md) for the exact
exit contract.

Linux has the strongest PID-reuse checks. New run records capture process start
time from `/proc` and compare it during stale reconciliation; older records
without that value use PID liveness plus command-line inspection. macOS has no
Linux-style `/proc` start-time value; BindPort combines PID liveness with a
best-effort command-line query through `ps`. If command inspection is
unavailable, some stale checks conservatively fall back to PID liveness, so PID
reuse can be misclassified until manual cleanup. `doctor` reports registry and
listener conflicts; it does not promise full process ownership attribution.

Background dashboard stop uses SIGTERM only after matching the recorded state.
Linux can compare both start time and command shape. macOS can inspect command
shape through `ps` when available but cannot compare the Linux start-time
field, so stale PID reuse has weaker protection.

## Filesystem And Path Assumptions

Config discovery walks ancestors of the current directory. Project config,
local override config, custom templates, and generated text are expected to be
UTF-8. CLI arguments use Rust's Unicode argument interface. Arbitrary non-UTF-8
path and argument round-tripping is not a supported contract for JSON, status,
or registry display fields.

Service paths and project-config output roots are relative to the discovered
config root and may not contain `..`; fallback-config outputs use the invoking
cwd as their base. Configured service paths are canonicalized and must remain
under their config root. Output targets must remain under the resolved output
root and may not traverse symlink components.

Generated outputs use a temporary sibling plus `rename`; atomic replacement is
therefore expected only within the same filesystem. Registry state uses SQLite
WAL files beside the database. On Unix, the registry directory/database created
by BindPort are hardened to `0700`/`0600`; output temp files are created as
`0600`.
See [Security and Privacy](../operations/security.md) for ownership and local
state limits.

Default paths are:

- project config: `.bindport.toml`, `.bindport.json`, or `.bindport.yaml`,
  discovered upward;
- user fallback config: `$XDG_CONFIG_HOME/bindport/config.toml` or
  `~/.config/bindport/config.toml`;
- registry: `$XDG_STATE_HOME/bindport/registry.sqlite` or
  `~/.local/state/bindport/registry.sqlite`; and
- dashboard state/log and hook trust: the same BindPort state directory.

## Verification Coverage

Linux CI runs format, clippy, tests, coverage, a platform-cfg guard, release
build, dependency/security checks, CLI asset checks, npm smoke, and docs build.
macOS CI runs clippy, all-target tests, a locked release build, and npm smoke.
The release matrix builds and packages all four prebuilt targets listed above
with Rust 1.96.0.

The platform-cfg guard catches Linux-only conditional-compilation shapes that a
Linux clippy run can miss but macOS would report as unused. There is no Windows,
musl, BSD, or other Unix CI job, and no compatibility promise for those targets.
