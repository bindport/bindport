# Status And URL Lookup

`bindport status --json` returns the local registry snapshot. The top-level
`schema_version` is currently `0.4`; pre-1.0 releases may extend the schema, but
existing fields should remain stable within a major version. The checked-in
JSON Schema for the current payload is [status.schema.json](status.schema.json).

Top-level fields:

- `schema_version`: status schema version.
- `generated_at`: registry read timestamp.
- `outputs`: aggregate output-file counts grouped by output name.
- `services`: latest service records grouped by BindPort identity.
- `runs`: run history, newest first.
- `hooks`: configured hook trust visibility for the current directory.

Service fields most useful to agents:

- `project`, `service`, `identity_key`: stable service identity.
- `state`: `active`, `stopped`, or `stale`.
- `host`, `port`, `url`: direct loopback URL for the wrapped process.
- `hostname`, `route_url`, `health_url`: configured route metadata when present.
- `health`: `unknown`, `pending`, `healthy`, or `failing`.
- `branch`, `branch_label`, `worktree_path`, `commit`: git context when known.
- `outputs`, `proxy`: generated output files and proxy-oriented summary.

`bindport open [service]` resolves the best active service URL from the same
snapshot. It prints `route_url` when configured, otherwise the direct loopback
`url`. Use `--project PROJECT` when multiple active services share the same
service name. `--browser` only launches HTTP or HTTPS URLs.

Examples:

```sh
bindport status --json
bindport open web
bindport open web --project example
bindport open web --browser
```
