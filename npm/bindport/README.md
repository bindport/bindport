# bindport

Proxy-neutral local development port registry and runner.

This package provides the `bindport` command for JavaScript package managers.
It dispatches to one optional native package for the current platform:

- `@bindport/linux-x64`
- `@bindport/linux-arm64`
- `@bindport/darwin-x64`
- `@bindport/darwin-arm64`

Install it as a development dependency in a package or monorepo:

```sh
npm install --save-dev bindport
```

Then use it from scripts:

```json
{
  "scripts": {
    "dev": "bindport -- next dev"
  }
}
```

Cargo remains a supported install path:

```sh
cargo install bindport
bindport --help
```

From a repository checkout, use Cargo directly:

```sh
cargo run -p bindport -- --help
```
