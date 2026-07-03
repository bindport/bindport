# Platform Support

BindPort currently supports Linux and macOS local development. Windows is
post-1.0 and is not a supported install target yet.

## Supported Targets

| Area | Supported |
|---|---|
| Operating systems | Linux, macOS |
| Architectures | x64, arm64 |
| Cargo install | Linux/macOS through `cargo install bindport` |
| npm install | `bindport` wrapper plus Linux/macOS native packages |
| GitHub Release assets | Linux/macOS raw binaries and npm tarballs |
| CI compatibility | Linux full CI, macOS compatibility build/test |
| Windows | Post-1.0; not supported yet |

## Paths

Project config is discovered from `.bindport.toml`, `.bindport.json`, or
`.bindport.yaml` by walking upward from the current directory. A matching
`.bindport.local.*` or `bindport.local.*` file in the same directory provides
machine-local overrides and should stay untracked.

User fallback config uses:

- `$XDG_CONFIG_HOME/bindport/config.toml`
- `~/.config/bindport/config.toml` when `XDG_CONFIG_HOME` is unset

State uses:

- `$XDG_STATE_HOME/bindport/registry.sqlite`
- `~/.local/state/bindport/registry.sqlite` when `XDG_STATE_HOME` is unset

Dashboard service state, dashboard logs, and hook trust state live under the
same BindPort state directory.

## Process And Port Behavior

BindPort probes TCP loopback on IPv4 and IPv6, then starts the child process with
the assigned `PORT`. The probe listener is released before the child starts, so
BindPort retries once when the child fails immediately and the assigned port is
then occupied.

On Unix platforms, the runner forwards SIGINT and SIGTERM to the wrapped child
and exits with the child's status code. Dashboard service controls use SIGTERM
for `bindport dashboard stop`.

Linux has the strongest PID-reuse protection today because BindPort can compare
recorded process start time and command line through `/proc`. macOS support uses
PID liveness checks without `/proc` command-line verification, so stale
dashboard state after PID reuse may require manual cleanup.

Output files are written with sibling temp files and `rename`, so generated
template output is atomic on the same filesystem. On Unix platforms, registry
directories and files are permission-hardened when BindPort creates them.

## Verification Gates

Local:

```sh
mise run ci
```

The local CI task includes format, clippy, platform cfg guard, workflow lint,
audit, dependency guard, secret scan, security scan, tests, coverage, release
build, and npm wrapper smoke tests.

GitHub Actions:

- Linux runs the full CI gate, including the platform cfg guard and npm wrapper
  smoke test.
- macOS runs clippy, tests, and release build as a compatibility gate.
- Release builds Linux/macOS x64/arm64 binaries and matching npm tarballs.

The platform cfg guard catches Linux-only `#[cfg(target_os = "linux")]` patterns
that Linux clippy can miss but macOS clippy reports as unused code.
