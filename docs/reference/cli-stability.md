# CLI Stability Contract

This page is the canonical BindPort CLI stability contract. BindPort v0.8
treats the command surface below as the candidate for the v1 freeze. The names
and machine interfaces are candidates for compatibility; incidental human
wording, whitespace, table layout, diagnostics, and completion ordering are not.

## v1-Candidate Public Surface

The following documented spellings are public candidates. Values in angle
brackets are arguments, not literal command names.

| Surface | Public command and flag names |
|---|---|
| Global | `bindport`, `--help`, `-h`, `--version`, `-V`, and `--` before a wrapped command |
| Run | `run [service]`, `--env`, `--hostname`, `--route-url`, `--health-url`, and `--` |
| Reserve | `reserve [service]`, `reserve --all`, `--hostname`, `--route-url`, and `--health-url` |
| Release | `release [service\|port]` |
| Inspect | `status [--json]`, `list [--json]`, `registry export`, `open [service]`, `open --project`, `open --print`, `open --browser`, `port <service>`, and `port --project` |
| Cleanup | `clean`, `--dry-run`, `--stopped`, `--stale`, `--all`, `--json`, `--yes`, and `-y` |
| Config | `init`, `init --project`, `init --user`, `config explain`, and `config validate` |
| Diagnostics | `doctor` and `doctor outputs` |
| Dashboard | `dashboard [serve]`, `dashboard start`, `dashboard status`, `dashboard stop`, `--host`, `--port`, `--auth`, `--auth-required`, `--no-auth`, `--register-service`, `--no-register-service`, `--token`, `--token-env`, `--allowed-host`, and `--static-dir` |
| Outputs | `render [output]`, `--all`, `--dry-run`, `--diff`, `--repair`, `--verbose`, `-v`, `templates list`, `templates show`, `templates export`, and `--source project\|global\|built-in` |
| Hooks | `hooks status`, `hooks trust`, `hooks deny`, `hooks reset`, `--scope worktree\|repo`, and `--all` |

The table freezes documented names, not every permissive parser edge case or
undocumented value alias. At v1, existing documented invocations also retain
compatible flag argument requirements and meanings; a flag will not silently
change from inspection/preview to a destructive action. Additive commands and
flags may be introduced without breaking this contract.

Before v1, a removal or rename must be called out in the release notes and this
page with the replacement and earliest removal release. After v1, an existing
spelling remains accepted through the current major release after deprecation;
removal requires the next major release. When an alias is practical, help and
diagnostics will direct users to the replacement during that window.

## Exit Status

BindPort has no separately reserved range of numeric exit codes.

| Case | Exit behavior |
|---|---|
| No arguments, `--help`/`-h`, or `--version`/`-V` | Prints to stdout and exits `0`. The version line is `bindport <version>`. |
| Implemented subcommand help | `reserve`, `release`, `list`, `registry`, `registry export`, `open`, `port`, `clean`, `config`, `doctor`, `dashboard`, `hooks`, `render`, `templates`, and `init` accept their documented `--help`/`-h` form, print to stdout, and exit `0`. Use top-level `bindport --help` as the uniform stable entry point. |
| Unknown command, invalid arguments, rejected non-interactive cleanup, or fatal config/registry/render/spawn/startup failure | Prints a diagnostic and exits `1`. |
| Wrapped child exits normally | On supported Unix platforms, returns the child's `0`-`255` exit code unchanged, including `0` and `1`. |
| Wrapped child terminates from a Unix signal | Records and returns the conventional `128 + signal` status, such as `130` for SIGINT or `143` for SIGTERM. BindPort forwards SIGINT and SIGTERM to the child on supported Unix platforms. This numeric convention is not a portable signal identity on every shell or operating system. |

Because both BindPort-owned failures and a child can return `1`, callers cannot
classify the source from the number alone. Use stderr and whether the child was
started when that distinction matters. BindPort does not currently enforce a
claim that any numeric code is reserved from child passthrough.

Ordinary runs can continue without registry recording when opening or updating
the registry fails; BindPort warns on stderr and still returns the child status.
Operations that require registry truth, including status, lookup, reservations,
reserved promotion, sibling resolution, and blocking output preflight, fail
with `1` instead. A reserved-promotion failure terminates the spawned child and
returns a BindPort-owned failure.

