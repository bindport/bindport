# Output Templates

BindPort resolves output templates by logical name. Template commands let
projects inspect, export, and customize templates, and `bindport render` writes
configured text output files from the current registry snapshot.

For practical proxy setup with Traefik, Caddy, Docker Desktop, Rancher Desktop,
or no proxy at all, see [Proxy Outputs](proxy-outputs.md).

## Commands

List templates:

```sh
bindport templates list
```

Show a template with source metadata:

```sh
bindport templates show bindport-traefik
```

Export only the template body, suitable for redirecting into a project template
file:

```sh
bindport templates export --source built-in bindport-traefik
```

Use `--source project`, `--source global`, or `--source built-in` to bypass the
normal first-match lookup and read only one source.

Render every enabled output:

```sh
bindport render
```

Render one output by name:

```sh
bindport render traefik
```

Preview rendered targets without writing files:

```sh
bindport render --dry-run
```

Preview content changes against DB-owned files without writing files:

```sh
bindport render --diff
```

Repair DB-owned output records and files:

```sh
bindport render --repair
bindport render --repair traefik
```

Validate configured outputs, template lookup, render planning, and safe output
paths without writing files:

```sh
bindport doctor outputs
```

## Lookup Order

Template names are logical names, not filesystem paths. Names must be safe
relative names with no path separators, no absolute path syntax, and no `..`.

For `template = "<name>"`, BindPort checks the first matching file:

1. project `.bindport/templates/<name>`
2. project `.bindport/templates/<name>.j2`
3. project `.bindport/templates/<name>.*.j2`
4. global `$XDG_CONFIG_HOME/bindport/templates/<name>`
5. global `$XDG_CONFIG_HOME/bindport/templates/<name>.j2`
6. global `$XDG_CONFIG_HOME/bindport/templates/<name>.*.j2`
7. built-in template, when the name is one of BindPort's built-ins

Project templates live beside the discovered project config. If no project
config is discovered, the project template directory is the current working
directory's `.bindport/templates`.

Wildcard matches are sorted lexicographically by full filename and the first
match wins. Templates are UTF-8 text.

## Built-In Templates

### `bindport-traefik`

The first built-in template is `bindport-traefik`. It is a MiniJinja text
template for Traefik's file provider and uses the same lookup/export path as
custom templates.

Supported vars:

```toml
[outputs.vars]
entrypoints = ["web"]
tls = false
middlewares = []
```

For an active route with a hostname, the template renders Traefik routers and
services pointing at `route.target_url`. For stopped, stale, or missing-hostname
routes, it renders comment-only YAML.

### `bindport-caddy`

The `bindport-caddy` built-in renders Caddyfile site blocks for Caddy's file
adapter or an imported Caddyfile directory.

Supported vars:

```toml
[outputs.vars]
site_scheme = "http"
```

For an active route with a hostname, the template renders a site address and
`reverse_proxy` directive pointing at `route.target_url`. The default
`site_scheme = "http"` keeps local `.localhost` setups from opting into Caddy
automatic HTTPS by accident. Export the template and change `site_scheme` when
your local Caddy setup intentionally owns HTTPS.

### `bindport-json-snapshot`

The `bindport-json-snapshot` built-in renders one JSON file for the current
route snapshot. It is useful for tools that can watch a file but should not poll
`bindport status --json`.

Example config:

```toml
[[outputs]]
name = "routes-json"
template = "bindport-json-snapshot"
root = ".bindport/generated"
target = "routes.json"
```

The generated document has:

- `snapshot.generated_at`: when the registry snapshot was captured.
- `snapshot.route_count`: number of routes rendered into the snapshot.
- `routes`: an array using the same route fields available to per-route
  templates, including `project`, `service`, `state`, `port`, `hostname`,
  `route_url`, `target_url`, branch/worktree fields, and command metadata.

Unlike normal output templates, `bindport-json-snapshot` renders once for the
whole snapshot. Its `target` template receives `snapshot.*`, `routes`,
`output.*`, and `vars.*`, but not a per-route `route.*` value.

### `bindport-env-local`

