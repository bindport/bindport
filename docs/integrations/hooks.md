# Hooks and Trust

Hooks let BindPort run local commands when route or output lifecycle events
happen. They are useful for tasks such as asking an existing proxy to reload
after generated files change.

Hooks are powerful local code execution. BindPort therefore treats every hook as
disabled until the user explicitly trusts it with the CLI.

Prefer file-watching integrations when possible. If an optional output needs an
explicit reload or apply step, use a small reviewed hook and keep the generated
files as the inspectable source of truth. See
[Optional Output Patterns](optional-output-patterns.md) for TCP alias,
Kubernetes, container, and JSON bridge examples.

## Configure Hooks

Example:

```toml
[hooks]
timeout_ms = 5000

[[hooks.commands]]
name = "reload-proxy"
events = ["output_rendered"]
command = ["docker", "kill", "-s", "HUP", "traefik"]
timeout_ms = 2000
```

Hook command arrays are structured argv, not shell strings. Use
`["sh", "-c", "..."]` only when shell behavior is intentional and reviewed.

## Paths And Working Directory

Relative hook command paths are resolved from the directory that contains the
discovered BindPort config. Hook processes also run with that directory as
their working directory.

In a monorepo, this lets package scripts run from `apps/web` while hooks live at
the repository root:

```toml
[[hooks.commands]]
name = "reload-proxy"
events = ["output_rendered"]
command = ["./ops/localhost/reload-proxy.sh"]
```

If `.bindport.toml` lives at the repository root, the command above resolves to
`<repo>/ops/localhost/reload-proxy.sh` no matter which service directory
launched `bindport run`.

## Events

Supported events:

- `route_started`: a service was recorded active or reserved.
- `route_finished`: a wrapped service exited.
- `routes_removed`: registry entries were removed by cleanup.
- `routes_marked_stale`: registry entries were reconciled as stale.
- `render_requested`: a render operation was requested.
- `output_rendered`: one or more output files were rendered.

Hooks can subscribe to one or more events. BindPort passes event metadata
through environment variables, not through secrets or inherited shell state.

## Trust Workflow

Inspect configured hooks:

```sh
bindport hooks status
```

Trust a reviewed hook in the current worktree:

```sh
bindport hooks trust reload-proxy
```

Deny a hook:

```sh
bindport hooks deny reload-proxy
```

Reset a hook decision:

```sh
bindport hooks reset reload-proxy
```

Trust scope defaults to `worktree`. Use `--scope repo` only when the same hook
definition should be trusted across worktrees that share the same git repo:

```sh
bindport hooks trust --scope repo reload-proxy
```

## Changed Hooks

Trust is tied to the hook definition. If the command definition changes,
BindPort marks the hook as changed and blocks execution until it is reviewed
again.

When the command target is a local path such as `./scripts/reload-proxy`,
BindPort fingerprints that file too. Changes to the target file invalidate the
trust decision.

Commands resolved from `PATH`, such as `docker`, are opaque targets. BindPort
can trust the configured command definition, but it cannot fingerprint every
external executable that may be found by the user's shell environment.

## Environment

Hook processes receive a minimal environment:

- `PATH` from the parent process.
- `BINDPORT_HOOK_EVENTS`
- `BINDPORT_HOOK_SOURCES`
- `BINDPORT_HOOK_CONTEXT`

Other parent environment values are not inherited. Secret values are not copied
into hook metadata or the registry.

Hooks that call tools such as `kubectl`, `docker`, or cloud CLIs should declare
the environment they need inside the hook or through reviewed wrapper scripts.
For example, a Kubernetes reload hook may need to set `HOME` and `KUBECONFIG`
before invoking `kubectl`.

## Dashboard Visibility

The dashboard can show hook state, including pending, approved, denied, and
changed hooks. Approval and denial remain CLI-only so a browser session cannot
grant local command execution.
