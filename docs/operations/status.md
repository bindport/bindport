# Status And Cleanup

BindPort records service state in its local SQLite registry. The registry is
what powers `bindport status`, `bindport list`, `bindport open`, the dashboard,
output rendering, and cleanup.

The registry is local machine state, not project config. By default it lives at
`$XDG_STATE_HOME/bindport/registry.sqlite`, or
`~/.local/state/bindport/registry.sqlite` when `XDG_STATE_HOME` is unset. See
[Registry Migration Policy](../reference/registry-migrations.md) for supported
schema upgrades, preservation guarantees, failure handling, and backups.

## Registry States

| State | Meaning |
|---|---|
| `active` | BindPort started or registered the process and it still appears live. |
| `reserved` | A port is intentionally held for a process BindPort does not wrap. |
| `stopped` | The run or reservation exited or was released normally. |
| `stale` | The registry entry points at a process that no longer appears live. |

Stopped and stale entries are retained so users can see recent state and so
output cleanup can make deliberate decisions. Building a normal status snapshot
can reconcile an apparently active record to stale and can perform a configured
loopback HTTP health probe, so status/list/open/dashboard reads are not pure
byte-for-byte database reads. Use cleanup commands when the history is no
longer useful.

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
a small JSON payload with its own current `schema_version` of `0.1`,
`generated_at`, aggregate counts, and `projects[].services[]`. That version is
not status schema 1.0 and does not carry the status field-freeze promise.

Use `list` when you need to answer "what projects and services are recorded on
this machine?" Use `status --json` when you need the full registry snapshot,
including output summaries, run history, hook visibility, and all service fields.

## JSON Status

`bindport status --json` returns the local registry snapshot:

```sh
bindport status --json
```

The payload is frozen as status schema `1.0`. The checked-in machine-readable
contract is [status.schema.json](../status.schema.json). This version belongs to
`status --json`; it is not the SQLite `user_version`, registry export version,
`list --json` version, or config schema version. The dashboard currently mirrors
the status payload at `/api/status`, but this does not create a separate
dashboard schema version.

### v1 Compatibility Policy

Through BindPort v1 releases:

- existing fields remain present with compatible names, JSON kinds,
  nullability, and meanings;
- existing required fields do not become optional, and a required nullable
  field continues to appear even when its value is `null`;
- object fields may be added. Consumers must ignore fields they do not
  recognize; the JSON Schema therefore permits additional object properties;
- the documented state, health, output status, hook status/trust, hook event,
  proxy adapter, and hook target enum values are closed for v1. Adding an enum
  value is not treated as a harmless additive object field; and
- object member order and array order are not compatibility guarantees. Select
  records by identity/name/ID and sort explicitly when order matters.

Removing or renaming a field, changing its meaning, narrowing its JSON kind or
nullability, or changing an enum set requires a future incompatible schema.

All current properties below are emitted on every matching object except
`hooks.error`, which appears only when current-directory config cannot be
resolved. Nullable properties are required properties with a JSON `null` value,
not omitted properties.

### Top-Level Object

| Field | JSON kind | Meaning |
|---|---|---|
| `schema_version` | string, exactly `"1.0"` | Status contract version. |
| `generated_at` | string | UTC registry-read timestamp. |
| `outputs` | array of output summaries | Aggregate owned-output counts by output name. |
| `services` | array of services | Latest lease/run view for each registry identity. |
| `runs` | array of runs | Retained run history. |
| `hooks` | object | Hook configuration and trust visibility for the current directory. |

### Output Summary

Each top-level `outputs[]` object contains:

| Field | JSON kind | Meaning |
|---|---|---|
| `name` | string | Configured output name. |
| `pending` | non-negative integer | Owned rows in `pending` state. |
| `rendered` | non-negative integer | Owned rows in `rendered` state. |
| `removed` | non-negative integer | Owned rows in `removed` state. |
| `error` | non-negative integer | Owned rows in `error` state. |

### Service Object

Each `services[]` object contains all of these fields:

