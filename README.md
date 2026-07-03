# BindPort

BindPort is a proxy-neutral local development port registry, allocator, and
runner. It is meant to wrap development commands, assign a stable local port,
record which project/service/worktree owns that port, and emit config fragments
for an existing proxy such as Traefik.

BindPort does not aim to become the local edge proxy. By default it should not
bind 80/443, install certificates, mutate DNS, edit `/etc/hosts`, or require a
root-owned daemon.

## Current Status

BindPort v0.5.1 is the current release candidate. Once published, it installs
the same Rust CLI through Cargo or JavaScript package managers.

Install globally with Cargo:

```sh
cargo install bindport
```

Or install the matching GitHub Release binary with `cargo binstall`:

```sh
cargo binstall bindport
```

Or install it as a JavaScript project dependency:

```sh
npm install --save-dev bindport
```

Then run the CLI directly or from project scripts:

```sh
bindport --help
bindport -- doctor
bindport dashboard serve
bindport -- -- sh -c 'echo "$PORT"'
```

The current source tree includes:

- Rust Cargo workspace with `bindport` plus core, registry, runner, and adapter
  crates.
- CLI support for `--help`, `--version`, `status`, `open`, `config explain`,
  `config validate`, `doctor`, `clean`, `dashboard`, `reserve`, `release`, and
  one-shot `bindport -- <command>` command wrapping.
- Optional config discovery from `.bindport.toml`, `.bindport.json`, or
  `.bindport.yaml`, with a fallback user config in the XDG config directory.
- Basic project/service identity resolution from environment, config,
  package-manager workspaces, `package.json`, command inference, and
  `bindport run <service> -- ...`, with git branch/worktree metadata recorded
  when available.
- Service command, argument, and env templates through `[[services]].command`,
  `[[services]].args`, `[[services]].env`, and `bindport run --env`, including
  route hostname metadata when configured.
- Basic SQLite-backed lease/run/output recording with `bindport status --json`,
  including reserved leases for processes BindPort does not wrap.
- `bindport doctor` diagnostics for config, registry, effective identity,
  active registry ports, known registry listener conflicts, unknown OS listener
  conflicts, and the next candidate port.
- `bindport doctor outputs` diagnostics for configured output templates and
  planned render paths without writing files.
- Built-in `bindport-env-local` output template for explicit `.env.local`
  generation through the normal owned-output pipeline.
- `bindport clean` registry cleanup for stopped and stale entries, with dry-run
  and JSON output options.
- Local dashboard API and embedded UI for active, stopped, and stale registry
  entries, including URL copy/open actions, optional token auth, scoped
  stopped/stale cleanup, and service-style `start` / `status` / `stop`
  controls.
- MIT license, security policy, third-party notices stub, CI/audit/release
  workflows, and local `mise` tasks.
- npm wrapper plus Linux/macOS x64/arm64 platform packages for installing
  native binaries through JavaScript package managers.
- Release artifacts for bash, zsh, and fish shell completions plus a
  `bindport.1` man page.
- Example `.bindport.toml`, `.bindport.json`, and `.bindport.yaml` files.

The current support target is Linux and macOS-style local development. Windows
is post-1.0 and is not claimed as supported yet. See
[Platform Support](docs/platform-support.md) for the supported OS, package,
path, and process behavior.

The runner and dashboard are available from Cargo, npm project scripts, or a
source checkout:

```sh
bindport -- next dev
bindport dashboard serve
npm exec -- bindport --help
cargo run -p bindport -- -- next dev
cargo run -p bindport -- dashboard serve
```

It probes TCP loopback (IPv4 and IPv6) for a currently-free port from
`29000-29999`, prefers the previous port for the same project/service/worktree
identity when it is still free, otherwise scans from a stable identity-based
offset, injects `PORT=<assigned>`, inherits child stdio, forwards Unix
SIGINT/SIGTERM to the child, and exits with the child process exit code. The
runner is probe-then-spawn, so another process can still claim the port
before the child binds. BindPort retries once with another port when the child
fails immediately and the assigned port is then occupied; stronger lease-based
coordination remains future work.

From a source checkout, use Cargo directly:

