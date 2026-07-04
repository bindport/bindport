# Quick Start

This guide runs BindPort without requiring a proxy, config file, or dashboard.
Use it to verify the CLI and the local registry before adopting project config.

## Install

Use the install path that matches the project. JavaScript monorepos usually use
npm; Rust-first users often use Cargo:

```sh
npm install --save-dev bindport
```

```sh
cargo install bindport
```

See [Install BindPort](install.md) for every supported channel.

## Run A Command

Run any command after `--`:

```sh
bindport -- sh -c 'printf "PORT=%s\n" "$PORT"; sleep 30'
```

BindPort assigns a port, injects `PORT`, records the run, and waits for the
child command.

## Inspect Status

In another terminal:

```sh
bindport status
bindport status --json
```

The service appears as `active` while the child process is running. After the
child exits, it appears as `stopped` until cleanup removes it.

## Open A URL

For a service that is still running:

```sh
bindport open sh --print
```

`open` prints the configured route URL when one exists. Otherwise it prints the
direct loopback URL for the assigned port.

## Add A Named Service

Use `run <service>` to make the service name explicit:

```sh
bindport run web -- sh -c 'printf "PORT=%s\n" "$PORT"; sleep 30'
```

The same project/service identity should reuse the previous free port when it
is available.

## Initialize Project Config

From a project root:

```sh
bindport init
bindport config validate
bindport config explain
```

Commit `.bindport.toml` only when it describes shared project behavior. Put
developer-specific values in `.bindport.local.toml`.

## Run The Dashboard

```sh
bindport dashboard serve
```

The dashboard defaults to `127.0.0.1:27080`. For remote development boxes,
enable auth before binding to `0.0.0.0`:

```sh
BINDPORT_DASHBOARD_TOKEN="change-me" \
  bindport dashboard serve --host 0.0.0.0 --auth required
```

## Clean Old State

Preview cleanup:

```sh
bindport clean --dry-run
```

Remove stopped entries:

```sh
bindport clean --stopped
```

Remove stale entries after review:

```sh
bindport clean --stale --yes
```

## Next Steps

- [Adoption Setup](adoption.md) for adding BindPort to a real project.
- [Configuration](../daily-use/configuration.md) for services, route metadata, outputs, hooks, and
  dashboard settings.
- [Running Services](../daily-use/running-services.md) for framework-specific command and
  environment patterns.
