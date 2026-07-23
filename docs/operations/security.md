# Security And Privacy

This is the canonical BindPort security and privacy contract. BindPort is a
local development tool, not a sandbox, secret manager, process owner, or
security boundary between mutually untrusted users.

## Trust Boundaries

BindPort itself runs with the invoking user's filesystem, process, and network
permissions. Its defaults do not require root or select privileged ports.
BindPort does not install certificates, edit `/etc/hosts`, mutate DNS, install a
system daemon, or kill active wrapped services during normal cleanup.

The following inputs must be treated according to who controls them:

- **Project config and templates** can choose configured child commands,
  application environment values, route metadata, output content, and output
  names. Review repository changes before running configured services or
  rendering outputs.
- **Wrapped commands** are arbitrary programs. They inherit the ambient child
  environment and stdio and can access files and networks as the user.
- **Hooks** are arbitrary programs behind a local trust decision. Trust
  controls whether a matching definition may run; it does not sandbox it.
- **Custom templates and `vars`** control generated text and can intentionally
  place supplied values into files. BindPort does not classify or scrub template
  values as secrets.
- **The dashboard** exposes local registry data and cleanup actions over HTTP.
  Remote binding changes that from a loopback-only interface into a network
  service.

## Config Discovery And Parsing

BindPort walks upward from the current directory and uses the first project
config it finds. Within one directory, `.bindport.toml` wins over
`.bindport.json`, which wins over `.bindport.yaml`. A matching
`.bindport.local.*` or `bindport.local.*` file beside project config is merged
as a machine-local override. If no project config is found, BindPort can read
the user fallback at `$XDG_CONFIG_HOME/bindport/config.toml` or
`~/.config/bindport/config.toml`.

Keep local overrides untracked and inspect `bindport config explain` before
running commands in an unfamiliar checkout. TOML, JSON, and YAML config files
and custom templates are UTF-8 text. YAML config is limited to 256 KiB and rejects anchors
and aliases. Unknown top-level keys are ignored but reported by `config
validate`, `config explain`, and `doctor`; unknown nested keys are currently
ignored. Unknown keys are not an extension or authorization mechanism.

Service paths and project-config output roots must be relative to the config
file and must not contain `..`. A fallback-config output resolves from the
invoking cwd. Configured service paths are canonicalized before spawn and must remain under
the project config root. This path validation limits accidental escape; it is not a sandbox against another local process changing the
filesystem concurrently.

## Child Environment And Executable Lookup

Configured service `env` cannot set execution-sensitive names such as `PATH`,
a name beginning with `LD_`, `DYLD_`, `MALLOC_`, or `GIT_CONFIG_`, or the
blocked shell/language loader variables documented in
[Configuration](../daily-use/configuration.md). BindPort ignores those names at
runtime as well as reporting them during validation. An explicit one-shot
`--env NAME=VALUE` can set them because the caller has requested that override.

A wrapped child otherwise inherits the invoking process environment. BindPort
does not scrub credentials already present there. Environment values are not
stored as dedicated registry fields, but expanded command arguments and route
metadata are recorded; do not put secrets in command-line arguments, hostnames,
or URLs.

For a configured service path, BindPort prepends only existing
`node_modules/.bin` directories from the service directory up to the detected
project/package-workspace boundary, then preserves the ambient `PATH`. It does
not walk above that boundary. This reduces accidental lookup outside the
project but does not authenticate executables: project-local binaries and the
remaining ambient `PATH` are trusted inputs. Commands without a configured
service path use the invoker's cwd and ambient lookup behavior.

## Registry And Other Local State

The SQLite registry defaults to:

- `$XDG_STATE_HOME/bindport/registry.sqlite`, or
- `~/.local/state/bindport/registry.sqlite` when `XDG_STATE_HOME` is unset.

It stores lease and run identity, ports, PIDs, process start data where
available, full expanded command lines, working directories, git/worktree and
branch metadata, route and health URLs, exit state, output paths/hashes/status,
and render scheduling state. It has no dedicated credential or application
environment table, but command arguments, URLs, paths, output metadata, or
user-chosen labels can still contain sensitive values. BindPort does not perform
secret detection or redaction in the registry.

On Unix, BindPort rejects a registry database path that is itself a symlink and
sets the registry directory and database it creates to mode `0700` and `0600`
respectively. These permissions protect against other ordinary users; the
registry is not encrypted and remains readable by the account and privileged
system software. Supported schema migrations are transactional and preserve
active/reserved leases, but BindPort does not create an automatic backup. See
[Registry Migration Policy](../reference/registry-migrations.md).

Other state under the BindPort state directory includes:

- `hooks-trust.json`, containing trust subjects, decisions, full hook
  definitions/argv, target fingerprints, hashes, and timestamps;
- `dashboard.state`, containing the background dashboard PID, URL, and Linux
  process-start value when available; and
- `dashboard.log`, containing background dashboard stderr.

Those files are local state, not encrypted secret storage. Their creation uses
the user's state directory and normal filesystem protections; only the registry
has the explicit permission-hardening contract above.

`status --json`, `list --json`, the dashboard, and registry export can disclose
project names, commands, cwd/worktree paths, branches, PIDs, ports, URLs, and
output paths. `registry export` additionally includes raw rows and the registry
path. Config diagnostics reveal discovered config paths and identity; hooks
status reveals configured argv; template show/export prints the selected custom
or built-in template contents. Review and redact outputs before sharing them.
BindPort does not promise to diagnose full process ownership from this data.

## Hooks

