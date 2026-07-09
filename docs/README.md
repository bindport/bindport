# BindPort Documentation

These Markdown files are the canonical BindPort docs. They are written to be
readable directly in GitHub and can also be built into a static site with
mdBook.

BindPort is a proxy-neutral local development port registry, allocator, runner,
dashboard, integration file renderer, and trusted hook runner. It wraps
development commands, assigns stable local ports, records project/service
identity, and can render files for tools such as Traefik.

BindPort is not a reverse proxy. It does not bind `80` or `443`, install
certificates, mutate DNS, edit `/etc/hosts`, install a root daemon, or kill
processes by default.

## What You Get

BindPort turns local dev ports into named, queryable, automatable project
state:

- Developers can run multiple services or worktrees without agreeing on a
  shared fixed port map.
- Frameworks that require `--port` flags can receive the assigned port through
  structured config instead of fragile shell snippets.
- Apps can receive route-aware environment values such as `PORT`,
  `NEXT_PUBLIC_*`, or a service URL derived from the current branch.
- Existing proxies can watch generated files instead of requiring BindPort to
  become the proxy.
- Dashboards, scripts, and AI agents can ask `bindport status --json`,
  `bindport list --json`, or `bindport open <service> --print` instead of
  guessing where a service is. Use `bindport registry export` only when a debug
  or backup workflow needs raw registry rows.
- Hook execution is explicit and locally trusted, so checked-in config cannot
  silently run commands on a new machine.

The value is strongest in monorepos, remote dev boxes, branch-heavy workflows,
and teams that already have a proxy or process runner but need a reliable source
of truth for local route state.

## Start Here

New users should read:

- [Why BindPort](getting-started/why-bindport.md) for the product value,
  use cases, and tradeoffs.
- [Concepts](getting-started/concepts.md) for the short mental model: allocator, registry,
  runner, dashboard, outputs, hooks, and what BindPort intentionally does not
  do.
- [Install BindPort](getting-started/install.md) to pick npm, Cargo, Homebrew, binstall, mise,
  or raw GitHub Release binaries.
- [Quick Start](getting-started/quick-start.md) to run a command, inspect the registry, open a
  URL, and clean old state.
- [Adoption Setup](getting-started/adoption.md) to add BindPort to an existing project without
  committing machine-local values.
- [CLI Commands](daily-use/cli.md) to see the commands available from scripts, shells, and
  automation.

Project maintainers should read:

- [Configuration](daily-use/configuration.md) for config discovery, precedence, service entries,
  dashboard settings, outputs, hooks, and template placeholders.
- [Running Services](daily-use/running-services.md) for configured service commands,
  command-line port flags, route metadata, and environment bridging.
- [Monorepos and Worktrees](daily-use/monorepos.md) for multi-service repository layouts.
- [Templates](integrations/templates.md) for generated integration files such as Traefik
  file-provider snippets and `.env.local` files.
- [Proxy Outputs](integrations/proxy-outputs.md) for Traefik, Caddy, container
  target hosts, and no-proxy usage.
- [Optional Output Patterns](integrations/optional-output-patterns.md) for TCP
  alias candidates, Kubernetes/IngressRoute manifests, container workflows,
  JSON bridge files, and the boundary between generated files and external
  tools.
- [Hooks and Trust](integrations/hooks.md) for lifecycle events and local trust decisions.

Operators and agents should read:

- [Dashboard](integrations/dashboard.md) for local dashboard controls, API behavior, auth,
  and cleanup actions.
- [Status and Cleanup](operations/status.md) for registry states, registry
  export, URL lookup, reservations, and stale/stopped cleanup.
- [Health and Troubleshooting](operations/troubleshooting.md) for diagnostics and common
  failure modes.
- [Security Model](operations/security.md) for local-first defaults, config safety, output
  ownership, dashboard auth, and hook trust.
- [Platform Support](reference/platform-support.md) for supported operating systems,
  package targets, process behavior, and CI gates.
- [Agent and LLM Setup](reference/agents.md) for `AGENTS.md`, project scripts, and
  `llms.txt` discovery.

## Local Site Preview

Install mdBook:

```sh
cargo install mdbook --locked
```

From the repository root, serve the docs site on all interfaces for remote dev
boxes:

```sh
scripts/docs-serve.sh
```

Build the static site:

```sh
scripts/docs-build.sh
```

The generated output goes to `dist/docs`, which is ignored by git.
The wrapper scripts call mdBook and copy static discovery files into the
generated output.

The built site also publishes:

- `llms.txt`: curated agent/LLM entrypoint.
- `llms-full.txt`: expanded agent/LLM context.
- `robots.txt`: basic crawler discovery.
- `404.html`: mdBook-generated not-found page.

Pass `--base-url` for production builds when the public docs URL is known. The
build wrapper then writes `sitemap.xml` and adds a sitemap entry to
`robots.txt`:

```sh
scripts/docs-build.sh --base-url https://example.com/docs/
```

When running through mise:

```sh
mise exec -- scripts/docs-build.sh --base-url https://example.com/docs/
```

## Conventions

- Keep docs usable in GitHub without the generated site.
- Put site navigation in [SUMMARY.md](SUMMARY.md).
- Prefer TOML examples first; JSON and YAML are alternates.
- Do not document unshipped features as available.
- Avoid local machine paths, personal names, secrets, and scratch workspace
  references in committed docs.
