# BindPort

BindPort is a proxy-neutral local development port registry, allocator, and
runner. It is meant to wrap development commands, assign a stable local port,
record which project/service/worktree owns that port, and eventually emit config
fragments for an existing proxy such as Traefik.

BindPort does not aim to become the local edge proxy. By default it should not
bind 80/443, install certificates, mutate DNS, edit `/etc/hosts`, or require a
root-owned daemon.

## Bootstrap Status

This repository is in the pre-v0.1 bootstrap phase. The current scaffold includes:

- Rust Cargo workspace with `bindport` plus core, registry, runner, and adapter
  crates.
- Minimal CLI support for `--help`, `--version`, `status`, `doctor`, and
  one-shot `bindport -- <command>` command wrapping.
- Optional config discovery from `.bindport.toml`, `.bindport.json`, or
  `.bindport.yaml`, with a fallback config next to the registry file.
- Basic project/service identity resolution from environment, config, command
  inference, and `bindport run <service> -- ...`, with git branch/worktree
  metadata recorded when available.
- Basic SQLite-backed lease/run recording with `bindport status --json`.
- MIT license, security policy, third-party notices stub, CI/audit workflows,
  and local `mise` tasks.
- npm wrapper package skeleton.
- Example `.bindport.toml`, `.bindport.json`, and `.bindport.yaml` files.

The first runner slice is available:

```sh
cargo run -p bindport -- -- next dev
```

It probes TCP loopback (IPv4 and IPv6) for a currently-free port from
`29000-29999`, prefers the previous port for the same project/service/worktree
identity when it is still free, otherwise scans from a stable identity-based
offset, injects `PORT=<assigned>`, inherits child stdio, forwards Unix
SIGINT/SIGTERM to the child, and exits with the child process exit code. This
bootstrap runner is probe-then-spawn, so another process can still claim the port
before the child binds. Package metadata inference and allocation retry are still
future v0.1 work.

During bootstrap, use Cargo directly:

```sh
cargo run -p bindport -- --help
cargo run -p bindport -- doctor
cargo run -p bindport -- init
cargo run -p bindport -- status --json
cargo run -p bindport -- run web -- sh -c 'echo "$PORT"'
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
```

## Configuration Examples

Starter config examples live in [examples/config](examples/config):

- [`.bindport.toml`](examples/config/.bindport.toml)
- [`.bindport.json`](examples/config/.bindport.json)
- [`.bindport.yaml`](examples/config/.bindport.yaml)

TOML is the reference format. When equivalent config files exist, discovery
prefers TOML, then JSON, then YAML. BindPort walks upward from the current
directory and uses the first project config it finds. If no project config
exists, it falls back to the optional `config.toml` stored next to
`registry.sqlite` in the BindPort state directory. `bindport init` creates that
fallback config with default values. Config is never required; missing config
means built-in defaults are used.

The current implementation reads only top-level `project`, `service`,
`default_range`, and `skip_ports`. The example `identity`, `services`, and
`proxy` sections document the intended future shape and are not applied yet;
`bindport doctor` reports ignored top-level keys so typos and future-only
sections are visible.

Identity precedence is intentionally narrow during bootstrap: the optional
service argument in `bindport run <service> -- ...` wins, then
`BINDPORT_PROJECT` / `BINDPORT_SERVICE`, then config, then inference from the
git worktree path and command name.

## Documentation

- [Release](docs/release.md): bootstrap release policy, package-name timing, and
  future npm/Cargo publish shape.

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
