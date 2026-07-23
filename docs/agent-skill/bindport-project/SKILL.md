---
name: bindport-project
description: Work with a repository that uses BindPort for local development ports, service discovery, route metadata, output templates, dashboard status, and hook trust. Use when Codex needs to configure BindPort, update project scripts, run local services through BindPort, inspect active URLs, or help agents adopt BindPort safely.
---

# BindPort Project

Use existing project scripts when they call BindPort. Do not hardcode local
development ports when BindPort config or scripts already describe the service.

## Workflow

1. Inspect `.bindport.toml`, `.bindport.json`, or `.bindport.yaml` from the
   current directory upward.
2. Run `bindport config explain` to confirm which config and service match the
   current directory.
3. Run `bindport config validate` after editing BindPort config.
4. Use `bindport reserve --all` when every named configured service needs a
   stable port before any child starts.
5. Use `bindport run <service>` to run configured services.
6. Use `bindport port <service>` for an exact current-worktree active or
   reserved port. Use `bindport status --json` and match identity/worktree
   fields for an exact URL; use `bindport list --json` for inventory and
   `bindport open <service> --print` only when registry-wide selection is
   unambiguous.
7. Use `bindport registry export` only for debug/backup snapshots or output
   ownership investigations; prefer `status --json` for normal automation.

`status --json` uses schema `1.0`. Ignore unfamiliar additive object fields,
handle its documented enum values explicitly, and do not depend on JSON member
or array ordering. Status and list are registry-wide: select by `identity_key`
or exact project/service/worktree fields, never array position. `open
--project` does not select a worktree. Do not edit the SQLite registry directly
to resolve a migration error; preserve it and follow
`docs/reference/registry-migrations.md`.

`reserve --all` is idempotent and scoped to the discovered project and current
worktree; new reservations commit all-or-nothing, and it neither starts
children nor owns sockets. `port` prints only the decimal port and newline, and
fails for missing, stopped, stale, or ambiguous matches. Use `open --print`,
never `--browser`, in non-interactive automation. Preview cleanup with
`bindport clean --dry-run --json`; do not add `--yes` without explicit approval.

## Config Rules

- Commit shared project config only when it avoids absolute paths, secrets, and
  machine-local values.
- Keep `.bindport.local.*`, `bindport.local.*`, generated output files, and
  `.env.local` private unless the user explicitly asks otherwise.
- Prefer service `command` plus `args` templates for tools that require a port
  CLI flag, for example `args = ["--port", "{port}"]`.
- Use service `env` templates for app-level values such as `PORT`,
  `HOSTNAME`, public route URLs, or framework-specific URL variables.
- Configured `env`, `command`, and `args` can reference an active or reserved
  sibling in the same project and exact worktree as
  `{services.<name>.<field>}`, where `field` is `port`, `host`, `url`,
  `hostname`, `route_url`, or `health_url`. Run `reserve --all` first when the
  sibling may not be active. Assignment does not imply readiness, and BindPort
  does not order or wait for services.
- Do not put execution-sensitive env names such as `PATH`, `LD_PRELOAD`,
  `DYLD_*`, `NODE_OPTIONS`, or `GIT_CONFIG_*` into config.

## Outputs And Hooks

- Run `bindport doctor outputs` before changing output templates, target hosts,
  render targets, or output roots.
- Run `bindport render --dry-run` before writing output files.
- Run `bindport render --diff` before replacing DB-owned output files when the
  content change matters.
- Treat hooks as disabled until trusted by the user through the CLI.
- Do not run `bindport hooks trust`, `bindport hooks deny`, or hook commands
  without explicit user approval.

## Useful Commands

```sh
bindport --help
bindport config explain
bindport config validate
bindport doctor
bindport doctor outputs
bindport status --json
bindport list --json
bindport registry export
bindport clean --dry-run --json
bindport reserve --all
bindport port <service>
bindport open <service> --print
bindport run <service>
bindport render --dry-run
BINDPORT_LOG=debug bindport run <service>
bindport hooks status
```
