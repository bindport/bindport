# Monorepo Configuration

BindPort walks upward from the current directory and uses the first project
config it finds. In a monorepo, keep one `.bindport.toml` at the repo root and
scope each service with a relative `path`.

## Root Config

```toml
project = "orderful"

[[services]]
name = "web"
path = "apps/web"
hostname = "{branch}.orderful-website.localhost"
env.PORT = "{port}"
env.HOSTNAME = "0.0.0.0"
env.NEXT_PUBLIC_BINDPORT_URL = "{route_url}"

[[services]]
name = "api"
path = "apps/api"
hostname = "{branch}.orderful-api.localhost"
env.PORT = "{port}"
env.BINDPORT_ROUTE_URL = "{route_url}"
```

Run from a service directory and BindPort selects the deepest matching path:

```sh
cd apps/web
bindport -- next dev

cd ../api
bindport -- cargo run
```

CLI and environment overrides still win. Use `bindport run api -- ...` or
`BINDPORT_SERVICE=api` when a command is launched from a shared directory.

## Workspace Inference

Explicit config wins, but package manager metadata is still useful fallback
context. If the repo has npm/yarn `package.json#workspaces` or
`pnpm-workspace.yaml`, BindPort uses the workspace root package as the inferred
project and the nearest package as the inferred service when config does not set
them.

Use config for names that should stay stable across package renames. Use
workspace inference for simple repos where package names already match the local
service names you want.

## Multiple Worktrees

Worktree identity is part of BindPort's service key. Two checkouts of the same
monorepo can run the same service names without colliding as long as they are in
different worktree paths. Branch labels are also available to hostname templates:

```toml
[[services]]
name = "web"
path = "apps/web"
hostname = "{branch}.orderful-website.localhost"
```

In a worktree on branch `feature/tree`, that renders a route hostname such as
`feature-tree.orderful-website.localhost`. If two worktrees use the same branch
name, BindPort still keeps their registry identity separate through the
worktree hash, but hostname templates should include enough information to stay
unique for your local proxy setup.

## Local Overrides

Machine-specific values belong in `.bindport.local.toml` or
`bindport.local.toml` beside the root config. Keep those files untracked.

```toml
[output_defaults]
target_host = "host.docker.internal"

[dashboard]
host = "0.0.0.0"
allowed_hosts = ["localhost", "127.0.0.1", "devbox.example.test"]

[dashboard.auth]
required = true
token_env = "BINDPORT_DASHBOARD_TOKEN"
```

Local overrides merge over the root config. For outputs, matching `name` values
merge, so local config can override destinations or disable one output without
duplicating the whole base file.

## Outputs

Root configs can define shared outputs once:

```toml
[output_defaults]
root = ".bindport/generated"
target_host = "127.0.0.1"
target_scheme = "http"

[[outputs]]
name = "traefik"
template = "bindport-traefik"
target = "traefik/{{ route.slug }}.yml"

[[outputs]]
name = "env-local"
template = "bindport-env-local"
root = "."
target = "apps/{{ route.service }}/.env.local"
auto_render = false
```

The Traefik output is safe to auto-render because file-provider watchers reload
after route state changes. `.env.local` is also an output, but many frameworks
read it only during startup. Use `[[services]].env` or `bindport run --env` for
values that must exist before the process starts; use the env-file output for
manual refreshes or tools that reread dotenv files. When shared
`output_defaults.root` points at a generated config directory, set `root = "."`
on env-file outputs that intentionally write back into package directories.

## Checks

Use these commands when changing a monorepo config:

```sh
bindport config explain
bindport config validate
bindport doctor outputs
bindport render --dry-run
```

`config explain` shows which config file and service path matched the current
directory. `config validate` catches missing or duplicate service names, unsafe
service paths, and output configuration errors.

## Example

A complete sample monorepo config lives in
[examples/monorepo](../examples/monorepo).
