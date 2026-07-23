# Monorepo Example

This fixture shows one root BindPort config for two package services:

- `apps/web`
- `apps/api`

It demonstrates:

- path-scoped services selected from the current directory;
- branch-scoped hostnames for separate local domains;
- service env templates for values needed before process startup, including the
  web service's `{services.api.route_url}` and `{services.api.port}` references;
- Traefik output files under `.bindport/generated/traefik`;
- opt-in `.env.local` output through `bindport-env-local`;
- machine-local override values in `.bindport.local.toml.sample`.

Try the config from a source checkout:

```sh
cd examples/monorepo
cargo run -p bindport -- config validate
cargo run -p bindport -- reserve --all
cargo run -p bindport -- run web -- sh -c \
  'printf "PORT=%s API_URL=%s API_PORT=%s\n" "$PORT" "$NEXT_PUBLIC_API_URL" "$API_PORT"'
cargo run -p bindport -- render --dry-run
```

`reserve --all` assigns both service addresses before either process starts, so
`web` can expand the API references even when `api` is still reserved. An
assigned address does not mean the API process is running or ready; the web
application remains responsible for retries or readiness handling.

Copy `.bindport.local.toml.sample` to `.bindport.local.toml` when a local
machine needs different output targets, dashboard auth, or host allowlists.