```sh
cargo run -p bindport -- --help
cargo run -p bindport -- doctor
cargo run -p bindport -- init
cargo run -p bindport -- init --user
cargo run -p bindport -- status --json
cargo run -p bindport -- open web --print
cargo run -p bindport -- clean --dry-run
cargo run -p bindport -- reserve web
cargo run -p bindport -- release web
cargo run -p bindport -- dashboard serve
cargo run -p bindport -- doctor outputs
cargo run -p bindport -- templates list
cargo run -p bindport -- templates export bindport-traefik
cargo run -p bindport -- render --dry-run
cargo run -p bindport -- render --repair
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

Starter config examples live in [examples/config](examples/config), and a
multi-service monorepo fixture lives in [examples/monorepo](examples/monorepo):

- [`.bindport.toml`](examples/config/.bindport.toml)
- [`.bindport.json`](examples/config/.bindport.json)
- [`.bindport.yaml`](examples/config/.bindport.yaml)
- [monorepo `.bindport.toml`](examples/monorepo/.bindport.toml)

TOML is the reference format. When equivalent config files exist, discovery
prefers TOML, then JSON, then YAML. BindPort walks upward from the current
directory and uses the first project config it finds. A matching
`.bindport.local.*` or `bindport.local.*` file in the same directory can provide
machine-local overrides and should stay untracked. If no project config exists,
it falls back to the
optional user config at
`$XDG_CONFIG_HOME/bindport/config.toml`, or `~/.config/bindport/config.toml`
when `XDG_CONFIG_HOME` is unset. The registry database remains state at
`$XDG_STATE_HOME/bindport/registry.sqlite`, or
`~/.local/state/bindport/registry.sqlite` when `XDG_STATE_HOME` is unset.
`bindport init` creates a minimal `.bindport.toml` project config in the current
directory. `bindport init --user` creates the optional user config with default
values. Config is never required; missing config means built-in defaults are
used.
Use `bindport config explain` to inspect the discovered config file, local
override file, effective config-field sources, and the project/service identity
source for the current directory. Use `bindport config validate` to check
service names, service paths, output configuration, and hook configuration
before running a wrapped command.

The current implementation reads top-level `project`, `service`,
`default_range`, `skip_ports`, `[[services]]` entries, `[dashboard]` /
`[dashboard.auth]`, `output_defaults`, `[[outputs]]`, and `[hooks]`. Output
configuration is used by `bindport render` to write text output files from the
current registry snapshot. `bindport doctor outputs` validates output config,
template lookup, planned render paths, and hook trust status without writing
files. Template lookup, listing, showing, and export are available through
`bindport templates`. Wrapped command start/exit events auto-render outputs
when `auto_render = true`;
`debounce_ms` spaces automatic renders, `on_failure = "block"` validates
required outputs before child startup, `delete_on` can remove DB-owned output
files for stopped/stale/removed routes, and CLI or dashboard cleanup triggers
removed-route output cleanup. Hooks can subscribe to the same lifecycle events,
but checked-in project config cannot enable hook execution by itself. Approve
or deny configured hooks per machine with `bindport hooks trust|deny|reset`.
`bindport render --repair` reconciles DB-owned files without adopting unknown
files.
`bindport reserve [service]` allocates and holds a port without running a child
process, which is useful for compose-managed or otherwise external services.
`bindport release [service|port]` releases a reserved lease and marks it
stopped so normal cleanup can remove it.
Dashboard defaults
are local-only (`127.0.0.1:27080`) with auth disabled; non-loopback dashboard
binds require auth and a token. Set `dashboard.register_service = true` or pass
`bindport dashboard --register-service` when you want the dashboard itself to
appear in `bindport status`. Service entries currently apply `name`, `path`,
`command`, `args`, `env`, `hostname`, `route_url`, and `health_url`. Active
services with a loopback `http://` health URL report `pending`, `healthy`, or
`failing`; non-loopback and unsupported destinations remain `unknown`. Config
`env` entries are meant for application values; execution-sensitive names such
as `PATH`, `LD_PRELOAD`, `DYLD_*`, `NODE_OPTIONS`, and `GIT_CONFIG_*` are
ignored from config and should be passed explicitly with `--env` for one-off
runs.

