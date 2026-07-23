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
when `XDG_CONFIG_HOME` is unset. `bindport init` creates a minimal project
`.bindport.toml` in the current directory. `bindport init --user` creates the
optional user fallback config.
YAML config is supported for simple documents only: anchors and aliases are
rejected, and YAML config files have a 256 KiB size limit. TOML remains the
reference format.

Runtime identity precedence is:

1. CLI service argument, such as `bindport run web -- ...`
2. environment variables, such as `BINDPORT_PROJECT` and `BINDPORT_SERVICE`
3. project config plus local override config
4. inference from package metadata, workspace roots, git worktree, and command
   name

`bindport config explain` shows the discovered files, field sources, and the
resolved project/service identity for the current directory.

## Initialize Config

For a project repo:

```sh
bindport init
```

This writes `.bindport.toml` in the current directory and avoids absolute paths,
secrets, and machine-local values. Commit this file when it describes the shared
project setup. Put developer-specific values in `.bindport.local.toml` or another
local override file.

For a user fallback config:

```sh
bindport init --user
```

The fallback config is optional and only applies when no project config is
discovered.

## Supported Fields

The complete machine-readable v1-candidate shape is
[`config.schema.json`](../config.schema.json). See
[Configuration Stability](../reference/config-stability.md) for the freeze and
deprecation policy. This is the candidate contract for v1, not a claim that the
v1 freeze has already happened.

Current top-level fields:

```toml
project = "example"
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
hostname = "{branch}.example-web.localhost"
route_url = "http://{hostname}"
health_url = "{route_url}/health"
env.PORT = "{port}"
env.HOSTNAME = "0.0.0.0"
env.NEXT_PUBLIC_BINDPORT_URL = "{route_url}"
```

Service `env` is for application-level values. Config cannot set
execution-sensitive names that can change child process loading or tool config,
including `PATH`, `LD_PRELOAD`, `LD_LIBRARY_PATH`, `DYLD_*`, `NODE_OPTIONS`,
`BASH_ENV`, `ENV`, language package path variables, shell path variables, and
`GIT_CONFIG_*`. Pass those explicitly with `bindport run --env NAME=value` when
a one-off run really needs them.

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
# token = "machine-local-token" # accepted, but prefer token_env
```

If `token` is used, keep it in an untracked machine-local override. Prefer
`token_env` so the credential does not live in config.

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
enabled = true
name = "traefik"
template = "bindport-traefik"
root = ".bindport/generated"
target = "traefik/{{ route.slug }}.yml"
target_host = "127.0.0.1"
target_scheme = "http"
auto_render = true
delete_on = ["removed"]
on_failure = "warn"
debounce_ms = 250

[outputs.vars]
entrypoints = ["web"]
tls = false
middlewares = []
```

Output `root` values must be relative to the config file and must not contain
`..`. That rule applies to committed config and machine-local overrides. Point
Traefik or another consumer at the generated project-relative directory instead
of making BindPort render to an arbitrary absolute path. Output targets are
relative text file paths under the output root.

Use built-in outputs for the common Traefik, Caddy, nginx, HAProxy, JSON
snapshot, and env-file cases. When a project needs to feed another local tool,
keep the same owned-file contract and use a custom text template; see
[Optional Output Patterns](../integrations/optional-output-patterns.md) for TCP
alias, Kubernetes manifest, Docker/container, and JSON bridge patterns.

Hook settings:

```toml
[hooks]
timeout_ms = 5000

[[hooks.commands]]
enabled = true
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
Relative hook command paths are resolved from the directory that contains the
discovered BindPort config, and hook processes run with that directory as their
working directory. This keeps root-level monorepo hooks stable when
`bindport run` is launched from a package directory.
Supported events are `route_started`, `route_finished`, `routes_removed`,
`routes_marked_stale`, `render_requested`, and `output_rendered`. Hook
processes receive a minimal environment: `PATH` from the parent process plus
BindPort lifecycle metadata through `BINDPORT_HOOK_EVENTS`,
`BINDPORT_HOOK_SOURCES`, and `BINDPORT_HOOK_CONTEXT`. Other parent environment
values are not inherited, and secret values are not copied into hook metadata or
the registry.

For output integrations, prefer tools that watch generated files. Add hooks only
when the external tool needs an explicit reload or apply step, and keep that
command behind local trust.

Unknown top-level keys are reported by `bindport config validate`, `bindport
config explain`, and `bindport doctor`; unknown nested keys are currently
ignored. `default_range` is the only supported port-range key. No config keys
are currently deprecated, so `config validate` has no deprecation warnings to
emit. Service-level `health_url` is stored with each run. Active loopback `http://` health URLs are
probed by `status`; non-loopback and unsupported destinations stay `unknown`.
Literal loopback IPs, `localhost`, and `*.localhost` names are treated as local
targets without DNS resolution. `hostname`, `route_url`, and `health_url`
config values must not contain control characters.

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

Configured service `env`, `command`, and `args` values can additionally use the
narrow sibling syntax:

```text
{services.<name>.port}
{services.<name>.host}
{services.<name>.url}
{services.<name>.hostname}
{services.<name>.route_url}
{services.<name>.health_url}
```

Sibling lookup accepts exactly one active or reserved service with that name in
the current project and exact current worktree. Missing, stopped, stale, and
ambiguous matches fail before output preflight, hooks, or child spawn. BindPort
captures the referenced values once during startup; a running child's argv and
environment do not change when registry state changes.

`port` is decimal, `host` is the registered direct host, and `url` is the direct
`http://<host>:<port>` URL. `hostname` and `health_url` require configured
metadata. `route_url` uses the registered configured or hostname-derived route,
then the existing direct URL fallback. An assigned active or reserved address
does not mean the service is ready. BindPort does not start, order, supervise,
wait for, or health-check a dependency graph.

Sibling references do not apply to hostname, route URL, health URL, output,
route metadata, hook, or CLI `--env` templates. This is a placeholder extension,
not an expression language.

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
paths, restricted service env names, control characters in route metadata, YAML
anchors or aliases, output configuration errors, and incomplete hook
configuration. `doctor outputs` validates target hosts, resolved output roots,
template lookup, render planning, safe output paths, output ownership checks,
and hook trust status without writing files.

## Examples

- [Starter TOML/JSON/YAML examples](https://github.com/bindport/bindport/tree/main/examples/config)
- [Monorepo config guide](monorepos.md)
- [Two-service monorepo fixture](https://github.com/bindport/bindport/tree/main/examples/monorepo)