The `bindport-env-local` built-in renders route metadata as a dotenv-style text
file. It exists only as an output template; BindPort never writes `.env.local`
unless a project explicitly configures an output for it.

Example monorepo config:

```toml
[[outputs]]
name = "env-local"
template = "bindport-env-local"
root = "."
target = "apps/{{ route.service }}/.env.local"
```

The generated file includes stable BindPort variables such as `PORT`,
`BINDPORT_PROJECT`, `BINDPORT_SERVICE`, `BINDPORT_STATE`,
`BINDPORT_TARGET_URL`, and route hostname fields when configured. Project
templates can shadow `bindport-env-local` when a framework needs aliases such as
`NEXT_PUBLIC_*`.

Like every output, `.env.local` files are written through the SQLite-backed
ownership checks. Existing unowned files are not overwritten. Automatic output
rendering happens after route state changes are recorded, so startup-critical
environment should still use configured service `command`, `args`, `env`, or
`bindport run --env`; use the env-file output for tools that reread files or for
explicit `bindport render` workflows. Set `root = "."` when shared output
defaults point generated config files somewhere else and the env file should
land in a package directory.

## Traefik File Provider Setup

BindPort does not run Traefik. Point an existing Traefik file provider at the
directory where BindPort writes generated route files, and let Traefik reload
when files change.

Example BindPort config for a project that wants branch-scoped local hostnames:

```toml
project = "example-web"

[[services]]
name = "web"
hostname = "{branch}.example-web.localhost"
health_url = "{route_url}/health"
env.PORT = "{port}"
env.HOSTNAME = "0.0.0.0"
env.NEXT_PUBLIC_BINDPORT_URL = "{route_url}"

[output_defaults]
root = ".bindport/generated"
target_host = "127.0.0.1"
target_scheme = "http"

[[outputs]]
name = "traefik"
template = "bindport-traefik"
target = "traefik/{{ route.slug }}.yml"

[outputs.vars]
entrypoints = ["web"]
tls = false
middlewares = []
```

With that config, a branch named `feature/tree` for service `web` can render a
host such as `feature-tree.example-web.localhost`, and Traefik receives an
upstream target like `http://127.0.0.1:29123`.

If Traefik runs in a container and needs to reach the host machine instead of
its own loopback device, set the output target host in local config:

```toml
[output_defaults]
target_host = "host.docker.internal"
```

Then mount or otherwise expose the generated directory to Traefik and configure
Traefik's file provider to watch that directory. For example:

```yaml
providers:
  file:
    directory: /path/to/project/.bindport/generated/traefik
    watch: true
```

Keep `root` project-relative in both committed config and local overrides.
Machine-specific values such as Docker target hosts belong in
`.bindport.local.toml`, which should stay untracked. Configure the proxy or
container mount to read the generated project-relative directory.

## Output Files

Each enabled `[[outputs]]` entry provides a template and a target path template:

```toml
[output_defaults]
root = ".bindport/generated"

[[outputs]]
name = "traefik"
template = "bindport-traefik"
target = "traefik/{{ route.service }}.yml"

[[outputs]]
name = "caddy"
template = "bindport-caddy"
target = "caddy/{{ route.service }}.caddy"

[[outputs]]
name = "routes-json"
template = "bindport-json-snapshot"
target = "routes.json"
```

`bindport render` reads the latest route state from the registry, renders text
files, and records ownership in the registry after a successful write. Most
templates render one file per route; `bindport-json-snapshot` renders one file
for the whole route snapshot. Existing files are overwritten only when BindPort
previously rendered the same output file and the on-disk content still matches
the recorded hash. Unowned or externally modified files cause the render to
fail instead of being overwritten.

Ownership is scoped to the resolved output root and config root. This lets two
worktrees or monorepo checkouts render the same output name and route key into
different `.bindport/generated` directories without replacing each other's
registry rows. Legacy unscoped rows from older BindPort versions are considered
only when their recorded path is inside the current output root, so same-root
files can be adopted while deleted-worktree or foreign-root rows do not block
current renders.

Templates render from a single route snapshot. Each rendered file receives:

- `snapshot.generated_at`: when the registry snapshot was captured.
- `snapshot.route_count`: number of routes in that snapshot after lifecycle
  deletion filtering.
