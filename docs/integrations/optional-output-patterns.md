# Optional Output Patterns

BindPort's stable integration surface is text output plus locally trusted hooks.
The built-in templates cover common route files, but the same output contract can
feed other local tools when a project needs it:

```text
registry snapshot -> template render -> owned text files -> external tool
```

This page documents patterns that are intentionally not core BindPort behavior.
They are useful when a project already has the surrounding tool, file watcher,
or local cluster. They are not promises that BindPort will open sockets, apply
Kubernetes resources, run proxy admin API clients, or mutate Docker state.

## What Exists Today

Use these capabilities as the boundary for optional integrations:

| Capability | Status | Notes |
| --- | --- | --- |
| Per-route text files | Available | Most output templates render one file per route. |
| Aggregate JSON route snapshot | Available | `bindport-json-snapshot` renders one file for all routes. |
| Project/global/custom templates | Available | Templates live under `.bindport/templates` or the user config template directory. |
| Built-in Traefik and Caddy file snippets | Available | These are file-provider snippets, not proxy management. |
| Output ownership and safe cleanup | Available | BindPort tracks generated files in SQLite and refuses unsafe overwrites. |
| `render --dry-run`, `--diff`, and `--repair` | Available | Use these before connecting generated files to another tool. |
| Trusted lifecycle hooks | Available | Hooks are disabled until locally trusted with the CLI. |
| TCP forwarding daemon | Not built in | BindPort can render config for another forwarding tool. |
| Kubernetes apply/reconcile | Not built in | BindPort can render manifests for another tool to apply. |
| Docker labels or container mutation | Not built in | Generate files or JSON for a local tool instead. |
| Proxy admin API clients | Not built in | Use file-provider reloads or an explicitly trusted hook. |
| Secrets capture or forwarding | Not built in | Templates and hooks should not rely on BindPort secrets. |

This keeps BindPort useful as a local source of route truth without making every
project inherit one proxy, one cluster runtime, or one deployment workflow.

## When To Use A Custom Output

Use a custom output when all of these are true:

- The target tool can read a file or directory.
- The generated file can be safely owned by BindPort.
- The reload/apply step is either automatic in the target tool or is handled by
  a reviewed local hook.
- The generated content does not need secrets from BindPort.

Do not use an output when the integration needs long-running network forwarding,
privileged ports, opaque side effects, or direct API mutation. Those belong in a
separate tool or a future explicit adapter.

## Choosing A Pattern

Start with the smallest surface that solves the integration:

| Need | Prefer |
| --- | --- |
| Another script needs the route list | `bindport-json-snapshot` |
| A proxy can watch a directory | Built-in or custom file-provider template |
| A framework needs env files after a route starts | `bindport-env-local` or a custom text template |
| A tool needs a reload after generated files change | `output_rendered` hook after local trust |
| A local cluster needs manifests | Custom Kubernetes template plus a separate apply tool |
| A TCP service needs a stable alias | Custom template for an external TCP forwarder |
| The integration must own sockets, apply resources, or poll health | A separate tool or future adapter |

Prefer generated files over hook side effects when the consumer already has a
watch mode. A watched directory is easier to inspect, diff, repair, and disable
than a hook that mutates external state.

## Template Authoring Workflow

Use this workflow before committing an optional output:

1. Create a project template under `.bindport/templates/`.
2. Add one `[[outputs]]` entry with a project-relative `root` and a target path
   under that root.
3. Put machine-specific values in `.bindport.local.toml`, not committed config.
4. Validate the config and output plan.
5. Dry-run and diff the rendered files.
6. Point the external tool at the generated directory.
7. Add a hook only if the external tool has no reliable file watcher.

Example template names:

```text
.bindport/templates/local-tcp-forwarder.conf.j2
.bindport/templates/local-k8s-ingress.yaml.j2
.bindport/templates/local-ingressroute.yaml.j2
```

Example output config:

```toml
[output_defaults]
root = ".bindport/generated"
delete_on = ["removed"]

[[outputs]]
name = "tcp-forwarder"
template = "local-tcp-forwarder"
target = "tcp/{{ route.slug }}.conf"

[outputs.vars]
bind_host = "127.0.0.1"
```

Validate and preview:

```sh
bindport config validate
bindport doctor outputs
bindport render --dry-run
bindport render --diff
```

Only run a normal render after the plan points at the intended files:

```sh
bindport render
```

## Template Data To Rely On

Custom output templates should stay close to the documented route model:

- `route.project`
- `route.service`
- `route.state`
- `route.port`
- `route.hostname`
- `route.route_url`
- `route.target_url`
- `route.branch_label`
- `route.slug`
- `route.unique_slug`
- `output.name`
- `output.template`
- `output.root`
- `output.target`
- `output.delete_on`
- `vars.*`

Use `vars.*` for integration-specific values such as a gateway address,
namespace, class name, reload mode, bind host, or include directory. These
values are scoped to the current output entry. If a config has multiple
`[[outputs]]` entries, put `[outputs.vars]` directly after the output it
belongs to.

