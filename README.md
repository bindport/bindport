# BindPort

BindPort is a proxy-neutral local development port registry, allocator, and
runner. It is meant to wrap development commands, assign a stable local port,
record which project/service/worktree owns that port, and eventually emit config
fragments for an existing proxy such as Traefik.

BindPort does not aim to become the local edge proxy. By default it should not
bind 80/443, install certificates, mutate DNS, edit `/etc/hosts`, or require a
root-owned daemon.

## Current Status

BindPort v0.1.0 is available through Cargo:

```sh
cargo install bindport
bindport --help
bindport -- doctor
bindport -- -- sh -c 'echo "$PORT"'
```

The current release includes:

- Rust Cargo workspace with `bindport` plus core, registry, runner, and adapter
  crates.
- Minimal CLI support for `--help`, `--version`, `status`, `doctor`, and
  one-shot `bindport -- <command>` command wrapping.
- Optional config discovery from `.bindport.toml`, `.bindport.json`, or
  `.bindport.yaml`, with a fallback user config in the XDG config directory.
- Basic project/service identity resolution from environment, config,
  `package.json`, command inference, and `bindport run <service> -- ...`, with
  git branch/worktree metadata recorded when available.
- Service env templates for wrapped commands through `[[services]].env` and
  `bindport run --env`, including route hostname metadata when configured.
- Basic SQLite-backed lease/run recording with `bindport status --json`.
- `bindport doctor` diagnostics for config, registry, effective identity,
  active registry ports, OS listener conflicts, and the next candidate port.
- `bindport clean` registry cleanup for stopped and stale entries, with dry-run
  and JSON output options.
- MIT license, security policy, third-party notices stub, CI/audit/release
  workflows, and local `mise` tasks.
- npm wrapper package skeleton. It is not published yet because native binary
  dispatch is not wired.
- Example `.bindport.toml`, `.bindport.json`, and `.bindport.yaml` files.

The v0.1 support target is Linux and macOS-style local development. Windows is
not claimed as supported until the cross-platform hardening milestone.

The v0.1 runner is available from Cargo or from a source checkout:

```sh
bindport -- next dev
cargo run -p bindport -- -- next dev
```

It probes TCP loopback (IPv4 and IPv6) for a currently-free port from
`29000-29999`, prefers the previous port for the same project/service/worktree
identity when it is still free, otherwise scans from a stable identity-based
offset, injects `PORT=<assigned>`, inherits child stdio, forwards Unix
SIGINT/SIGTERM to the child, and exits with the child process exit code. This
v0.1 runner is probe-then-spawn, so another process can still claim the port
before the child binds. BindPort retries once with another port when the child
fails immediately and the assigned port is then occupied; stronger lease-based
coordination is still future v0.1 work.

From a source checkout, use Cargo directly:

```sh
cargo run -p bindport -- --help
cargo run -p bindport -- doctor
cargo run -p bindport -- init
cargo run -p bindport -- status --json
cargo run -p bindport -- clean --dry-run
cargo run -p bindport -- dashboard serve
cargo run -p bindport -- run web -- sh -c 'echo "$PORT"'
cargo run -p bindport -- run web --env NEXT_PUBLIC_BINDPORT_URL='{route_url}' --hostname '{branch}.{project}.localhost' -- sh -c 'echo "$NEXT_PUBLIC_BINDPORT_URL"'
cargo run -p bindport -- -- sh -c 'echo "$PORT"'
```

## Project Commands

```sh
cargo check --all-targets
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
cargo build --release --locked
```

The same local checks are available through `mise`:

```sh
mise install --locked
mise run check
mise run ci
mise run dev-dashboard
```

## Configuration Examples

Starter config examples live in [examples/config](examples/config):

- [`.bindport.toml`](examples/config/.bindport.toml)
- [`.bindport.json`](examples/config/.bindport.json)
- [`.bindport.yaml`](examples/config/.bindport.yaml)

TOML is the reference format. When equivalent config files exist, discovery
prefers TOML, then JSON, then YAML. BindPort walks upward from the current
directory and uses the first project config it finds. If no project config
exists, it falls back to the optional user config at
`$XDG_CONFIG_HOME/bindport/config.toml`, or `~/.config/bindport/config.toml`
when `XDG_CONFIG_HOME` is unset. The registry database remains state at
`$XDG_STATE_HOME/bindport/registry.sqlite`, or
`~/.local/state/bindport/registry.sqlite` when `XDG_STATE_HOME` is unset.
`bindport init` creates the optional user config with default values. Config is
never required; missing config means built-in defaults are used.

The current implementation reads top-level `project`, `service`,
`default_range`, `skip_ports`, `[[services]]` entries, and the `[dashboard]` /
`[dashboard.auth]` settings used by the local dashboard. Dashboard defaults are
local-only (`127.0.0.1:27080`) with auth disabled; non-loopback dashboard binds
require auth and a token. Set `dashboard.register_service = true` or pass
`bindport dashboard --register-service` when you want the dashboard itself to
appear in `bindport status`. Service entries currently apply `name`, `env`,
`hostname`, and `route_url`. The example `identity`, `proxy`, and deeper
service fields such as `command` and `health_url` document the intended future
shape and are not applied yet; `bindport doctor` reports ignored top-level keys
so typos and future-only sections are visible.

Identity precedence is intentionally narrow during bootstrap: the optional
service argument in `bindport run <service> -- ...` wins, then
`BINDPORT_PROJECT` / `BINDPORT_SERVICE`, then config, then inference from
`package.json`, the git worktree path, and command name.

Wrapped commands always receive `PORT=<assigned>`. Service env templates can
add more variables:

```toml
[[services]]
name = "web"
hostname = "{branch}.{project}.localhost"
env.PORT = "{port}"
env.HOSTNAME = "0.0.0.0"
env.NEXT_PUBLIC_BINDPORT_URL = "{route_url}"
```

Supported template placeholders are `{port}`, `{host}`, `{url}`, `{project}`,
`{service}`, `{hostname}`, `{route_url}`, `{branch}`, `{branch_label}`,
`{git_branch}`, `{worktree}`, `{worktree_label}`, and `{worktree_hash}`.
Use `{{` and `}}` when a template value needs literal braces, for example a
JSON-valued environment variable.
`bindport run --env NAME=VALUE`, `--hostname TEMPLATE`, and
`--route-url TEMPLATE` override service config for a single run.
`BINDPORT_HOSTNAME` and `BINDPORT_ROUTE_URL` can also override the matching
service config values for wrapper scripts.

## Registry Cleanup

Stopped and stale entries can be removed from the local registry with:

```sh
bindport clean --dry-run
bindport clean
```

`bindport clean` removes stopped and stale entries by default. Use `--stopped`
or `--stale` to scope cleanup, and `--json` for machine-readable counts. Active
services are not removed.

## Documentation

- [Dashboard](docs/dashboard.md): local service dashboard, status API, scoped
  registry cleanup actions, service-style controls, configurable bind/auth
  options, dev modes, and security posture.
- [Release](docs/release.md): release prep automation, GitHub release binaries,
  Cargo publish helpers, and npm packaging boundaries.

## License

BindPort is licensed under the MIT License. See [LICENSE](./LICENSE).

## Commit Messages

Use Conventional Commit-style subjects:

```text
<type>: <imperative summary>
```

Common prefixes:

- `docs`: documentation and repo guidance
- `feat`: user-facing features
- `fix`: bug fixes
- `ci`: CI and release automation
- `build`: build system, packaging, and dependency tooling
- `deps`: dependency updates
- `test`: tests and test infrastructure
- `refactor`: behavior-preserving code changes
