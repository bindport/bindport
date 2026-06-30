# Monorepo Example

This fixture shows one root BindPort config for two package services:

- `apps/web`
- `apps/api`

It demonstrates:

- path-scoped services selected from the current directory;
- branch-scoped hostnames for separate local domains;
- service env templates for values needed before process startup;
- Traefik output files under `.bindport/generated/traefik`;
- opt-in `.env.local` output through `bindport-env-local`;
- machine-local override values in `.bindport.local.toml.sample`.

Try the config from a source checkout:

```sh
cd examples/monorepo/apps/web
cargo run -p bindport -- config explain
cargo run -p bindport -- config validate
cargo run -p bindport -- run web -- sh -c 'printf "PORT=%s\n" "$PORT"'
cargo run -p bindport -- render --dry-run
```

Copy `.bindport.local.toml.sample` to `.bindport.local.toml` when a local
machine needs different output targets, dashboard auth, or host allowlists.
