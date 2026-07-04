# Running Services

BindPort can run a command passed on the CLI or a service command declared in
config. Shared projects should prefer configured services so scripts, route
metadata, health checks, outputs, and hooks all agree on the same identity.

The goal is not just to launch a process. The goal is to launch it in a way
that every other local tool can understand: the registry knows the service, the
dashboard can show it, `open` can find it, templates can render it, hooks can
react to it, and agents can inspect it.

## One-Off Commands

Put the child command after `--`:

```sh
bindport -- npm run dev
```

The child receives:

- `PORT`: the assigned port.
- inherited stdio.
- inherited parent environment, except values overridden by service config or
  `--env`.

Use a shell only when shell expansion is intentional:

```sh
bindport -- sh -c 'storybook dev --port "$PORT" --host 0.0.0.0'
```

For tools that accept structured args, prefer config `command` and `args`
instead of shell wrappers.

One-off commands are useful for experiments, but they are intentionally less
expressive than configured services. They do not document the expected service
path, route URL, health URL, generated outputs, or framework-specific args for
the rest of the team.

## Configured Services

Example:

```toml
project = "example"

[[services]]
name = "web"
path = "apps/web"
command = ["storybook", "dev"]
args = ["--port", "{port}", "--host", "0.0.0.0"]
hostname = "{branch}.example-web.localhost"
route_url = "http://{hostname}"
health_url = "{route_url}/health"
env.PORT = "{port}"
env.NEXT_PUBLIC_BINDPORT_URL = "{route_url}"
```

Run it with:

```sh
bindport run web
```

BindPort changes into the service `path` before spawning the child. The route
metadata is stored in the registry and becomes available to status, dashboard,
templates, hooks, and `bindport open`.

Configured services are the main team workflow. Once a service is in config,
developers and agents can use the same command:

```sh
bindport run web
```

That command becomes the stable contract even if the framework command, route
hostname pattern, health URL, or output behavior changes later.

## CLI Overrides

Use CLI overrides for local experiments or scripts that need to avoid editing
shared config:

```sh
bindport run web \
  --hostname '{branch}.example-web.localhost' \
  --route-url 'http://{hostname}' \
  --health-url '{route_url}/health' \
  -- npm run dev
```

Use `--env NAME=VALUE` for one-off application env values:

```sh
bindport run web --env NEXT_PUBLIC_BINDPORT_URL='{route_url}'
```

Config cannot set execution-sensitive names such as `PATH`, `LD_PRELOAD`,
`DYLD_*`, `NODE_OPTIONS`, or `GIT_CONFIG_*`. Pass those explicitly only when a
reviewed one-off command needs them.

## Framework Port Flags

Some frameworks honor `PORT`; others require a CLI flag. BindPort supports both
patterns through templates.

Next.js:

```toml
[[services]]
name = "next"
path = "apps/web"
command = ["next", "dev"]
args = ["--hostname", "0.0.0.0", "--port", "{port}"]
```

Vite:

```toml
[[services]]
name = "vite"
path = "apps/web"
command = ["vite"]
args = ["--host", "0.0.0.0", "--port", "{port}", "--strictPort"]
```

Storybook:

```toml
[[services]]
name = "storybook"
path = "apps/web"
command = ["storybook", "dev"]
args = ["--host", "0.0.0.0", "--port", "{port}"]
```

FastAPI with Uvicorn:

```toml
[[services]]
name = "api"
path = "services/api"
command = ["uvicorn", "example.main:app"]
args = ["--host", "0.0.0.0", "--port", "{port}"]
```

## Environment Bridging

Use service `env` for values the app needs at runtime:

```toml
env.PORT = "{port}"
env.HOSTNAME = "0.0.0.0"
env.NEXT_PUBLIC_BINDPORT_URL = "{route_url}"
env.BINDPORT_SERVICE = "{service}"
```

Use output templates for files that tools reread after startup, such as
`.env.local`. Startup-critical values should still be passed through service
`command`, `args`, `env`, or one-off `--env`.

## Signals And Exit Codes

On Unix platforms, BindPort forwards SIGINT and SIGTERM to the wrapped child
and exits with the child's status code. This lets package scripts and CI jobs
treat BindPort as a transparent wrapper around the dev command.

## Probe Window

BindPort probes a port, releases the probe listener, then starts the child.
Another process can still claim the port before the child binds. BindPort
retries once when the child fails immediately and the assigned port is then
occupied.
