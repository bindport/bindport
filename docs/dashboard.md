# Dashboard

BindPort includes a read-only local dashboard for inspecting the same registry
state exposed by `bindport status --json`.

Start it from a source checkout with:

```sh
cargo run -p bindport -- dashboard
```

Or, after installing a version that includes the dashboard command:

```sh
bindport dashboard
```

The command prints the URL before serving requests:

```text
dashboard: http://127.0.0.1:27080
```

Stop it with `Ctrl-C`.

## Port Selection

The dashboard binds to `127.0.0.1:27080` by default. If that port is already in
use, BindPort scans the configured BindPort range instead:

- project config or fallback config `default_range`, when present;
- otherwise the built-in `29000-29999` range.

The fallback scan skips configured `skip_ports` and active registry ports. The
dashboard does not bind privileged ports and does not claim `80` or `443`.

## Views

The dashboard groups services by registry state:

- `active`
- `stopped`
- `stale`
- other unexpected states

Rows show project, service, URL, worktree, branch, PID, and command. URL cells
include `Open` and `Copy` actions for quick browser and testing workflows. Only
`http` and `https` URLs are opened as links; other schemes are displayed as
plain text.

The dashboard refreshes its registry snapshot every five seconds and shows the
last successful refresh time in the header. If a later refresh fails, the last
successful view stays visible while the header reports the refresh error.

## API

The dashboard serves:

- `GET /` - embedded read-only HTML dashboard.
- `GET /api/status` - JSON registry snapshot, matching `bindport status --json`.
- `GET /healthz` - plain `ok` health response for smoke checks.

`/api/status` returns the registry snapshot with route-oriented fields such as
`hostname`, `route_url`, and `proxy`. Those fields remain `null` until a future
proxy adapter renders routes.

## Security Posture

The dashboard is local and read-only:

- binds only to loopback by default;
- accepts `Host` headers for `127.0.0.1` and `localhost`;
- does not provide write actions or registry mutation APIs;
- does not start, stop, clean, or release services.

Registry data can include project names, branch names, PIDs, command lines, and
working directories. Avoid putting secrets in local dev command arguments; use
environment or secret-management tooling instead.
