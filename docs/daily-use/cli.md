# CLI Commands

BindPort is one CLI with a few related jobs: run a process with an assigned
port, inspect the registry, render integration files, and manage local trust for
hooks. Commands are designed to work both directly in a shell and inside project
scripts. The canonical v1-candidate name, compatibility, exit-status, and
machine-output policy is the [CLI Stability Contract](../reference/cli-stability.md).
Human help and table formatting are not machine interfaces.

## Command Groups

| Command | Purpose |
|---|---|
| `bindport -- <command>` | Run a one-off command with an assigned `PORT`. |
| `bindport run [service] [-- <command>]` | Run a configured service or override it for one run. |
| `bindport reserve [service]` | Hold a port for an externally managed process. |
| `bindport reserve --all` | Prepare ports for every named service in the discovered project config. |
| `bindport release [service\|port]` | Release a reserved port. |
| `bindport status [--json]` | Show active, reserved, stopped, and stale registry state. |
| `bindport list [--json]` | Group registry services by project for inventory views. |
| `bindport registry export` | Print a full registry debug/backup JSON snapshot. |
| `bindport open [service]` | Print or open the best URL for an active service. |
| `bindport port <service> [--project PROJECT]` | Print an active or reserved service port. |
| `bindport clean` | Remove stopped and stale registry entries. |
| `bindport init` | Create project or user fallback config. |
| `bindport config explain` | Show discovered config, local overrides, and identity sources. |
| `bindport config validate` | Validate config structure and output actionable errors. |
| `bindport doctor` | Show local diagnostics and the next candidate port. |
| `bindport doctor outputs` | Validate configured output rendering without writing files. |
| `bindport dashboard ...` | Serve, start, stop, or inspect the local dashboard. |
| `bindport render ...` | Render configured output files from registry state. |
| `bindport templates ...` | List, show, or export output templates. |
| `bindport hooks ...` | Inspect, trust, deny, or reset hook trust decisions. |

## Run Commands

For one-off usage, put the child command after `--`:

```sh
bindport -- npm run dev
bindport -- sh -c 'printf "PORT=%s\n" "$PORT"'
```

