# Dashboard

BindPort includes a local dashboard for inspecting the same registry state
exposed by `bindport status --json` and cleaning stopped or stale registry
entries.

Serve it in the foreground from a source checkout with:

```sh
cargo run -p bindport -- dashboard serve
```

Or, after installing BindPort:

```sh
bindport dashboard serve
```

The command prints the URL before serving requests:

```text
dashboard: http://127.0.0.1:27080
```

Stop it with `Ctrl-C`.

Pass `--register-service` when you want the dashboard process itself to appear
in `bindport status` and `/api/status`:

```sh
bindport dashboard serve --register-service
```

For service-style control, use:

```sh
bindport dashboard start
bindport dashboard status
bindport dashboard stop
```

`start` runs the dashboard in the background and writes a small state file under
the BindPort state directory. `status` reports the recorded PID and URL, and
`stop` sends the dashboard process a termination signal. On Linux, BindPort
checks the recorded PID against `/proc` before signaling it. On other platforms,
`stop` can only check PID liveness, so stale state after PID reuse may need
manual cleanup. Background dashboard stderr is written to `dashboard.log` in the
same state directory, and startup failures include the first logged error.

## Port Selection

The dashboard binds to `127.0.0.1:27080` by default. If that port is already in
use, BindPort scans the configured BindPort range instead:

- project config or fallback config `default_range`, when present;
- otherwise the built-in `29000-29999` range.

The fallback scan skips configured `skip_ports` and active registry ports. The
dashboard does not bind privileged ports and does not claim `80` or `443`.

Override the bind IP and preferred port with CLI flags. Non-loopback hosts
require dashboard auth:

```sh
BINDPORT_DASHBOARD_TOKEN="change-me" \
  cargo run -p bindport -- dashboard serve --host 0.0.0.0 --port 27080 --auth required
```

Or set them in config:

```toml
[dashboard]
host = "127.0.0.1"
port = 27080
register_service = false
allowed_hosts = ["localhost", "127.0.0.1"]
```

## Views

The dashboard groups services by registry state:

- `active`
- `stopped`
- `stale`
- `conflict`, when a future registry state records a conflict
- other unexpected states

Rows show project, service, URL, branch, and root path. State is represented by
the group heading instead of a repeated row column. URL, branch, and root cells
include compact copy actions, and `http` / `https` URLs also include an open
action. Other URL schemes are displayed as plain text.

Stopped and stale groups include a cleanup action in the group header. Cleanup
removes the matching stopped or stale registry entries, triggers configured
output rendering/deletion, and then refreshes the dashboard. Active services
cannot be removed from the dashboard.

Each row has an expand control for secondary details: state, PID, port, health,
proxy render status, current working directory, and command. Health is
`unknown` until service health checks are implemented. Proxy status reflects a
recorded `traefik` output row when one exists. Expanded rows stay expanded
across automatic refreshes while the matching service remains in the registry
snapshot.

Use the search field to filter by state, project, service, URL, root path,
branch, PID, command, or working directory. State buttons narrow the table to
one registry state while keeping the text search active.

The dashboard refreshes its registry snapshot every five seconds and shows the
last successful refresh time in the header. If a later refresh fails, the last
successful view stays visible while the header reports the refresh error.

The header shows a lock button when the browser tab has a stored dashboard
token. Use it to clear the token from `sessionStorage` and return to the token
prompt. The footer shows the app name and build version.

## API

The dashboard serves:

- `GET /` - embedded dashboard HTML shell.
- `GET /assets/app.css` - embedded dashboard stylesheet.
- `GET /assets/app.js` - embedded dashboard client script.
- `GET /api/status` - JSON registry snapshot, matching `bindport status --json`.
- `POST /api/clean` - remove stopped and stale registry entries.
- `POST /api/clean/stopped` - remove stopped registry entries.
- `POST /api/clean/stale` - remove stale registry entries.
- `GET /healthz` - plain `ok` health response for smoke checks.

`/api/status` returns the registry snapshot with route-oriented fields such as
`hostname`, `route_url`, `outputs`, and `proxy`. `hostname` and `route_url` are
populated when a wrapped service config or run option sets them. `outputs`
contains per-service rendered output status. `proxy` is a compatibility alias
for recorded `traefik` output status and remains `null` when no matching output
row exists.

## Security Posture

The dashboard is local by default and has a narrow write surface:

- binds only to loopback by default;
- accepts `Host` headers for `127.0.0.1` and `localhost`;
- only exposes write actions for stopped/stale registry cleanup;
- does not start, stop, run, reserve, or release services;
- requires the `X-BindPort-Dashboard-Action: clean` header for cleanup requests
  so simple cross-site form posts cannot trigger cleanup in a browser.

Use `bindport clean --dry-run` from the CLI when you want to preview cleanup
counts before removing registry entries.

When `dashboard.auth.required` or `--auth required` is enabled, `/api/status`
and `/api/clean*` require `Authorization: Bearer <token>`. The HTML shell
remains public so the browser can load the token prompt and static assets, but
registry data and cleanup actions are not available until the token is
provided. The browser stores the token in `sessionStorage` for the current
tab/session only. Prefer `token_env` / `--token-env` over `--token` so the
secret does not land in shell history or the foreground `serve` process list.

When `dashboard start` receives `--token`, BindPort passes it to the detached
server through the configured token environment variable instead of keeping it
in the background process arguments.

```toml
[dashboard.auth]
required = true
token_env = "BINDPORT_DASHBOARD_TOKEN"
```

```sh
BINDPORT_DASHBOARD_TOKEN="change-me" \
  cargo run -p bindport -- dashboard serve --host 0.0.0.0 --auth required
```

Binding `0.0.0.0` with auth enabled accepts arbitrary Host headers so remote
browser testing works with an IP address or forwarded hostname. BindPort refuses
non-loopback dashboard binds when auth is disabled. For loopback-only dashboards
reached through a local hostname or tunnel, configure each non-local Host header
explicitly with `allowed_hosts` or `--allowed-host`.

Registry data can include project names, branch names, PIDs, command lines, and
working directories. Avoid putting secrets in local dev command arguments; use
environment or secret-management tooling instead.

## Development

Dashboard assets live in `crates/bindport-dashboard/static`. Release builds
embed those files. Debug/dev runs can read them from disk with:

```sh
cargo run -p bindport -- dashboard serve \
  --static-dir crates/bindport-dashboard/static
```

The dev static mode injects a lightweight reload script that refreshes the page
when those static files change. In debug builds with `--static-dir`, the
dashboard also exposes `/assets/dev-reload.js` and `/assets/dev-version` for
that reload loop.

The same workflows are available through `mise`:

```sh
mise run dev-dashboard
mise run dev-dashboard-static
BINDPORT_DASHBOARD_TOKEN="change-me" mise run dev-dashboard-remote
BINDPORT_DASHBOARD_TOKEN="change-me" mise run dev-dashboard-remote-static
```

`dev-dashboard` serves local static assets from disk and restarts the Rust
dashboard process when Cargo files or Rust crate sources change. Static asset
changes still use the lightweight browser reload loop and do not need a server
restart. `dev-dashboard-static` keeps the previous static-asset-only behavior
when you want to restart the Rust server manually.

`dev-dashboard-remote` binds `0.0.0.0`, requires token auth, serves the same
static assets from disk for testing from a remote browser, and restarts on Rust
server changes. Use `dev-dashboard-remote-static` when you need the remote
dashboard without Rust server restarts.
