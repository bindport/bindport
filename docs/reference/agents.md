# Agent and LLM Setup

BindPort is meant to be easy for AI coding agents to operate safely. Agents
should know how to start services, discover URLs, validate config, and avoid
mutating local-only state.

## Project Agent Instructions

Add a short BindPort section to `AGENTS.md` or the project's equivalent agent
instruction file:

```markdown
## BindPort

- Use existing project scripts or `bindport run <service>` instead of hardcoding
  development ports.
- Run `bindport config validate` after changing `.bindport.*` config.
- Use `bindport status --json` or `bindport open <service> --print` to discover
  active service URLs.
- Do not edit `.bindport.local.*`, `bindport.local.*`, generated output files,
  or `.env.local` unless explicitly asked.
- Do not run `bindport hooks trust`, `bindport hooks deny`, or hook commands
  without explicit user approval.
```

For `CLAUDE.md`, prefer a pointer when `AGENTS.md` is already present:

```markdown
# CLAUDE.md

See AGENTS.md for project instructions.
```

## Codex Skill

A compact Codex skill lives in the repository at
[docs/agent-skill/bindport-project/SKILL.md](https://github.com/bindport/bindport/blob/main/docs/agent-skill/bindport-project/SKILL.md).
Install it only in projects where agents routinely configure or operate
BindPort.

The skill is not a replacement for project docs. It is an orientation layer
that tells an agent which commands to run and which docs to consult.

## LLM Discovery

The docs site publishes:

- `llms.txt`: a curated index of the most important pages.
- `llms-full.txt`: expanded project context for larger context windows.

`llms.txt` is an emerging convention, not an access-control or crawler policy
mechanism. Do not include secrets, private paths, customer data, or unpublished
plans in it.

## Agent Command Pattern

Recommended inspection flow:

```sh
bindport config explain
bindport config validate
bindport status --json
bindport doctor
```

Recommended service flow:

```sh
bindport run web
bindport open web --print
```

Recommended cleanup flow:

```sh
bindport clean --dry-run
bindport clean --stopped
```

Agents should ask before:

- trusting, denying, or resetting hooks.
- deleting stale entries with `--yes`.
- changing dashboard bind addresses or auth.
- editing machine-local overrides.
