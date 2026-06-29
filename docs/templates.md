# Output Templates

BindPort resolves output templates by logical name. Template commands are
available before the file-rendering pipeline so projects can inspect, export,
and customize templates without writing generated proxy config yet.

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
routes, it renders comment-only YAML. File rendering, ownership records, and
automatic route-state updates are implemented in later v0.3 output slices.

## MiniJinja Behavior

BindPort uses MiniJinja with strict undefined placeholders and autoescaping
disabled. That means missing values are errors, and templates must quote or
escape their own target format correctly.
