# Concepts

BindPort is local development coordination glue. It gives each project service
a stable port, records who owns that port, and exposes the resulting route
metadata to humans, scripts, dashboards, templates, hooks, and AI agents.

It is intentionally not the local edge proxy. It does not bind `80` or `443`,
install certificates, mutate DNS, edit `/etc/hosts`, install a root daemon, or
kill active processes by default.

## Mental Model

BindPort has six core pieces:

| Piece | What it does |
|---|---|
| Allocator | Finds an available port from the configured range. |
| Runner | Starts a child command with `PORT=<assigned>` and optional templated args/env. |
| Registry | Stores active, reserved, stopped, and stale service state in SQLite. |
| Dashboard | Shows registry state and exposes limited local cleanup actions. |
| Outputs | Render text files from registry routes for tools such as Traefik. |
| Hooks | Run trusted local commands when route/output lifecycle events happen. |

The registry is the center. Commands write to it, status reads from it, output
rendering uses it as the route source, and the dashboard presents it.

## Source Of Truth

The registry is what makes BindPort more than a port picker. A port picker only
answers "what port can I use right now?" BindPort also records "who owns this
port, what route represents it, what branch/worktree produced it, what outputs
were rendered for it, and what cleanup is safe later?"

That source-of-truth role lets separate tools cooperate without sharing an
implementation:

- package scripts run services;
- dashboard views show service state;
- output templates create proxy/env files;
- hooks react to lifecycle events;
- agents discover URLs without guessing.

## What BindPort Solves

Local development often has three recurring problems:

- Multiple apps want fixed ports and collide.
- Branches or worktrees need unique, discoverable local URLs.
- External tools need generated config whenever local routes change.

BindPort handles those by allocating ports per resolved project/service identity
and preserving stable assignments when possible.

The practical outcome is that the development environment can be dynamic while
the workflow stays predictable. Developers do not need to remember which branch
got which port, and tools do not need to hardcode a global port map.

## What BindPort Does Not Solve

BindPort does not replace:

- Traefik, Caddy, nginx, mkcert, or another proxy/certificate layer.
- Docker Compose, systemd, process managers, or framework dev servers.
- Secret managers or environment-file policy.
- Network access control for non-loopback services.

Use BindPort to allocate, record, and render local route state. Let the tool
that already owns networking or process supervision keep owning that job.

## Identities

A BindPort identity is the stable key used to decide whether a service should
reuse a previous port. Identity can come from:

1. `bindport run <service>` or command options.
2. `BINDPORT_PROJECT` / `BINDPORT_SERVICE`.
3. project config and local overrides.
4. package metadata, workspace roots, git worktree, and command inference.

Use explicit config for teams. Inference is useful for one-off experiments, but
it should not be the primary contract for a shared project.

## Routes

A route is the user-facing service metadata BindPort can expose after a port is
chosen:

- direct URL, such as `http://127.0.0.1:29123`
- hostname, such as `feature-tree.example-web.localhost`
- route URL, such as `http://feature-tree.example-web.localhost`
- health URL, such as `http://feature-tree.example-web.localhost/health`

The direct URL always points at the assigned port. Hostname and route URL are
configured metadata for tools that route traffic through a proxy.

## Lifecycle

A normal wrapped service lifecycle looks like this:

1. Resolve config, identity, port range, and route metadata.
2. Prefer the previous free port for the same identity, otherwise scan from a
   stable identity-based offset.
3. Record the active run in the registry.
4. Start the child command with `PORT` and any configured templated args/env.
5. Render enabled outputs and run trusted hooks for lifecycle events.
6. Mark the run stopped when the child exits.

If the assigned port is claimed between the probe and child startup, BindPort
retries once with another port when the child fails immediately and the assigned
port is then occupied.