- `route.*`: route metadata for the file being rendered, such as
  `route.project`, `route.service`, `route.state`, `route.port`,
  `route.hostname`, `route.route_url`, `route.target_url`, `route.branch_label`,
  `route.slug`, and `route.unique_slug`.
- `output.*`: the output config context, such as `output.name`,
  `output.template`, `output.root`, `output.target`, `output.auto_render`,
  `output.delete_on`, and `output.on_failure`.
- `vars.*`: user-defined values from `[outputs.vars]` for the current output.

Avoid embedding `snapshot.generated_at` unless the output should change on
every render. A changing timestamp changes the file hash, which can trigger
proxy reloads or hook events even when route data is otherwise unchanged.

`bindport render --diff` uses the normal render plan and ownership checks, then
prints added, modified, removed, and unchanged counts plus content hunks for
changed files. It does not write files, delete lifecycle-managed files, update
registry ownership rows, or execute hooks. Approved hooks that would match the
render request are printed in dry-run mode.

`bindport render --repair` uses the same safety checks, but treats recoverable
filesystem drift as state to record instead of a command-wide failure. Current
route files that are missing are rendered again. Content-identical planned files
whose ownership row was lost are adopted back into the registry without
rewriting them. DB-owned files for removed or configured deletion states are
deleted only when their current hash still matches the registry record. Missing
DB-owned files in the current output scope are marked removed, current-scope
rows outside the current output root are marked removed with
`outside_output_root`, and externally modified DB-owned files are preserved and
marked with `external_modified`.
Unknown files with different content are never adopted.

`delete_on` controls when DB-owned output files are removed. The default is
`["removed"]`, which deletes a rendered file after the matching route has been
removed from the registry and cleanup triggers output rendering. Users can opt
into earlier cleanup:

```toml
[[outputs]]
name = "traefik"
template = "bindport-traefik"
target = "traefik/{{ route.service }}.yml"
delete_on = ["stopped", "stale", "removed"]
```

Deletion is conservative: BindPort removes only files recorded in SQLite as
rendered output files, and only when the current on-disk hash matches the
recorded hash. Missing files are marked removed. Externally modified files are
preserved and marked as output errors.

`bindport doctor outputs` checks the same configured outputs, target host
syntax, resolved output roots, template lookup, target planning, output path
safety, and ownership rows without writing files or recording ownership.

Wrapped command start and exit events automatically render outputs where
`auto_render = true`, which is the default. The start render records the active
route after the child process is spawned; the exit render records the stopped
route after the registry is updated. Render triggers flow through an internal
route-event collector with source tags for `cli_runner`, `cli_clean`,
`dashboard_clean`, `manual_render`, and `stale_reconcile`. The collector is not a
public API, but keeps local CLI and dashboard actions on the same path for later
trusted automation. Automatic renders reserve a SQLite-backed debounce slot per
output. The default `debounce_ms = 250` spaces rapid events; set
`debounce_ms = 0` to render immediately on every automatic event. Manual
`bindport render`, `bindport render --diff`, and `bindport render --repair`
bypass debounce.

For render troubleshooting, pass `--verbose` to manual render commands. The
diagnostics are printed to stderr and include the selected output, resolved
template source, output root/scope, route count, planned file count, ownership
row count, and lifecycle cleanup summary. For automatic renders triggered by
`bindport run`, `clean`, or dashboard cleanup, set `BINDPORT_LOG=debug` in the
environment. Debug logs intentionally avoid hook environment payloads and child
process environment values.

Hooks subscribe to the same lifecycle events as output rendering. Approved
hooks can run after route start, route finish, CLI or dashboard cleanup,
stale-route reconciliation, manual render requests, or successful output
renders. Hooks run even when no output is configured, which lets a project use
BindPort as a small local event bridge without forcing a generated file.

Hook execution is intentionally disabled by default. Put hook definitions in
checked-in config when a team should share the same event commands, then review
and approve them on each machine with `bindport hooks trust <name>` or
`bindport hooks trust --all`. The default trust scope is the current worktree;
use `--scope repo` when the same reviewed hook should apply across worktrees
that share a git repository. `bindport hooks status` shows whether each hook is
pending, approved, denied, or changed since the last decision. Trust decisions
live in BindPort state outside the repository, and local path command targets
are fingerprinted so edits to scripts such as `./scripts/reload-proxy` require
re-approval.