Identity precedence is intentionally narrow during bootstrap: the optional
service argument in `bindport run <service> -- ...` wins, then
`BINDPORT_PROJECT` / `BINDPORT_SERVICE`, then config, then inference from
package-manager workspace roots, nearest `package.json`, the git worktree path,
and command name.

For monorepos, define service paths relative to the discovered project config.
When no CLI or environment service override is provided, BindPort selects the
deepest `[[services]].path` that contains the current working directory:

```toml
project = "example"

[[services]]
name = "web"
path = "apps/web"
hostname = "{branch}.example-web.localhost"

[[services]]
name = "api"
path = "apps/api"
hostname = "{branch}.example-api.localhost"
```

Wrapped commands always receive `PORT=<assigned>`. Service command, argument,
and env templates can pass the assigned port as an argv value for tools that do
not read `PORT` from the environment:

```toml
[[services]]
name = "web"
path = "apps/web"
command = ["storybook", "dev"]
args = ["--port", "{port}", "--host", "0.0.0.0"]
hostname = "{branch}.{project}.localhost"
route_url = "http://{hostname}"
health_url = "{route_url}/health"
env.PORT = "{port}"
env.HOSTNAME = "0.0.0.0"
env.NEXT_PUBLIC_BINDPORT_URL = "{route_url}"
```

Run the configured command with:

```sh
bindport run web
```

Explicit child commands still override service command config:

```sh
bindport run web -- next dev
```

For one-off commands that need a CLI flag before a service command is
configured, use a shell wrapper so `$PORT` is expanded by the child shell after
BindPort assigns it:

```sh
bindport run web -- sh -c 'storybook dev --port "$PORT" --host 0.0.0.0'
```

Supported template placeholders are `{port}`, `{host}`, `{url}`, `{project}`,
`{service}`, `{hostname}`, `{route_url}`, `{health_url}`, `{branch}`,
`{branch_label}`, `{git_branch}`, `{worktree}`, `{worktree_label}`, and
`{worktree_hash}`.
Use `{{` and `}}` when a template value needs literal braces, for example a
JSON-valued environment variable.
`bindport run --env NAME=VALUE`, `--hostname TEMPLATE`, `--route-url TEMPLATE`,
and `--health-url TEMPLATE` override service config for a single run.
`BINDPORT_HOSTNAME`, `BINDPORT_ROUTE_URL`, and
`BINDPORT_HEALTH_URL` can also override the matching service config values for
wrapper scripts.

For services started by another tool, reserve a BindPort port without wrapping a
process:

```sh
bindport reserve web
bindport release web
```

## Registry Cleanup

Stopped and stale entries can be removed from the local registry with:

```sh
bindport clean --dry-run
bindport clean
```

`bindport clean` removes stopped and stale entries by default. Stale entries
require confirmation before deletion; pass `--yes` for reviewed noninteractive
cleanup. Use `--stopped` or `--stale` to scope cleanup, and `--json` for
machine-readable counts. Active services are not removed.

## Documentation

- [Adoption Setup](docs/adoption.md): what to commit, what to ignore,
  no-proxy setup, framework examples, and agent guidance for project adoption.
- [Config](docs/config.md): config discovery, precedence, service entries,
  local overrides, validation, and supported placeholders.
- [Dashboard](docs/dashboard.md): local service dashboard, status API, scoped
  registry cleanup actions, service-style controls, configurable bind/auth
  options, dev modes, and security posture.
- [Status](docs/status.md): `status --json` schema, service URL selection, and
  agent-oriented lookup guidance.
- [Templates](docs/templates.md): output template lookup, built-in Traefik
  file-provider setup, custom templates, render lifecycle, and troubleshooting.
- [Monorepos](docs/monorepos.md): root config, path-scoped services, workspace
  inference, local overrides, and output examples for multi-package repos.
- [Platform Support](docs/platform-support.md): supported operating systems,
  package targets, filesystem paths, process behavior, and verification gates.
- [Release](docs/release.md): release prep automation, GitHub release binaries,
  Cargo publish helpers, and npm packaging.
- [Changelog](CHANGELOG.md): generated release notes from Conventional Commits.

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
