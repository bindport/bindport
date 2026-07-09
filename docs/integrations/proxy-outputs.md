# Proxy Output Guides

BindPort integrates with existing local proxies by writing files those tools can
read. It does not start Traefik, Caddy, nginx, Kubernetes, Docker, Rancher
Desktop, or any proxy admin API client. The boundary is:

```text
BindPort registry snapshot -> generated text files -> your existing tool
```

That boundary keeps generated routing state inspectable, reviewable, and easy
to delete. The proxy still owns entrypoints, TLS, middleware, DNS, certificates,
and reload behavior.

For TCP alias candidates, Kubernetes Ingress, Traefik IngressRoute, container
helper files, and similar custom file-based integrations, see
[Optional Output Patterns](optional-output-patterns.md).

## Shared BindPort Config

Most proxy outputs start with the same BindPort shape:

```toml
project = "example-web"

[[services]]
name = "web"
hostname = "{branch}.example-web.localhost"
health_url = "{route_url}/health"
env.PORT = "{port}"
env.NEXT_PUBLIC_BINDPORT_URL = "{route_url}"

[output_defaults]
root = ".bindport/generated"
target_host = "127.0.0.1"
target_scheme = "http"
delete_on = ["removed"]

[[outputs]]
name = "traefik"
template = "bindport-traefik"
target = "traefik/{{ route.slug }}.yml"
```

The `hostname` is the browser-facing route. The generated proxy target is built
from `target_scheme`, `target_host`, and the assigned route port. With the
defaults above, an active route points the proxy at
`http://127.0.0.1:<assigned-port>`.

Keep committed config portable. Machine-specific target hosts, proxy mount
paths, and reload commands belong in `.bindport.local.toml` or in the proxy's
own local config, not in the shared project config.

## Target Host Selection

`target_host` is the address the proxy uses to reach the dev server. It is not
the browser hostname.

| Proxy location | Suggested `target_host` |
|---|---|
| Proxy runs directly on the host | `127.0.0.1` |
| Docker Desktop container reaches a host process | `host.docker.internal` |
| Rancher Desktop, k3s, or another VM/container runtime | runtime host alias or gateway IP |
| No proxy | omit proxy outputs |

Use `127.0.0.1` when the proxy and wrapped process share the host network
namespace. Use `host.docker.internal`, a runtime-provided host alias, or a
gateway IP when the proxy runs in a container or VM and cannot reach host
loopback. Rancher Desktop and k3s setups vary by backend and networking mode, so
store the verified value locally:

```toml
# .bindport.local.toml
[output_defaults]
target_host = "host.docker.internal"
```

Do not set `target_host = "0.0.0.0"`. That is a bind address, not a
connectable target. `bindport doctor outputs` rejects target hosts that include
URLs, paths, ports, whitespace, or unspecified addresses.

Check the result before starting the proxy:

```sh
bindport doctor outputs
bindport render --dry-run
bindport render --verbose --dry-run
```

## Traefik File Provider

Traefik is the most direct fit because its file provider can watch a directory
of dynamic config files.

BindPort output:

```toml
[[outputs]]
name = "traefik"
template = "bindport-traefik"
target = "traefik/{{ route.slug }}.yml"

[outputs.vars]
entrypoints = ["web"]
tls = false
middlewares = []
```

Traefik dynamic provider example:

```yaml
providers:
  file:
    directory: /absolute/path/to/project/.bindport/generated/traefik
    watch: true
```

If Traefik runs in a container, mount the generated directory into the container
and make the file provider watch the mounted path. The rendered service target
still needs to be reachable from inside that container. If routes render but
Traefik returns 502, the first thing to check is whether `target_host` is still
`127.0.0.1` while Traefik is running in a different network namespace.

Useful checks:

```sh
bindport render --diff traefik
bindport doctor outputs
BINDPORT_LOG=debug bindport run web
```

## Caddyfile Snippets

The built-in `bindport-caddy` template renders Caddyfile site blocks. Caddy does
not behave like Traefik's file provider by default, so treat BindPort as the
snippet generator and keep reload behavior explicit in your Caddy setup.

BindPort output:

```toml
[[outputs]]
name = "caddy"
template = "bindport-caddy"
target = "caddy/{{ route.slug }}.caddy"

[outputs.vars]
site_scheme = "http"
```

Caddyfile import example:

```caddyfile
{
  auto_https off
}

import /absolute/path/to/project/.bindport/generated/caddy/*.caddy
```

Reload Caddy the way your local setup expects: manually after
`bindport render`, through your process runner, through a file watcher, or
through a locally trusted BindPort hook. Do not commit a hook that reloads Caddy
and expect it to run automatically on other machines; hook execution remains a
local trust decision.

## No Proxy

BindPort is still useful without a proxy. Omit `[[outputs]]` entirely and use
the registry as the source of truth:

```sh
bindport run web
bindport status --json
bindport list --json
bindport open web --print
```

In no-proxy mode, route metadata is still available for apps and agents:

```toml
[[services]]
name = "web"
command = ["npm", "run", "dev"]
args = ["--", "--port", "{port}"]
hostname = "{branch}.example-web.localhost"
route_url = "http://127.0.0.1:{port}"
env.NEXT_PUBLIC_BINDPORT_URL = "{route_url}"
```

The `hostname` field is only metadata unless some other tool routes that name.
Use `url` or `route_url` from `status --json` when no proxy is listening on that
hostname.

## Troubleshooting

- Run `bindport doctor outputs` after changing output config or target hosts.
- Run `bindport render --dry-run` to confirm planned files before writing.
- Run `bindport render --diff` before reloading a proxy when you want to see the
  exact config change.
- Use `BINDPORT_LOG=debug bindport run <service>` when an automatic render does
  not behave as expected.
- If generated files are not removed, check `delete_on`, cleanup state, and
  whether the files are still DB-owned and unmodified.
- If the proxy returns 404, check the generated hostname and route state.
- If the proxy returns 502, check `target_host` from the proxy's network
  namespace.