`bindport render --dry-run` prints matching approved hooks without executing
them, and `bindport doctor outputs` shows hook source, trust, events, command,
timeout, target fingerprint summary, and redacted BindPort hook environment
keys.

Auto-render failures are warnings and do not change the wrapped command's exit
code by default. Set `on_failure = "block"` on an output when startup should
fail if BindPort cannot validate the required output plan before spawning the
child process. The blocking check renders the pending route in memory and
verifies template lookup, target rendering, path safety, target collisions, and
existing DB-owned file hashes. Post-spawn, exit, and cleanup render failures are
still warnings because BindPort does not roll back already-running processes or
completed lifecycle cleanup.

`bindport status --json` exposes top-level output summaries plus per-service
output status from the same registry records. The legacy `proxy` field is a
compatibility alias for recorded `traefik` output status.

Relative `root` values are resolved beside the discovered project config. If no
project config is discovered, they resolve from the current working directory.
Roots must be relative and must not contain `..`. Targets must stay under the
output root, must be relative text file paths, and may not traverse through
symlinks.

CLI and dashboard registry cleanup trigger output rendering so the default
`delete_on = ["removed"]` behavior can remove DB-owned files for routes that
were just cleaned from the registry.

## MiniJinja Behavior

BindPort uses MiniJinja with strict undefined placeholders and autoescaping
disabled. That means missing values are errors, and templates must quote or
escape their own target format correctly.

Rendered output is limited to 1 MiB per file. Templates also run with a bounded
MiniJinja fuel budget so accidental runaway templates fail instead of hanging a
render.

## Custom Templates

Export the built-in template when you want a project-local starting point:

```sh
mkdir -p .bindport/templates
bindport templates export --source built-in bindport-traefik \
  > .bindport/templates/my-traefik.yml.j2
```

Then point an output at the new logical template name:

```toml
[[outputs]]
name = "traefik"
template = "my-traefik"
target = "traefik/{{ route.slug }}.yml"
```

Custom templates receive the same `route`, `output`, and `vars` context as the
built-in template. They are text-only and are resolved by logical name, so keep
template files under project `.bindport/templates` or global
`$XDG_CONFIG_HOME/bindport/templates`.

## Troubleshooting

- Run `bindport doctor outputs` before starting a wrapped command. It checks
  template lookup, target rendering, path safety, target collisions, and
  wildcard-template ambiguity without writing files.
- Run `bindport render --dry-run` to see planned files without touching disk.
- Run `bindport render --diff` to inspect content changes against DB-owned
  files before overwriting or deleting them.
- Run `bindport status --json` and inspect `outputs` plus per-service
  `services[].outputs` when a generated file is missing or preserved.
- If Traefik renders comment-only YAML, confirm the route is active and has
  `hostname` metadata. Stopped, stale, and missing-hostname routes intentionally
  render no live router.
- If Traefik cannot reach the service, check `target_host`. Host Traefik usually
  works with `127.0.0.1`; containerized Traefik often needs
  `host.docker.internal` or an equivalent host gateway name.
- If `doctor outputs` reports foreign or stale ownership rows, the registry has
  output records for the same output name from another output root or worktree.
  Those rows are diagnostic context and do not block current-scope rendering.
- If BindPort refuses to overwrite a file, the file is unowned or externally
  modified. Use `bindport render --repair` to adopt content-identical planned
  files whose ownership row was lost, or to record externally modified DB-owned
  files without overwriting them.
- If render reports `outside_output_root`, a stale ownership row pointed at a
  generated file outside the current output root, usually from a deleted
  worktree or old output location. Repair or the next render marks that row
  removed instead of deleting files outside the current root.
- If cleanup does not delete a generated file, confirm the route was removed
  from the registry, that stale CLI cleanup was confirmed with `--yes` when run
  noninteractively, and that `delete_on` includes the lifecycle state you expect.