Avoid using `snapshot.generated_at` in integration files unless a changed file
on every render is desirable. Changing content can trigger proxy reloads or
external watchers even when the route data did not change.

## Lifecycle And Cleanup

Optional outputs should be designed around route state:

- `active`: the wrapped service is running and has a usable route.
- `reserved`: a port is intentionally held for an external process.
- `stopped`: BindPort saw the wrapped process exit.
- `stale`: BindPort reconciled a registry entry whose process is gone.
- `removed`: cleanup deleted the route entry from active registry status.

The default cleanup is conservative:

```toml
delete_on = ["removed"]
```

That means stopped and stale routes can still render state for dashboards,
debugging, and external tools. The file is deleted only after cleanup removes
the route from the registry view.

Use earlier deletion only for generated files that should disappear as soon as
the process stops:

```toml
delete_on = ["stopped", "stale", "removed"]
```

Deletion is still ownership-checked. BindPort removes only files it previously
rendered and only when the current file hash matches the registry record.
Externally modified files are preserved and reported as output errors.

## Hooks With Optional Outputs

Hooks are useful when a generated file is not enough:

- reload an existing proxy after `output_rendered`;
- ask a local supervisor to reread a generated TCP config;
- notify a dev-only apply script after manifests changed;
- run a local validation command after `render_requested`.

Keep hooks small and reviewed:

```toml
[hooks]
timeout_ms = 5000

[[hooks.commands]]
name = "reload-forwarder"
events = ["output_rendered"]
command = ["./ops/localhost/reload-forwarder.sh"]
timeout_ms = 2000
```

Hooks do not run until trusted:

```sh
bindport hooks status
bindport hooks trust reload-forwarder
```

Trust is local state outside the repository. If the hook definition changes, or
if a local script target changes, BindPort marks it as changed and blocks it
until the user reviews it again.

Prefer wrapper scripts over long shell commands in config. Wrapper scripts make
the trust target visible, keep quoting out of TOML, and can set any required
tool environment such as `HOME`, `KUBECONFIG`, or a container socket path.

## TCP Alias Candidates

TCP aliasing means giving a non-HTTP service, such as a database, cache, or
message broker, a stable local connection target while the real process still
uses an allocated port. BindPort does not provide a TCP forwarding daemon today.

What BindPort can do now is render configuration for a tool that already knows
how to forward TCP:

```toml
[[outputs]]
name = "tcp-forwarder"
template = "local-tcp-forwarder"
target = "tcp/{{ route.slug }}.conf"
delete_on = ["removed"]
```

The external forwarder decides whether that becomes an HAProxy backend, an
nginx `stream` block, a `socat` supervision file, or another local config
format. BindPort's job is only to keep the generated file aligned with the
current route state.

An illustrative template shape:

```text
# generated by BindPort
name={{ route.unique_slug }}
state={{ route.state }}
target_host={{ vars.target_host }}
target_port={{ route.port }}
```

The forwarding tool owns the real syntax. Keep the generated file easy to
inspect, and keep any alias bind address or forwarded hostname in `vars.*` so a
developer can override it locally.

Keep these limits clear:

- BindPort does not bind the alias port.
- BindPort does not proxy TCP traffic.
- BindPort does not reserve privileged ports for aliases.
- BindPort does not know whether the target protocol is Postgres, Redis, MySQL,
  or anything else.
- Health checks remain the current route metadata and HTTP-oriented status
  surface unless a future release adds TCP health behavior.

If the forwarding tool needs a reload command, use a hook only after reviewing
and trusting it locally:

```toml
[hooks]

[[hooks.commands]]
name = "reload-forwarder"
events = ["output_rendered"]
command = ["./scripts/reload-forwarder"]
```

The hook will not run on another machine until that user approves it with
`bindport hooks trust`.

## TCP Alias Checklist

Before adopting a TCP alias pattern, confirm:

- The external forwarder can watch the generated directory or be reloaded by a
  trusted local hook.
- The alias bind address is not privileged unless the external tool handles
  privilege safely.
- The generated file includes route state so stopped/stale entries can be
  handled intentionally.
- The template does not assume HTTP-only fields such as `route.route_url` when
  the service is not HTTP.
- The run command still receives the assigned port through `PORT`, service
  `env`, or service `args`.

## Kubernetes Ingress Candidates

Kubernetes output is useful when a local cluster already exists and another
tool can apply generated manifests. Examples include Rancher Desktop/k3s,
Tilt, Skaffold, kustomize overlays, or a local script watched by the user.

BindPort can render manifest files, but it does not apply them:

```toml
[[outputs]]
name = "k8s-ingress"
template = "local-k8s-ingress"
target = "k8s/ingress/{{ route.slug }}.yaml"
delete_on = ["removed"]
```

Keep cluster choices in output vars:

```toml
[[outputs]]
name = "k8s-ingress"
template = "local-k8s-ingress"
target = "k8s/ingress/{{ route.slug }}.yaml"

[outputs.vars]
namespace = "default"
ingress_class = "traefik"
gateway_host = "host.docker.internal"
```

