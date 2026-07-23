# Install BindPort

BindPort ships one Rust CLI for supported Linux and macOS x64/arm64 hosts.
Windows remains post-1.0. Choose the channel that matches how the project will
invoke BindPort:

- JavaScript monorepos should normally pin the npm package as a dev dependency.
- Rust-first users can build from source with Cargo or use the GitHub binary
  selected by `cargo binstall`.
- Homebrew tap, mise/ubi, and direct-download users consume the same GitHub
  Release binary names.

Channel availability is release-specific. BindPort CI verifies package metadata,
local tarballs, checksums, formula generation, and host-compatible staged
execution without contacting public registries. That local smoke does not prove
that a particular version has been published to npm, crates.io, GitHub, or the
external Homebrew tap.

BindPort does not provide a `curl | sh` installer. Package managers and explicit,
checksummed release downloads are the supported install surfaces.

## npm

Install the wrapper as a development dependency so package scripts pin one
version for developers and CI:

```sh
npm install --save-dev bindport
```

Then call the local executable from scripts:

```json
{
  "scripts": {
    "dev:web": "bindport run web",
    "bindport:status": "bindport status --json",
    "bindport:doctor": "bindport doctor"
  }
}
```

The published `bindport` package is a POSIX shell wrapper with no postinstall
download. It executes the Rust binary from exactly one optional native package:

- `@bindport/linux-x64`
- `@bindport/linux-arm64`
- `@bindport/darwin-x64`
- `@bindport/darwin-arm64`

The npm launcher therefore requires a POSIX environment and is not a Windows
launcher.

## Cargo

Build the published source package on a supported host with Rust 1.96.0 or
newer:

```sh
cargo install bindport
```

This reaches crates.io and compiles locally. The repository release smoke checks
`cargo package -p bindport --list` and the release binary, but deliberately does
not install or publish a crates.io package.

## cargo-binstall

Install the matching prebuilt GitHub Release binary:

```sh
cargo binstall bindport
```

The crate metadata maps these four Rust targets to the raw release assets:

| Rust target | Release asset |
|---|---|
| `x86_64-unknown-linux-gnu` | `bindport-linux-x64` |
| `aarch64-unknown-linux-gnu` | `bindport-linux-arm64` |
| `x86_64-apple-darwin` | `bindport-macos-x64` |
| `aarch64-apple-darwin` | `bindport-macos-arm64` |

The metadata selects an asset; use the direct-download procedure below when you
want to verify the published `.sha256` sidecar explicitly.

## Homebrew Tap

For a version published to the BindPort tap:

```sh
brew install bindport/tap/bindport
```

This is a project tap, not homebrew-core. The generated formula points only at
the four checksummed GitHub Release binaries and installs:

- the host-compatible `bindport` binary;
- bash, zsh, and fish completions from `bindport-completions.tar.gz`; and
- `bindport.1` from `bindport-manpage.tar.gz`.

Ordinary pull-request CI generates and syntax-checks the formula but does not
run `brew install`: an install would mutate the host Homebrew prefix and reach a
published release. The authorized live-channel checklist is in
[Release Process](../project/release.md#authorized-v100-rc1-live-channel-checklist).
Homebrew core submission remains post-1.0.

## mise / ubi

mise can delegate GitHub Release discovery to its ubi backend:

```sh
mise use -g ubi:bindport/bindport
```

A project can pin the tool instead of changing global mise state:

```toml
[tools]
"ubi:bindport/bindport" = "X.Y.Z"
```

Replace `X.Y.Z` with a published version. BindPort has no separate mise plugin or
manifest; this path depends on ubi recognizing the documented GitHub asset
names. CI validates those names and their mappings, but does not download or
install through mise/ubi.

## GitHub Releases

Each release uses these raw binary and checksum names:

- `bindport-linux-x64` and `bindport-linux-x64.sha256`
- `bindport-linux-arm64` and `bindport-linux-arm64.sha256`
- `bindport-macos-x64` and `bindport-macos-x64.sha256`
- `bindport-macos-arm64` and `bindport-macos-arm64.sha256`

It also contains versioned npm tarballs and checksums,
`bindport-completions.tar.gz` with its checksum, and
`bindport-manpage.tar.gz` with its checksum.

Example for Linux x64:

```sh
VERSION=X.Y.Z
ASSET=bindport-linux-x64
mkdir -p dist/bindport
gh release download "v${VERSION}" \
  --repo bindport/bindport \
  --pattern "${ASSET}" \
  --pattern "${ASSET}.sha256" \
  --dir dist/bindport
(
  cd dist/bindport
  sha256sum -c "${ASSET}.sha256"
)
install -m 0755 "dist/bindport/${ASSET}" "$HOME/.local/bin/bindport"
```

Choose the matching asset from the list above. On macOS, use
`shasum -a 256 -c "${ASSET}.sha256"` when `sha256sum` is unavailable.

## Verify

After any install:

```sh
bindport --version
bindport --help
bindport doctor
bindport -- sh -c 'printf "PORT=%s\n" "$PORT"'
```

For project adoption, continue with [Adoption Setup](adoption.md).
