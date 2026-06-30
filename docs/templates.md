# Output Templates

BindPort resolves output templates by logical name. Template commands let
projects inspect, export, and customize templates, and `bindport render` writes
configured text output files from the current registry snapshot.

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

For `template = "bindport-traefik"`, BindPort checks the first matching file:

1. project `.bindport/templates/bindport-traefik`
2. project `.bindport/templates/bindport-traefik.j2`
3. project `.bindport/templates/bindport-traefik.*.j2`
4. global `$XDG_CONFIG_HOME/bindport/templates/bindport-traefik`
5. global `$XDG_CONFIG_HOME/bindport/templates/bindport-traefik.j2`
6. global `$XDG_CONFIG_HOME/bindport/templates/bindport-traefik.*.j2`
7. built-in `bindport-traefik`

Project templates live beside the discovered project config. If no project
config is discovered, the project template directory is the current working
directory's `.bindport/templates`.

Wildcard matches are sorted lexicographically by full filename and the first
match wins. Templates are UTF-8 text.

## Built-In Traefik Template

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

## Traefik File Provider Setup

BindPort does not run Traefik. Point an existing Traefik file provider at the
directory where BindPort writes generated route files, and let Traefik reload
when files change.

Example BindPort config for a project that wants branch-scoped local hostnames:

```toml
project = "orderful-website"

[[services]]
name = "web"
hostname = "{branch}.orderful-website.localhost"
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
host such as `feature-tree.orderful-website.localhost`, and Traefik receives an
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

Use a project-relative `root` in committed config. Put machine-specific absolute
paths, container mount paths, or Docker-specific target hosts in
`.bindport.local.toml`, which should stay untracked.

## Output Files

Each enabled `[[outputs]]` entry provides a template and a target path template:

```toml
[output_defaults]
root = ".bindport/generated"

[[outputs]]
name = "traefik"
template = "bindport-traefik"
target = "traefik/{{ route.service }}.yml"
```

`bindport render` reads the latest route state from the registry, renders one
text file per route, and records ownership in the registry after a successful
write. Existing files are overwritten only when BindPort previously rendered
the same output file and the on-disk content still matches the recorded hash.
Unowned or externally modified files cause the render to fail instead of being
overwritten.

`bindport render --repair` uses the same safety checks, but treats an
externally modified DB-owned file as state to record instead of a command-wide
failure. Current route files that are missing are rendered again. DB-owned files
for removed or configured deletion states are deleted only when their current
hash still matches the registry record. Missing DB-owned files are marked
removed, and externally modified DB-owned files are preserved and marked with
`external_modified`. Unknown files are never adopted.

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

`bindport doctor outputs` checks the same configured outputs, template lookup,
target planning, and output path safety without writing files or recording
ownership.

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
`bindport render` and `bindport render --repair` bypass debounce.

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
Relative targets must stay under the output root and may not traverse through
symlinks. Absolute roots are accepted after path cleanup, but target paths are
always relative text file paths.

CLI and dashboard registry cleanup trigger output rendering so the default
`delete_on = ["removed"]` behavior can remove DB-owned files for routes that
were just cleaned from the registry.

## MiniJinja Behavior

BindPort uses MiniJinja with strict undefined placeholders and autoescaping
disabled. That means missing values are errors, and templates must quote or
escape their own target format correctly.

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
- Run `bindport status --json` and inspect `outputs` plus per-service
  `services[].outputs` when a generated file is missing or preserved.
- If Traefik renders comment-only YAML, confirm the route is active and has
  `hostname` metadata. Stopped, stale, and missing-hostname routes intentionally
  render no live router.
- If Traefik cannot reach the service, check `target_host`. Host Traefik usually
  works with `127.0.0.1`; containerized Traefik often needs
  `host.docker.internal` or an equivalent host gateway name.
- If BindPort refuses to overwrite a file, the file is unowned or externally
  modified. Use `bindport render --repair` to record externally modified
  DB-owned files without adopting unknown files.
- If cleanup does not delete a generated file, confirm the route was removed
  from the registry and that `delete_on` includes the lifecycle state you expect.