Use this pattern only when the rest of the Kubernetes wiring is already
understood. A generic Ingress usually points at a Kubernetes Service, while a
host dev server is a process listening on the host. Your local cluster may need
an `ExternalName` Service, gateway address, Traefik-specific resource, or a
separate bridge before an Ingress can reach the host process.

Recommended workflow:

1. Render files with `bindport render --dry-run`.
2. Inspect changed manifests with `bindport render --diff`.
3. Render to a project-local generated directory.
4. Let a separate, explicit tool apply those manifests.
5. Keep apply/reload hooks local and trusted.

Avoid a template that embeds cluster-specific names, namespaces, or gateway
addresses in committed config. Put those values in `.bindport.local.toml` or in
the external tool's local configuration.

## Kubernetes Checklist

Before committing Kubernetes output config, confirm:

- The generated directory is separate from hand-edited manifests.
- The namespace, ingress class, gateway address, and TLS assumptions are local
  vars or documented project conventions.
- A developer can run `bindport render --diff` before applying anything.
- The apply step is explicit, watched by another tool, or hidden behind a
  locally trusted hook.
- The template documents whether it expects a Service, ExternalName Service,
  local cluster gateway, or proxy-specific CRD.
- Generated manifests do not include secrets.

## Traefik IngressRoute Candidates

Traefik IngressRoute manifests are also template candidates, but they should
stay separate from the built-in `bindport-traefik` file-provider template. The
file-provider template writes Traefik dynamic config for an existing proxy.
IngressRoute output would write Kubernetes custom resources for a cluster
controller to reconcile.

That difference matters:

- File-provider output is read directly by Traefik.
- IngressRoute output is read by Kubernetes and then reconciled by Traefik.
- BindPort should not assume the Traefik CRDs are installed.
- BindPort should not run `kubectl apply` by default.
- BindPort should not create namespaces, Services, Secrets, or TLS resources.

If a project uses this pattern, document the local cluster assumptions beside
the template and keep the generated directory out of hand-edited manifests.

## Docker And Container Candidates

BindPort should not mutate Docker containers or labels. For container-heavy
local setups, use one of these safer patterns:

- Generate Traefik or Caddy file-provider config and mount the generated
  directory into the proxy container.
- Generate a JSON snapshot for a local helper that already owns Docker access.
- Use `.bindport.local.toml` for container-specific target hosts such as a
  gateway IP.
- Use a trusted hook only when a reviewed local wrapper script needs to reload
  a containerized tool.

Do not put a Docker socket path, container name, or label mutation command in
shared config unless the project has explicitly reviewed that local trust model.

## JSON Snapshot As A Safer Bridge

When a tool does not need a native config format, prefer the built-in JSON
snapshot output:

```toml
[[outputs]]
name = "routes-json"
template = "bindport-json-snapshot"
target = "routes.json"
```

The JSON snapshot gives external scripts a stable route model without requiring
the script to parse human CLI output. It is often the better first step before
creating a more opinionated TCP, Kubernetes, nginx, HAProxy, or custom proxy
template.

Use JSON when:

- the consumer is another script or agent;
- the target tool has its own renderer;
- several route states need to be considered together;
- the integration is still experimental;
- the project wants a stable contract before committing to a custom template
  syntax.

## Security Checklist

Optional outputs cross a boundary from BindPort's registry into another tool.
Review that boundary before trusting the integration:

- Generated files should contain route metadata, not secrets.
- Output roots must stay project-relative and must not use `..`.
- Output targets should be deterministic and under the output root.
- Machine-specific hosts, namespaces, socket paths, and reload commands belong
  in local overrides or local scripts.
- Hooks are disabled by default and must be trusted by each user.
- Hook processes receive a minimal environment; wrapper scripts must opt into
  required environment values.
- Prefer file watchers and `render --diff` over hooks that immediately mutate
  external state.
- Do not use BindPort templates to write directly into system directories,
  cluster-managed directories, or hand-edited config trees.

## When A Future Adapter Is Still Needed

A template is not the right tool for every integration. A future adapter or
separate companion tool would be a better fit for:

- a TCP proxy daemon managed by BindPort;
- reserved alias ports that need collision checks separate from service ports;
- direct Kubernetes API apply/reconcile behavior;
- Docker label discovery, import, or mutation;
- TCP health checks or protocol-aware status;
- certificate, DNS, or `/etc/hosts` management;
- long-running background processes beyond the dashboard service.

Keeping those behaviors out of generic templates avoids surprising side effects
and lets each future integration define explicit safety and test boundaries.

## Acceptance Checklist

Before committing an optional output template:

- Run `bindport doctor outputs`.
- Run `bindport render --dry-run`.
- Run `bindport render --diff` after the first write.
- Confirm generated files land under a project-relative output root.
- Confirm cleanup behavior with the intended `delete_on` states.
- Keep machine-specific hostnames, gateway IPs, namespaces, and reload commands
  out of committed config.
- Document which external tool watches or applies the generated files.
- If a hook is needed, inspect it with `bindport hooks status` and trust it only
  after local review.
