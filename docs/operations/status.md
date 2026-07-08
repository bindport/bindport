# Status And Cleanup

BindPort records service state in its local SQLite registry. The registry is
what powers `bindport status`, `bindport list`, `bindport open`, the dashboard,
output rendering, and cleanup.

The registry is local machine state, not project config. By default it lives at
`$XDG_STATE_HOME/bindport/registry.sqlite`, or
`~/.local/state/bindport/registry.sqlite` when `XDG_STATE_HOME` is unset.

## Registry States

| State | Meaning |
|---|---|
| `active` | BindPort started or registered the process and it still appears live. |
| `reserved` | A port is intentionally held for a process BindPort does not wrap. |
| `stopped` | The run or reservation exited or was released normally. |
| `stale` | The registry entry points at a process that no longer appears live. |

Stopped and stale entries are retained so users can see recent state and so
output cleanup can make deliberate decisions. Use cleanup commands when the
history is no longer useful.

## Human Status

Run:

```sh
bindport status
```

The human output is for quick shell inspection. It groups the latest known
service records and includes the state, project, service, URL, PID, and command
when available.

Use the dashboard when you want a grouped, searchable view with copy/open
buttons and stopped/stale cleanup actions:

```sh
bindport dashboard serve
```

## Project Listing

Run:

```sh
bindport list
bindport list --json
```

`bindport list` is a registry-wide inventory view. It groups latest service
records by project and prints compact service rows with state, address, best URL,
branch label, and PID. `bindport list --json` exposes the same grouping through
a small JSON payload with `schema_version`, `generated_at`, aggregate counts,
and `projects[].services[]`.

Use `list` when you need to answer "what projects and services are recorded on
this machine?" Use `status --json` when you need the full registry snapshot,
including output summaries, run history, hook visibility, and all service fields.

## JSON Status

`bindport status --json` returns the local registry snapshot:

```sh
bindport status --json
```

The top-level `schema_version` is currently `0.4`; pre-1.0 releases may extend
the schema, but existing fields should remain stable within a major version.
The checked-in JSON Schema for the current payload is
[status.schema.json](../status.schema.json).

Top-level fields:

- `schema_version`: status schema version.
- `generated_at`: registry read timestamp.
- `outputs`: aggregate output-file counts grouped by output name.
- `services`: latest service records grouped by BindPort identity.
- `runs`: run history, newest first.
- `hooks`: configured hook trust visibility for the current directory.

Service fields most useful to agents:

- `project`, `service`, `identity_key`: stable service identity.
- `state`: `active`, `reserved`, `stopped`, or `stale`.
- `host`, `port`, `url`: direct loopback URL for the wrapped process.
- `hostname`, `route_url`, `health_url`: configured route metadata when present.
- `health`: `unknown`, `pending`, `healthy`, or `failing`.
- `branch`, `branch_label`, `worktree_path`, `commit`: git context when known.
- `outputs`, `proxy`: generated output files and proxy-oriented summary.

Agents and scripts should prefer `status --json` or `list --json` over parsing
human output. Use `status --json` for full registry detail and `list --json`
for grouped project/service inventory.

## Registry Export

Use registry export for debug or backup workflows:

```sh
bindport registry export
```

The export is JSON-only and includes raw `leases`, `runs`, `output_files`, and
`output_render_state` rows. Output rows include scoped ownership fields such as
`output_scope`, `output_root`, `config_root`, `worktree_path`, and
`worktree_hash`, which makes the payload useful when diagnosing multi-worktree
output ownership problems.

Prefer `status --json` for normal automation. Use `registry export` when you
need a fuller local registry snapshot for troubleshooting, bug reports, or a
local backup before manual cleanup.
The export can contain sensitive local data, including full command lines that
may include tokens or passwords passed as arguments, plus filesystem paths.
Review and redact it before sharing in a bug report.

## URL Lookup

`bindport open [service]` resolves the best active service URL from the same
snapshot. It prints `route_url` when configured, otherwise the direct loopback
`url`.

Examples:

```sh
bindport open web
bindport open web --project example
bindport open web --print
bindport open web --browser
```

Use `--project PROJECT` when multiple active services share the same service
name. `--browser` only launches HTTP or HTTPS URLs.

## Reservations

Reserved services come from `bindport reserve [service]`. They hold a port for
an externally managed process and appear in `services`, but they do not run a
child process.

Use reservations when Docker Compose, another process supervisor, or a manually
started app needs a port chosen by BindPort:

```sh
bindport reserve web
bindport status --json
bindport release web
bindport release 29123
```

`release` marks a reserved lease stopped. It does not delete the stopped entry;
use cleanup when you want it removed from the registry.

## Cleanup

Preview cleanup first:

```sh
bindport clean --dry-run
```

Remove stopped entries:

```sh
bindport clean --stopped
```

Remove stale entries:

```sh
bindport clean --stale --yes
```

Remove stopped and stale entries, which is the default selection:

```sh
bindport clean --all --yes
```

Machine-readable cleanup counts are available with `--json`:

```sh
bindport clean --json --dry-run
bindport clean --json --yes
```

Stale cleanup requires confirmation unless `--yes` is passed. Active and
reserved services are not removed by cleanup.

Dashboard cleanup uses the same registry cleanup behavior for stopped and stale
groups. Cleanup can also trigger configured output deletion for routes that are
removed from the registry.

## Output And Hook Effects

Registry state changes can trigger output rendering when an output has
`auto_render = true`. Cleanup and release operations produce lifecycle events
used by templates and hooks, including removed-route events when entries leave
the registry.

Hooks still require local trust before execution. See
[Configuration](../daily-use/configuration.md) and
[Templates](../integrations/templates.md) for the output and hook
configuration model.

## Examples

Common shell checks:

```sh
bindport status --json
bindport registry export
bindport open web
bindport open web --project example
bindport open web --browser
bindport clean --dry-run
bindport reserve api
bindport release api
```
