# Adoption Setup

Use this guide when adding BindPort to an existing project. It focuses on the
files a team should commit, the local files each developer should keep private,
and the checks to run before relying on the config.

## Install

For Rust-first projects or local global use:

```sh
cargo install bindport
```

For JavaScript and monorepo projects, prefer a development dependency so project
scripts use the same tool version for everyone:

```sh
npm install --save-dev bindport
```

Project scripts can then call the local executable:

```json
{
  "scripts": {
    "dev:web": "bindport run web",
    "bindport:status": "bindport status --json",
    "bindport:doctor": "bindport doctor"
  }
}
```

## Initialize

Create the shared project config from the repository root:

```sh
bindport init
bindport config validate
bindport config explain
```

Commit `.bindport.toml` when it describes shared project behavior. The generated
config avoids absolute paths and machine-local values by default.

Use `bindport init --user` only for the optional user fallback config. That file
lives in the user config directory and is not a project adoption step.

## Commit Or Ignore

Commit:

- `.bindport.toml`, `.bindport.json`, or `.bindport.yaml`
- project-owned templates under `.bindport/templates/`
- package scripts and docs that call `bindport`
- reviewed hook declarations, without assuming they are trusted

Ignore:

```gitignore
.bindport.local.*
bindport.local.*
.bindport/generated/
.env.local
```

Machine-local overrides belong in `.bindport.local.toml` or another local
override file beside the shared config. Keep dashboard bind addresses, local
proxy host allowlists, private output destinations, and machine-specific values
there.

BindPort state lives outside the repository by default:

- config fallback: `$XDG_CONFIG_HOME/bindport/config.toml`, or
  `~/.config/bindport/config.toml`
- registry state: `$XDG_STATE_HOME/bindport/registry.sqlite`, or
  `~/.local/state/bindport/registry.sqlite`

## No-Proxy Mode

BindPort does not require a local proxy. For no-proxy adoption, omit `[[outputs]]`
and hooks, then use the assigned port directly:

```toml
project = "example"

[[services]]
name = "web"
path = "."
command = ["vite"]
args = ["--host", "0.0.0.0", "--port", "{port}", "--strictPort"]
env.PORT = "{port}"
```

Run:

```sh
bindport run web
bindport status
bindport open web --print
```

Add outputs later only when an external tool, such as Traefik, needs generated
route files.

For services started by another tool, such as Docker Compose, reserve the port
first and pass the assigned value to that tool's own config or command:

```sh
bindport reserve web
bindport status --json
bindport release web
```

## Framework Examples

Next.js:

```toml
[[services]]
name = "next"
path = "apps/web"
command = ["next", "dev"]
args = ["--hostname", "0.0.0.0", "--port", "{port}"]
hostname = "{branch}.example-web.localhost"
route_url = "http://{hostname}"
env.PORT = "{port}"
env.HOSTNAME = "0.0.0.0"
env.NEXT_PUBLIC_BINDPORT_URL = "{route_url}"
```

Vite:

```toml
[[services]]
name = "vite"
path = "apps/web"
command = ["vite"]
args = ["--host", "0.0.0.0", "--port", "{port}", "--strictPort"]
hostname = "{branch}.example-web.localhost"
route_url = "http://{hostname}"
env.VITE_BINDPORT_URL = "{route_url}"
```

FastAPI with Uvicorn:

```toml
[[services]]
name = "api"
path = "services/api"
command = ["uvicorn", "example.main:app"]
args = ["--host", "0.0.0.0", "--port", "{port}"]
hostname = "{branch}.example-api.localhost"
route_url = "http://{hostname}"
health_url = "{route_url}/health"
env.BINDPORT_ROUTE_URL = "{route_url}"
```

Tools that need a port flag should use `command` plus `args` templates instead
of relying on environment variables. One-off commands can still use a shell
wrapper:

```sh
bindport run storybook -- sh -c 'storybook dev --port "$PORT" --host 0.0.0.0'
```

## Agent Setup

Add a short BindPort section to the project agent instructions so AI coding
agents do not hardcode ports or edit local-only files:

```markdown
## BindPort

- Use `bindport run <service>` or existing project scripts instead of hardcoding
  development ports.
- Run `bindport config validate` after changing `.bindport.*` config.
- Use `bindport status --json` or `bindport open <service> --print` to discover
  active service URLs.
- Do not edit `.bindport.local.*`, `bindport.local.*`, generated output files,
  or `.env.local` unless explicitly asked.
- Do not run `bindport hooks trust`, `bindport hooks deny`, or hook commands
  without explicit user approval.
```

For `CLAUDE.md`, keep the file as a pointer when the project already has
`AGENTS.md`:

```markdown
# CLAUDE.md

See [AGENTS.md](./AGENTS.md) for project instructions.
```

A copyable Codex skill is available at
[docs/agent-skill/bindport-project](agent-skill/bindport-project/SKILL.md).
Install it by copying the `bindport-project` folder into
`$CODEX_HOME/skills/`, or the equivalent skill directory for the agent runtime.
Install it only in projects where agents routinely configure or operate
BindPort.

Point agents at the detailed docs when they need more than the short project
rules:

- [Config](config.md): config discovery, supported fields, validation, hooks,
  and placeholders.
- [Status](status.md): `status --json` schema and service URL lookup.
- [Templates](templates.md): output templates, render lifecycle, ownership, and
  Traefik file-provider setup.
- [Dashboard](dashboard.md): local dashboard service controls and API behavior.
- [Monorepos](monorepos.md): root config, service paths, local overrides, and
  workspace inference.

## Adoption Checks

Run these before opening the adoption PR:

```sh
bindport config validate
bindport config explain
bindport doctor
bindport doctor outputs
```

If outputs are configured, dry-run rendering before writing files:

```sh
bindport render --dry-run
```

If hooks are configured, inspect their trust state:

```sh
bindport hooks status
```