Checked-in config can declare hooks but cannot approve execution. A user must
approve a reviewed definition with `bindport hooks trust`. Decisions default to
the exact worktree; repository scope must be selected explicitly. A changed
command definition is blocked until re-approved. For path-like local commands,
BindPort also fingerprints the target file; commands resolved from `PATH` are
opaque and are not fingerprinted.

Hooks are structured argv and run directly, without an implicit shell. An
explicit `sh -c` or equivalent opts into shell interpretation. Hook cwd is the
discovered config directory. The hook process receives an environment cleared
to `PATH` plus `BINDPORT_HOOK_EVENTS`, `BINDPORT_HOOK_SOURCES`, and
`BINDPORT_HOOK_CONTEXT`; it does not inherit `HOME`, cloud credentials, or other
ambient values by default. Hook stdin is null, while stdout and stderr are
inherited and can mix with the invoking CLI's output. Hook argv remains visible
in config, status, trust state, and process inspection.

Hooks have bounded timeouts. On Unix, timeout cleanup targets the hook process
group. This is lifecycle control, not containment: an approved hook can access
local files, start detached processes, contact networks, or invoke tools such as
Docker and `kubectl` with whatever access its own setup provides. Dashboard
hook actions are visibility-only; trust, deny, and reset remain CLI-only.

## Templates And Output Ownership

Template lookup is by a safe logical name under the project template directory,
the global BindPort template directory, or built-ins. MiniJinja uses strict
undefined values, no automatic escaping, a fuel budget, and a 1 MiB rendered
file limit. Template authors must quote the destination format correctly.
Templates receive registry route context and user-provided output vars, so a
custom template can disclose that data in its generated file.

Project-config output roots are project-relative; fallback-config outputs use
the invoking cwd as their base. Targets must remain under their resolved root,
and target paths may not traverse symlink components. BindPort writes via a
sibling temporary file and rename and creates output temp files with mode
`0600` on Unix. Normal rendering refuses to overwrite an unowned file or a
DB-owned file whose content no longer matches its recorded hash. Repair can
adopt only content-identical planned files.

Ownership hashes prevent accidental overwrite; they are not signatures or
protection against a malicious process running as the same user. BindPort does
not apply generated files to proxies, clusters, or containers by itself. A
watcher, approved hook, or other user-run tool is a separate trust boundary.

## Dashboard And Browser Opening

The dashboard binds to `127.0.0.1` by default. BindPort refuses a non-loopback
IPv4 bind unless bearer auth is required and a token is available. Host-header
checks apply before routing. When auth is enabled, registry status and cleanup
APIs require `Authorization: Bearer <token>`; cleanup also requires
`X-BindPort-Dashboard-Action: clean`.

The HTML shell and static assets remain available without auth so they can show
the token prompt. `/healthz` is also unauthenticated. Release builds serve
embedded assets; the explicit development `--static-dir` option serves the
known dashboard asset names from that local directory, so do not point it at
unreviewed sensitive content. The browser stores the
provided token in tab-scoped `sessionStorage`. Binding `0.0.0.0` with auth
accepts arbitrary Host values; a loopback bind reached through another hostname
requires that hostname in `allowed_hosts`.

Prefer `--token-env` or `dashboard.auth.token_env`. A config `token` is stored
in plaintext config, and foreground `--token` can appear in shell history and
process arguments. `dashboard start --token` transfers the value to the child
through the selected environment name rather than keeping it in the detached
server argv. Dashboard tokens are not written to the registry, but BindPort is
plain HTTP and provides no TLS, token rotation, user accounts, or remote-access
firewall. For remote use, combine a strong token with a trusted private network
or authenticated tunnel.

`bindport open --print` only prints a URL. `--browser` accepts only `http://` or
`https://`, then invokes `open` on macOS or `xdg-open` on Linux. The launcher and
browser are external processes, and opening a user-configured route can make
network requests or disclose the URL to browser history and extensions.

## Network And Subprocess Activity

The BindPort runtime has no telemetry, analytics, hosted dependency, or update
check. It does perform these local network operations:

- TCP bind probes on IPv4 and IPv6 loopback for port availability;
- optional HTTP health probes when a registry snapshot is built for status,
  list, open, rendering, or the dashboard, limited to loopback IPs, `localhost`,
  and `*.localhost`, with no DNS lookup, HTTPS, redirect following, or
  non-loopback destination;
- the dashboard HTTP listener on its configured IPv4 address; and
- browser launching when explicitly requested.

Wrapped children and approved hooks are arbitrary and can perform any network
or subprocess activity available to the user. Package managers, release
installation commands, and maintainer release tooling can also access their
registries or GitHub; they are outside the installed CLI runtime boundary.

## Safe Use

- Review project config, configured commands, local binaries, templates, and
  hooks before using an unfamiliar checkout.
- Keep secrets out of command arguments, route URLs, output vars, generated
  files, and tracked config. Use application-appropriate environment or secret
  tooling.
- Keep local overrides and the BindPort state directory private. Redact status,
  dashboard screenshots, logs, and registry exports before sharing.
- Run `bindport config validate`, `bindport doctor outputs`, and `bindport
  render --diff` before executing or writing changed integrations.
- Do not approve hooks merely to silence a pending warning. Prefer file watchers
  over hooks when possible.
- Keep the dashboard loopback-only unless remote access is necessary; require
  auth and a trusted transport when exposing it.
- Use `bindport clean --dry-run` before deletion. BindPort cleanup removes
  stopped/stale registry state and matching owned outputs, not arbitrary active
  processes.
