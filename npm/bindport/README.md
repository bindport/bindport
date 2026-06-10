# bindport npm wrapper

Bootstrap placeholder for the future BindPort npm package.

The published package will provide a `bindport` command that dispatches to the
native binary for the current platform. During repository bootstrap, use Cargo
directly:

```sh
cargo run -p bindport -- --help
```
