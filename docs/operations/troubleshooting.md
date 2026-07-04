# Health and Troubleshooting

Use this page when a service does not appear where expected, a URL does not
open, output files are missing, or hooks are not running.

## Start With Doctor

Run:

```sh
bindport doctor
```

`doctor` reports:

- discovered config and registry paths.
- effective project/service identity.
- active registry ports.
- obvious registry/listener conflicts.
- unknown OS listener conflicts.
- the next candidate port.

For output issues:

```sh
bindport doctor outputs
```

That checks output config, template lookup, planned file paths, ownership
rules, and hook trust status without writing files.

## Config Issues

Check discovery and precedence:

```sh
bindport config explain
```

Validate structure:

```sh
bindport config validate
```

Common problems:

- Running from a directory outside the intended project config tree.
- A local override in `.bindport.local.*` changing host, port range, outputs, or
  dashboard settings.
- Duplicate service names.
- A service `path` that does not exist or escapes the config root.
- YAML anchors or aliases in a YAML config file.

## Port Issues

If the selected port is unexpected:

1. Check `default_range` and `skip_ports`.
2. Run `bindport status --json` and look for active or reserved entries.
3. Run `bindport doctor` to see the next candidate and listener conflicts.
4. Check whether another process claimed the port after BindPort's probe.

BindPort retries once when a child fails immediately and the assigned port is
then occupied, but it cannot prevent every external race.

## URL Issues

`bindport open` uses `route_url` when configured and direct `url` otherwise:

```sh
bindport open web --print
```

If the printed URL is wrong, check:

- service `hostname`, `route_url`, and `health_url`.
- CLI overrides such as `--hostname` or `--route-url`.
- environment overrides such as `BINDPORT_ROUTE_URL`.
- whether the external proxy is watching the generated output directory.

## Health Issues

Active loopback `http://` health URLs can report:

- `pending`: still within startup grace.
- `healthy`: 2xx or 3xx response.
- `failing`: probe failed.
- `unknown`: no supported local health URL is configured.

Non-loopback and unsupported destinations remain `unknown`. BindPort does not
use DNS for `localhost` or `*.localhost`; those names are treated as local
targets.

## Dashboard Issues

Foreground serve:

```sh
bindport dashboard serve
```

Background service state:

```sh
bindport dashboard status
bindport dashboard stop
```

Remote dev boxes must enable auth before binding to a non-loopback address:

```sh
BINDPORT_DASHBOARD_TOKEN="change-me" \
  bindport dashboard serve --host 0.0.0.0 --auth required
```

If a background dashboard fails to start, check `dashboard.log` in the BindPort
state directory.

## Output Issues

Preview before writing:

```sh
bindport render --dry-run
```

Repair DB-owned output files:

```sh
bindport render --repair
```

Common causes:

- Template name does not match project, global, or built-in lookup.
- Output `root` is not relative to the config file.
- Output `target` escapes the output root.
- Existing file is unowned or externally modified.
- Required route metadata is missing, such as `hostname`.

## Hook Issues

Inspect trust:

```sh
bindport hooks status
```

Hooks do not run until trusted. If a hook or local target file changes, the
trust state becomes changed and the hook must be reviewed again.

## Cleanup Issues

Preview cleanup:

```sh
bindport clean --dry-run
```

Stale cleanup requires confirmation:

```sh
bindport clean --stale --yes
```

Cleanup never removes active or reserved services.
