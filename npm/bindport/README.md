# bindport npm wrapper

Bootstrap placeholder for the future BindPort npm package.

The published package will provide a `bindport` command that dispatches to the
native binary for the current platform. Do not publish this package until the
native binary install/dispatch path is real. See
[`docs/release.md`](../../docs/release.md) for the release policy.

During repository bootstrap, use Cargo directly:

```sh
cargo run -p bindport -- --help
```