BindPort allocates a port, injects `PORT=<assigned>`, records a run in the
registry, forwards Unix SIGINT/SIGTERM to the child, and returns normal child
exit codes unchanged. A signaled Unix child is represented as `128 + signal`.
BindPort-owned failures use `1`; that number is not reserved from children. See
the [exit contract](../reference/cli-stability.md#exit-status).

For configured services, prefer `bindport run <service>`:

```sh
bindport run web
bindport run api
```

When a service has `command` and `args` in config, BindPort expands templates
such as `{port}`, `{hostname}`, and `{route_url}` after allocation. Configured
`env`, `command`, and `args` may also read one startup snapshot of same-project,
exact-worktree active or reserved siblings through `{services.<name>.<field>}`.
An explicit child command overrides the configured command for one run:

```sh
bindport run web -- next dev --hostname 0.0.0.0 --port "$PORT"
```

Use route metadata options when a one-off run needs values normally provided by
config:

```sh
bindport run web \
  --hostname '{branch}.example-web.localhost' \
  --route-url 'http://{hostname}' \
  --health-url '{route_url}/health' \
  -- npm run dev
```

Use `--env NAME=VALUE` for one-off application environment values. Values are
templated after allocation:

```sh
bindport run web --env NEXT_PUBLIC_BINDPORT_URL='{route_url}'
```

## Config Commands

Initialize a project config:

```sh
bindport init
```

Create the optional user fallback config:

```sh
bindport init --user
```

Explain what BindPort found from the current directory:

```sh
bindport config explain
```

Validate project config before committing changes:

```sh
bindport config validate
```

## Registry Commands

Show the registry:

```sh
bindport status
bindport status --json
bindport list
bindport list --json
bindport registry export
```

`status --json` is the normal machine-readable current-state API and alone uses
the frozen status schema `1.0`. `list --json` is a grouped inventory view with
its own current schema `0.1`. `registry export` is a fuller debug/backup payload
with its own schema `0.1`, SQLite `user_version`, and raw lease, run, output
ownership, and output render scheduling rows. Neither is status schema 1.0, and
`clean --json` is currently unversioned. No JSON array ordering is guaranteed.
It can contain sensitive local data, including full command lines that may
include tokens or passwords passed as arguments, plus filesystem paths. Review
and redact it before sharing in a bug report.

Resolve the best active service URL from the registry-wide snapshot:

```sh
bindport open web --print
bindport open web --browser
bindport open web --project example
```

`--project` does not select a worktree. If duplicate worktrees can be active,
filter `status --json` by `identity_key` or exact worktree fields instead of
assuming `open` selects the current checkout. Use `--print`, not `--browser`,
for non-interactive automation.

Reserve and release a port for an external process:

```sh
bindport reserve web
bindport release web
bindport release 29123
```

Prepare every named service in the discovered project config before starting
processes:

```sh
bindport reserve --all
```

`reserve --all` is scoped to the current project and worktree. It preserves
matching active services and reservations, allocates every missing configured
service, and commits all new reservations as one idempotent, all-or-nothing
registry operation. Use it before startup when configured sibling references
need every address. It starts no children, orders no dependency graph, implies
no readiness, and owns no OS sockets; reservations
coordinate BindPort registry clients only.

Print one prepared or running service port for scripts:

```sh
bindport port web
bindport port web --project example
```

`port` uses the same current-worktree project and service scope for active and
reserved services. On success, stdout is exactly the decimal port followed by a
newline. Missing, stopped, stale, or ambiguous matches fail instead of printing
a fallback port or selecting another project or worktree.

Clean stopped and stale entries:

```sh
bindport clean --dry-run
bindport clean --stopped
bindport clean --stale --yes
bindport clean --json --yes
```

`clean --json` is unversioned. Destructive cleanup can run approved lifecycle
hooks, and a hook that writes to inherited stdout can contaminate the JSON
stream. `clean --json --dry-run` executes no hooks and is the safe parse-only
preview.

## Dashboard Commands

Serve the dashboard in the foreground:

```sh
bindport dashboard serve
```

Run it as a background service:

```sh
bindport dashboard start
bindport dashboard status
bindport dashboard stop
```

Bind to a remote-accessible host only with auth enabled:

```sh
BINDPORT_DASHBOARD_TOKEN="change-me" \
  bindport dashboard serve --host 0.0.0.0 --auth required
```

Use `--token-env NAME` instead of `--token VALUE` when possible so secrets do
not land in shell history or process arguments.

## Output Commands

List templates:

```sh
bindport templates list
```

Inspect or export a template:

```sh
bindport templates show bindport-traefik
bindport templates export --source built-in bindport-traefik
```

Render configured outputs:

```sh
bindport render
bindport render traefik
bindport render --dry-run
bindport render --diff
bindport render --repair
bindport render --verbose
```

`--verbose` / `-v` prints render diagnostics to stderr. It includes selected
outputs, template source, route and file counts, output roots, ownership row
counts, and lifecycle removal/adoption counts. Set `BINDPORT_LOG=debug` when
you need the same diagnostics from automatic renders triggered by
`bindport run`, `clean`, or dashboard cleanup.

Validate output config, template lookup, and planned target paths:

```sh
bindport doctor outputs
```

## Hook Commands

Hooks are disabled until trusted locally. A project config can declare hooks,
but config cannot approve execution by itself.

Inspect hook state:

```sh
bindport hooks status
```

Approve, deny, or reset reviewed hooks:

```sh
bindport hooks trust reload-proxy
bindport hooks deny reload-proxy
bindport hooks reset reload-proxy
```

Trust scope defaults to the current worktree. Use `--scope repo` only when the
same reviewed hook definition should be trusted across worktrees that share a
git repository:

```sh
bindport hooks trust --scope repo reload-proxy
```

## Diagnostics

Run the local diagnostics before opening an adoption PR or when a port looks
wrong:

```sh
bindport doctor
bindport doctor outputs
```

`doctor` reports config discovery, registry health, effective identity, active
registry ports, obvious registry/listener conflicts, unknown OS listener
conflicts, and the next candidate port. `doctor outputs` validates configured
outputs, target hosts, resolved output roots, ownership rows, and hook trust
state without writing output files.