| Field | JSON kind | Meaning |
|---|---|---|
| `project`, `service` | string | Resolved project and service names. |
| `state` | enum string | `active`, `reserved`, `stopped`, or `stale`. |
| `port` | integer, 0-65535 | Recorded service port. |
| `host`, `url` | string | Recorded host and direct `http://host:port` URL. |
| `hostname`, `route_url`, `health_url` | string or `null` | Configured route/health metadata when available. |
| `worktree_path`, `worktree_hash`, `git_common_dir` | string or `null` | Git checkout identity when available. |
| `branch`, `branch_label`, `commit` | string or `null` | Git revision metadata when available. |
| `identity_key` | string or `null` | BindPort identity key; old records can predate it. |
| `pid` | non-negative integer or `null` | Latest run PID; reservations have `null`. |
| `command`, `cwd` | string | Latest run command and working directory. A reservation uses `"reserved"` and an empty cwd until promoted. |
| `started_at` | string | Latest run start, or reservation allocation, UTC timestamp. |
| `exited_at` | string or `null` | Recorded exit/stale-observation timestamp, or `null` while no exit is recorded. |
| `exit_code` | 32-bit integer or `null` | Child exit code when known; running, reserved, stale, or signaled records can be `null`. |
| `health` | enum string | `unknown`, `pending`, `healthy`, or `failing`. |
| `outputs` | array of service outputs | Owned output rows associated with this route identity. |
| `proxy` | proxy object or `null` | Compatibility alias for an associated Traefik output. |

A service `outputs[]` object always has string `name`, enum-string `status`
(`pending`, `rendered`, `removed`, or `error`), nullable-string `reason`, and
string `path`.

A non-null `proxy` object always has `adapter` equal to `"traefik"`, boolean
`rendered`, and string `target`. Services without that alias emit `proxy: null`.

### Run Object

Each `runs[]` object contains integer `id` and `lease_id`, a non-negative integer
`pid`, string `command`, `cwd`, and `started_at`, nullable-string `exited_at`,
and a nullable 32-bit integer `exit_code`. Registry-generated timestamps use UTC
RFC 3339-style values at second precision. Consumers should still treat the
contracted JSON kind as string rather than relying on presentation ordering.

### Hooks Object

`hooks.items` is always an array. A hook item contains:

| Field | JSON kind | Meaning |
|---|---|---|
| `name` | string | Effective hook name. |
| `status` | enum string | `approved`, `denied`, `changed`, or `pending`. |
| `trust` | enum string | `approved (worktree)`, `approved (repo)`, `denied (worktree)`, `denied (repo)`, `changed`, or `pending`. |
| `source` | string | Human-readable config source; it can include a local path. |
| `events` | array of enum strings | `route_started`, `route_finished`, `routes_removed`, `routes_marked_stale`, `render_requested`, or `output_rendered`. |
| `command` | array of strings | Structured hook argv. |
| `command_display` | string | Human-readable command rendering. |
| `timeout_ms` | non-negative integer | Effective timeout in milliseconds. |
| `hook_hash` | string | Effective hook-definition fingerprint. |
| `target` | object | Target `kind`, `display`, and `hash`. |

Hook target `kind` is `local_file`, `missing_file`, or `opaque`; `display` and
`hash` are strings. When config resolution fails, `hooks` additionally contains
a string `error` and `items` remains present (normally empty).

Agents and scripts should prefer `status --json` or `list --json` over parsing
human output. Both are registry-wide: select by `identity_key` or exact
project/service/worktree fields, never by array position. Use `status --json`
for the complete v1 status contract and `list --json` for grouped
project/service inventory. See the [CLI Stability Contract](../reference/cli-stability.md)
for stdout, stderr, exit, and ordering guarantees.

## Registry Export

Use registry export for debug or backup workflows:

```sh
bindport registry export
```

The export is JSON-only, currently reports its own export `schema_version` of
`0.1` plus SQLite `user_version`, and includes raw `leases`, `runs`,
`output_files`, and `output_render_state` rows. Output rows include scoped ownership fields such as
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

`bindport open [service]` resolves the best active service URL from the
registry-wide snapshot. It prints `route_url` when configured, otherwise the
direct loopback `url`.

Examples:

```sh
bindport open web
bindport open web --project example
bindport open web --print
bindport open web --browser
```

Use `--project PROJECT` when multiple active services share the same service
name. Project filtering does not select the current worktree, so duplicate
active worktrees can remain ambiguous. For exact-worktree automation, filter
`status --json` by identity/worktree fields. `--browser` only launches HTTP or
HTTPS URLs; use `--print` in headless or machine workflows.

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

Machine-readable cleanup counts are available with `--json`. This report is
currently unversioned and is not status schema 1.0. Destructive cleanup can run
an approved lifecycle hook whose inherited stdout can contaminate the JSON;
`--dry-run` runs no hooks and is the safe parse-only preview:

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