Post-start auto-render, hook, and non-required registry-recording failures are
warnings and do not replace the wrapped child's exit status. Before a child is
spawned, invalid config, missing sibling data, an unavailable reserved port, a
blocking output failure, or a spawn failure returns `1`.

Wrapped children inherit stdin, stdout, and stderr. Their output can therefore
mix with BindPort warnings; wrapper mode does not reserve stdout as a machine
channel.

`run --help` is not a defined run option and currently returns a usage failure.
`status --help` is not a status-help mode; the status dispatcher currently
ignores trailing arguments other than detecting `--json`. Those permissive or
non-uniform parser edges are not public candidate behavior and may be tightened
without a compatibility window.

## Machine Output

Prefer these modes over parsing human tables or diagnostic prose. On success,
JSON and scalar payloads are written to stdout with a trailing newline. Fatal
machine-command diagnostics are written to stderr and produce a nonzero status.
Warnings can also appear on stderr, so automation should parse stdout only after
a successful exit and should not require stderr to be empty.

| Command | Guarantee |
|---|---|
| `status --json` | JSON status schema exactly `1.0`. Its checked-in contract and compatibility rules are in [Status and Cleanup](../operations/status.md) and [`status.schema.json`](../status.schema.json). A config-resolution problem for hook visibility is represented as `hooks.error` in otherwise valid JSON. |
| Dashboard `GET /api/status` | The same status 1.0 payload as `status --json`, subject to dashboard access controls. It is not a separately versioned schema. |
| `list --json` | Valid grouped-inventory JSON with its own current `schema_version` of `0.1`. It is not status schema 1.0 and has no separate v1 field-freeze promise. Consumers must inspect its version. |
| `registry export` | Valid debug/backup JSON with its own current `schema_version` of `0.1` plus SQLite `user_version`. It is not status schema 1.0, a restore format, or a stable raw-database API. |
| `clean --json` | BindPort emits one currently unversioned JSON object after any required confirmation: boolean `dry_run`, integer `leases` and `runs`, and integer `states.stopped`/`states.stale`. It has no status-schema compatibility promise. Destructive cleanup can run an approved lifecycle hook whose inherited stdout can contaminate this stream; `--dry-run` executes no hooks and remains the safe parse-only preview. Use `--yes` only after authorizing stale cleanup. |
| `port <service> [--project PROJECT]` | Exactly one decimal port followed by `\n` for one active or reserved match in the current worktree identity. Missing, stopped, stale, cross-worktree, and ambiguous matches fail without a fallback value. |
| `open [service] --print` | One selected active service URL followed by `\n`. Selection is registry-wide, optionally filtered by project; it is not current-worktree scoped. `--browser` may add launcher output and is not the automation mode. |
| `--version` | `bindport <version>\n`. |
| `templates export ...` | Raw resolved template contents, with no added trailing newline. Built-in template content is allowed to evolve and is not a schema contract. |

No JSON object-member or array ordering is guaranteed, including where the
current implementation happens to sort rows. Select by documented identity,
name, or ID and sort explicitly when order matters.

`status` without `--json`, `list` without `--json`, `config explain`, `config
validate`, `hooks status`, `dashboard status`, reserve/release output, render
reports, template list/show output, help, and diagnostics are human interfaces.
Their prose and whitespace can change within v1. In particular, config
explain/validate report their human result on stdout even when validation
returns a nonzero status; no config or hooks JSON mode currently exists.

## Safe Agent And Script Use

`status --json` and `list --json` are registry-wide. They can contain records
from unrelated projects and multiple worktrees. Match `identity_key` when
available, or match project, service, and exact `worktree_path`/`worktree_hash`;
do not select the first array entry.

Use this non-interactive flow from the intended project/worktree directory:

```sh
bindport config validate
bindport reserve --all
bindport port web
bindport status --json
bindport open web --print
```

- Use `port` for an exact current-worktree active or reserved port.
- Use `status --json` for exact current-worktree URL selection when duplicate
  project/service names can exist. `open --project` narrows only by project and
  can still be ambiguous across worktrees.
- Use `open --print`, never `--browser`, in headless automation.
- Treat reservations as registry coordination, not readiness or socket
  ownership.
- Use `clean --dry-run --json` before cleanup. Non-interactive stale cleanup
  requires the explicit `--yes` authorization.
- Never approve, deny, or reset hook trust without user authorization.
- Do not parse human output or edit the SQLite registry directly.
