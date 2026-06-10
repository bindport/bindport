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
- Minimal CLI placeholders for `--help`, `--version`, `status`, and `doctor`.
- MIT license, security policy, third-party notices stub, CI/audit workflows,
  and local `mise` tasks.
- npm wrapper package skeleton.
- Example `.bindport.toml`, `.bindport.json`, and `.bindport.yaml` files.

The planned first working UX is:

```sh
bindport -- next dev
```

That command is not implemented yet. During bootstrap, use Cargo directly:

```sh
cargo run -p bindport -- --help
cargo run -p bindport -- doctor
cargo run -p bindport -- status --json
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
should prefer TOML, then JSON, then YAML.

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
