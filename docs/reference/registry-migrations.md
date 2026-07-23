# Registry Migration Policy

BindPort stores machine-local lease, run, route, health, and output ownership
state in SQLite. The registry is operational state, not project configuration.
Its default path is `$XDG_STATE_HOME/bindport/registry.sqlite`, or
`~/.local/state/bindport/registry.sqlite` when `XDG_STATE_HOME` is unset.

This policy applies to automatic registry upgrades performed when BindPort opens
that database. It is separate from the `status --json`, registry export, list
JSON, and configuration schema versions.

## Preservation Guarantee

For a supported upgrade to a v1 BindPort release, migration must not silently
delete an active or reserved lease, change its port, or replace its project,
service, worktree, or identity. Existing run history and route, health, and
output metadata available in the source schema are preserved. A field that did
not exist in an older schema is initialized to its documented empty/default
form; migration does not invent historical metadata.

The current registry uses SQLite `PRAGMA user_version = 9`. The tagged schemas
supported for automatic upgrade are:

| Source release | `user_version` | Material schema change represented |
|---|---:|---|
| v0.1.0 | 2 | Lease/run tables and worktree identity columns |
| v0.2.0 | 3 | Hostname and route URL columns |
| v0.3.0-v0.4.0 | 6 | Output ownership and render-state tables |
| v0.5.0 | 7 | Health URL column |
| v0.5.1, v0.6.0-v0.6.2 | 8 | Process start-time column; v0.6 can contain reservations |
| v0.7.0 and current v0.8 development | 9 | Worktree-scoped output ownership |

Versions 4 and 5 were intermediate development schemas, not shipped tag
schemas. BindPort's shape-based migration may be able to open them, but they are
not part of the published compatibility guarantee. A new empty database starts
at SQLite version 0 and is initialized directly to the current schema.

The v9 output migration preserves legacy output rows under the explicit
`unscoped` scope. Scope metadata that did not exist previously remains `null`;
it is not inferred from paths.

## Transaction And Concurrent Opens

Schema creation, additive columns, output-table conversion, indexes, row copies,
and the final `user_version` update run in one SQLite immediate transaction. If
any migration statement fails, SQLite rolls the transaction back: the prior
schema version and registry rows remain intact, and BindPort does not continue
with the partially migrated registry.

Multiple BindPort processes may open the same registry. SQLite's write lock and
BindPort's busy timeout serialize migration work. Waiting clients re-run the
idempotent schema checks after the first client commits; they do not duplicate
rows or repeat destructive table conversion.

## Unsupported Or Invalid Registries

BindPort reads `user_version` before enabling WAL or running schema changes. If
the value is newer than this binary supports, open fails with an explicit
unsupported-version error. BindPort does not lower the version, rewrite the
schema, or operate on that database. Install the same or a newer BindPort
version than the one that created it.

A malformed or partial older schema also fails open if it cannot complete the
supported migration transaction. BindPort does not guess at missing required
columns, discard rows, rebuild the registry from process observations, or claim
data recovery.

After any registry-open or migration error:

1. Stop other BindPort commands using that registry and keep the database in
   place.
2. Record the exact error and BindPort version.
3. Retry with the latest compatible BindPort release. Do not delete the
   registry merely to make the command start.
4. If the error persists, make a backup before manual SQLite investigation and
   include the error and source release in a bug report. Registry exports and
   database copies can contain local paths and full command lines; review them
   before sharing.

## Backups, Recovery, And Downgrades

BindPort does not create an automatic pre-migration backup. Users who need a
rollback point are responsible for one. Before upgrading, stop BindPort clients
and either run `bindport registry export` for a JSON debug/backup snapshot or
copy the SQLite database with a SQLite-aware backup method. If copying files
directly, stop writers and preserve any matching `-wal` and `-shm` files with
the database.

The export is useful for inspection but is not an automatic restore format.
This policy does not promise downgrade compatibility, automatic restoration,
repair of arbitrary SQLite corruption, or recovery of state that was already
missing before migration. Older BindPort binaries must not be used as a
rollback tool against a registry opened by a newer release.
