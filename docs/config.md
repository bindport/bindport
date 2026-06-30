# Configuration

BindPort configuration is optional. Missing config means built-in defaults are
used, and wrapped commands still receive an assigned `PORT`.

## Files And Precedence

BindPort walks upward from the current directory and uses the first project
config it finds. In a directory with equivalent config files, TOML wins over
JSON, and JSON wins over YAML.

Project config names:

```text
.bindport.toml
.bindport.json
.bindport.yaml
```

Machine-local overrides live beside the project config and should stay
untracked:

```text
.bindport.local.toml
.bindport.local.json
.bindport.local.yaml
.bindport.local.yml
bindport.local.toml
bindport.local.json
bindport.local.yaml
bindport.local.yml
```

If no project config exists, BindPort falls back to the optional user config at
`$XDG_CONFIG_HOME/bindport/config.toml`, or `~/.config/bindport/config.toml`
when `XDG_CONFIG_HOME` is unset. `bindport init` creates that fallback config.

Runtime identity precedence is:

1. CLI service argument, such as `bindport run web -- ...`
2. environment variables, such as `BINDPORT_PROJECT` and `BINDPORT_SERVICE`
3. project config plus local override config
4. inference from package metadata, workspace roots, git worktree, and command
   name

`bindport config explain` shows the discovered files, field sources, and the
resolved project/service identity for the current directory.

## Supported Fields

Current top-level fields:

```toml
project = "orderful"
service = "web"
default_range = "29000-29999"
skip_ports = [29000, 29070]
```

Service entries:

```toml
[[services]]
name = "web"
path = "apps/web"
hostname = "{branch}.orderful-website.localhost"
route_url = "http://{hostname}"
env.PORT = "{port}"
env.HOSTNAME = "0.0.0.0"
env.NEXT_PUBLIC_BINDPORT_URL = "{route_url}"
```

Dashboard settings:

```toml
[dashboard]
host = "127.0.0.1"
port = 27080
register_service = false
allowed_hosts = ["localhost", "127.0.0.1"]

[dashboard.auth]
required = false
token_env = "BINDPORT_DASHBOARD_TOKEN"
```

Output settings:

```toml
[output_defaults]
root = ".bindport/generated"
target_host = "127.0.0.1"
target_scheme = "http"
auto_render = true
delete_on = ["removed"]
on_failure = "warn"
debounce_ms = 250

[[outputs]]
name = "traefik"
template = "bindport-traefik"
target = "traefik/{{ route.slug }}.yml"

[outputs.vars]
entrypoints = ["web"]
tls = false
middlewares = []
```

Unknown top-level keys are reported by `bindport doctor`. Some example files may
show intended future fields such as `identity`, `command`, or `health_url`; they
are not applied by the current runtime.

## Template Placeholders

Wrapped commands always receive `PORT=<assigned>`. Service env, hostname, and
route URL templates can use:

```text
{port}
{host}
{url}
{project}
{service}
{hostname}
{route_url}
{branch}
{branch_label}
{git_branch}
{worktree}
{worktree_label}
{worktree_hash}
```

Use `{{` and `}}` when a template value needs literal braces.

## Validation

Run these after changing config:

```sh
bindport config explain
bindport config validate
bindport doctor outputs
```

`config validate` catches missing or duplicate service names, unsafe service
paths, and output configuration errors. `doctor outputs` validates template
lookup, render planning, safe output paths, and output ownership checks without
writing files.

## Examples

- [Starter TOML/JSON/YAML examples](../examples/config)
- [Monorepo config guide](monorepos.md)
- [Two-service monorepo fixture](../examples/monorepo)
