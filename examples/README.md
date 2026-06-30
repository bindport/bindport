# BindPort Examples

Example config files show the same starter service in TOML, JSON, and YAML.
TOML is the reference format, then JSON, then YAML when equivalent files exist.
The examples include service env templates, route hostname metadata, output
template config, and the optional local dashboard settings: loopback bind,
preferred dashboard port, allowed Host headers, and token-env based auth.

- [`.bindport.toml`](config/.bindport.toml)
- [`.bindport.json`](config/.bindport.json)
- [`.bindport.yaml`](config/.bindport.yaml)

The TOML example renders one Traefik file-provider config per route under
`.bindport/generated/traefik`. Its service hostname template:

```toml
hostname = "{branch}.{project}.localhost"
```

means a branch such as `feature/tree` in project `example-app` becomes
`feature-tree.example-app.localhost`. For project-specific local domains, set
the project name or service hostname accordingly, for example:

```toml
project = "orderful-website"

[[services]]
name = "web"
path = "."
hostname = "{branch}.orderful-website.localhost"
```

For a monorepo, keep one config at the repo root and scope services by relative
path. Running BindPort from inside `apps/api` selects the `api` service unless a
CLI or environment service override is provided:

```toml
project = "orderful"

[[services]]
name = "web"
path = "apps/web"

[[services]]
name = "api"
path = "apps/api"
```
