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
command = ["storybook", "dev"]
args = ["--port", "{port}", "--host", "0.0.0.0"]
hostname = "{branch}.orderful-website.localhost"
route_url = "http://{hostname}"
health_url = "{route_url}/health"
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

Hook settings:

```toml
[hooks]
timeout_ms = 5000

[[hooks.commands]]
name = "reload-proxy"
events = ["route_started", "route_finished", "routes_removed", "output_rendered"]
command = ["docker", "kill", "-s", "HUP", "traefik"]
timeout_ms = 2000
```

Hooks are disabled until explicitly trusted with the local CLI. A checked-in
project config can declare hooks, but config cannot approve hook execution by
itself. Use `bindport hooks status` to inspect pending, approved, denied, or
changed hooks, then approve a reviewed hook with:

```sh
bindport hooks trust reload-proxy
```

Trust decisions are stored in BindPort state outside the repository. The
default scope is the current worktree. Use `--scope repo` to trust the same hook
definition across worktrees that share the same git repository. A hook is
blocked again as `changed` when its command definition changes. If the command
target is a local path such as `./scripts/reload-proxy`, BindPort also
fingerprints that file and invalidates trust when it changes. Commands resolved
from `PATH`, such as `docker`, are treated as opaque targets.

Hook commands are structured argv arrays and are spawned directly, not through
a shell. Use `["sh", "-c", "..."]` only when shell behavior is intentional.
Supported events are `route_started`, `route_finished`, `routes_removed`,
`routes_marked_stale`, `render_requested`, and `output_rendered`. Hook
processes receive a minimal environment: `PATH` from the parent process plus
BindPort lifecycle metadata through `BINDPORT_HOOK_EVENTS`,
`BINDPORT_HOOK_SOURCES`, and `BINDPORT_HOOK_CONTEXT`. Other parent environment
values are not inherited, and secret values are not copied into hook metadata or
the registry.

Unknown top-level keys are reported by `bindport doctor`. Service-level
`health_url` is stored with each run. Active loopback `http://` health URLs are
probed by `status`; non-loopback and unsupported destinations stay `unknown`.
Literal loopback IPs, `localhost`, and `*.localhost` names are treated as local
targets without DNS resolution.

## Template Placeholders

Wrapped commands always receive `PORT=<assigned>`. Service `command`, `args`,
env, hostname, route URL, and health URL templates can use:

```text
{port}
{host}
{url}
{project}
{service}
{hostname}
{route_url}
{health_url}
{branch}
{branch_label}
{git_branch}
{worktree}
{worktree_label}
{worktree_hash}
```

Use `{{` and `}}` when a template value needs literal braces.
`bindport run --hostname TEMPLATE`, `--route-url TEMPLATE`, and
`--health-url TEMPLATE` override service config for one run. Wrapper scripts can
also set `BINDPORT_HOSTNAME`, `BINDPORT_ROUTE_URL`, or `BINDPORT_HEALTH_URL` to
override the matching service config value.

`command` and `args` are structured argv arrays. They are spawned directly, not
through a shell. When `bindport run <service>` is called without an explicit
`-- <command>`, BindPort expands those argv templates after allocation and runs
the configured command. Explicit child commands still override service command
config for one-off runs:

```sh
bindport run web
bindport run web -- next dev
```

## Validation

Run these after changing config:

```sh
bindport config explain
bindport config validate
bindport doctor outputs
```

`config validate` catches missing or duplicate service names, unsafe service
paths, output configuration errors, and incomplete hook configuration. `doctor
outputs` validates template lookup, render planning, safe output paths, output
ownership checks, and hook trust status without writing files.

## Examples

- [Starter TOML/JSON/YAML examples](../examples/config)
- [Monorepo config guide](monorepos.md)
- [Two-service monorepo fixture](../examples/monorepo)
