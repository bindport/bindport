# Install BindPort

BindPort ships one Rust CLI through several install channels. Linux and macOS
are supported through v1; Windows is post-1.0.

Use the install path that matches how the project will call BindPort:

- JavaScript monorepos should usually use the npm package as a dev dependency.
- Rust-first users can install globally with Cargo.
- Workstations that prefer reviewed GitHub Release binaries can use
  `cargo binstall`, Homebrew, mise/ubi, or a direct release download.

BindPort does not provide a `curl | sh` installer. Release binaries, package
manager formulas, and package registries are the supported install surfaces.

## npm

For JavaScript, TypeScript, and Turborepo-style monorepos, install BindPort as a
development dependency so package scripts use the same version for every
developer and CI job:

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

The published `bindport` package is a small POSIX shell wrapper. It resolves the
matching native package for the current platform and executes the Rust binary.
The native package names are:

- `@bindport/linux-x64`
- `@bindport/linux-arm64`
- `@bindport/darwin-x64`
- `@bindport/darwin-arm64`

There is no postinstall download script.

## Cargo

For Rust-first projects or global developer installs:

```sh
cargo install bindport
```

Cargo builds from crates.io source on the local machine. Use `cargo binstall`
when you prefer GitHub Release binaries instead.

## cargo-binstall

BindPort release metadata supports `cargo binstall` for Linux and macOS x64 and
arm64 targets:

```sh
cargo binstall bindport
```

`cargo binstall` resolves the matching binary from GitHub Releases and verifies
the release metadata before installing.

## Homebrew

The BindPort Homebrew tap is the package-manager path for users who want
reviewed GitHub Release binaries plus shell completions and the man page:

```sh
brew install bindport/tap/bindport
```

The tap formula should install:

- the matching `bindport` binary for the platform;
- bash, zsh, and fish completions from `bindport-completions.tar.gz`;
- the `bindport.1` man page from `bindport-manpage.tar.gz`;
- checksummed GitHub Release assets only.

Homebrew core submission is deferred until after v1.0.

## mise / ubi

For users who manage developer tools with mise, install from the GitHub Release
binary through ubi:

```sh
mise use -g ubi:bindport/bindport
```

Project-local mise users can pin the tool in their own repo:

```toml
[tools]
"ubi:bindport/bindport" = "latest"
```

Use an explicit version instead of `latest` when the project needs reproducible
tooling.

## GitHub Releases

GitHub Releases contain raw binaries and checksums for:

- `bindport-linux-x64`
- `bindport-linux-arm64`
- `bindport-macos-x64`
- `bindport-macos-arm64`

They also contain npm tarballs, shell completion tarballs, the manpage tarball,
and `.sha256` checksum files.

Manual binary install:

```sh
gh release download v0.6.0 --repo bindport/bindport --pattern 'bindport-linux-x64*' --dir dist/bindport
cd dist/bindport
sha256sum -c bindport-linux-x64.sha256
install -m 0755 bindport-linux-x64 ~/.local/bin/bindport
```

Use the matching asset name for macOS or arm64 systems. On macOS, verify with
`shasum -a 256 -c <asset>.sha256` if `sha256sum` is not installed.

## Verify

After installation:

```sh
bindport --version
bindport doctor
bindport -- sh -c 'printf "PORT=%s\n" "$PORT"'
```

For project adoption, continue with [Adoption Setup](adoption.md).
