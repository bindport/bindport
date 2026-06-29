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

Wrapped command start and exit events automatically render outputs where
`auto_render = true`, which is the default. The start render records the active
route after the child process is spawned; the exit render records the stopped
route after the registry is updated. Auto-render failures are warnings and do
not change the wrapped command's exit code.

Relative `root` values are resolved beside the discovered project config. If no
project config is discovered, they resolve from the current working directory.
Relative targets must stay under the output root and may not traverse through
symlinks. Absolute roots are accepted after path cleanup, but target paths are
always relative text file paths.

Automatic rendering for registry cleanup and deletion for removed routes are
later v0.3 output slices.

## MiniJinja Behavior

BindPort uses MiniJinja with strict undefined placeholders and autoescaping
disabled. That means missing values are errors, and templates must quote or
escape their own target format correctly.
