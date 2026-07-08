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
4. Use `bindport run <service>` to run configured services.
5. Use `bindport status --json` or `bindport open <service> --print` to find
   active service URLs.

## Config Rules

- Commit shared project config only when it avoids absolute paths, secrets, and
  machine-local values.
- Keep `.bindport.local.*`, `bindport.local.*`, generated output files, and
  `.env.local` private unless the user explicitly asks otherwise.
- Prefer service `command` plus `args` templates for tools that require a port
  CLI flag, for example `args = ["--port", "{port}"]`.
- Use service `env` templates for app-level values such as `PORT`,
  `HOSTNAME`, public route URLs, or framework-specific URL variables.
- Do not put execution-sensitive env names such as `PATH`, `LD_PRELOAD`,
  `DYLD_*`, `NODE_OPTIONS`, or `GIT_CONFIG_*` into config.

## Outputs And Hooks

- Run `bindport doctor outputs` before changing output templates or render
  targets.
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
bindport open <service> --print
bindport run <service>
bindport render --dry-run
bindport hooks status
```
