# Security Model

BindPort is designed for local development. Its defaults should be safe for a
developer machine without requiring a privileged daemon or broad network
exposure.

## Local-First Defaults

BindPort does not:

- bind privileged ports.
- install certificates.
- edit `/etc/hosts`.
- mutate DNS.
- install a root-owned service.
- kill active processes by default.
- execute hooks without local trust.

The dashboard binds to `127.0.0.1` by default. Non-loopback dashboard binds
require auth.

## Config Safety

Shared config should describe project behavior. Machine-specific values belong
in `.bindport.local.*` or `bindport.local.*`, which should stay untracked.

Output roots must be relative to the config file and must not contain `..`.
Service paths and output targets are validated before use.

Service `env` is for application-level values. Config cannot set names that can
change process loading or tool configuration, including `PATH`, `LD_PRELOAD`,
`LD_LIBRARY_PATH`, `DYLD_*`, `NODE_OPTIONS`, `BASH_ENV`, `ENV`, language
package path variables, shell path variables, and `GIT_CONFIG_*`.

## Registry State

Registry state lives outside the project by default:

- `$XDG_STATE_HOME/bindport/registry.sqlite`
- `~/.local/state/bindport/registry.sqlite`

On Unix platforms, BindPort hardens registry directories and files it creates.
Registry data can include command lines, project names, branch names, and local
paths, so it should be treated as local developer-machine state.

## Output Ownership

BindPort records generated output files in the registry with content hashes.
It overwrites a file only when BindPort owns the file and the on-disk content
still matches the recorded hash.

Unowned or externally modified files cause normal rendering to fail instead of
being overwritten. `bindport render --repair` reconciles DB-owned files and can
adopt content-identical planned files whose ownership row was lost. Files with
different content are never adopted.

## Hook Trust

Checked-in config can declare hooks, but it cannot approve hook execution.
Users must trust hooks locally with `bindport hooks trust`.

Trust is invalidated when:

- the hook command definition changes.
- a local command target file changes.

Dashboard hook actions are read-only. Trust, deny, and reset actions are
CLI-only.

## Dashboard Auth

Dashboard HTML and assets can load without auth so the browser can show a token
prompt. Registry data and cleanup APIs require bearer auth when dashboard auth
is enabled.

Prefer `--token-env` or config `dashboard.auth.token_env` over `--token` so
the token does not appear in shell history or process arguments.

Cleanup APIs require an action header so simple cross-site form posts cannot
trigger cleanup in a browser.

## Agent Safety

AI coding agents should:

- use existing project scripts or `bindport run <service>`.
- run `bindport config validate` after changing config.
- inspect `bindport status --json` instead of hardcoding ports.
- avoid editing `.bindport.local.*`, generated output files, `.env.local`, and
  hook trust state unless explicitly asked.
- never run `bindport hooks trust`, `deny`, or `reset` without user approval.
