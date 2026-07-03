#!/usr/bin/env bash
set -euo pipefail

dest="${1:-dist}"
repo_root="$(git rev-parse --show-toplevel)"
cd "$repo_root"

mkdir -p "$dest"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

mkdir -p "$tmp/completions/bash" "$tmp/completions/zsh" "$tmp/completions/fish"
cp packaging/completions/bindport.bash "$tmp/completions/bash/bindport"
cp packaging/completions/bindport.zsh "$tmp/completions/zsh/_bindport"
cp packaging/completions/bindport.fish "$tmp/completions/fish/bindport.fish"
tar -C "$tmp" -czf "$dest/bindport-completions.tar.gz" completions

mkdir -p "$tmp/man/man1"
cp packaging/man/bindport.1 "$tmp/man/man1/bindport.1"
tar -C "$tmp" -czf "$dest/bindport-manpage.tar.gz" man

checksum() {
  local file="$1"
  local name
  name="$(basename "$file")"
  if command -v sha256sum >/dev/null 2>&1; then
    (cd "$dest" && sha256sum "$name" > "$name.sha256")
  else
    (cd "$dest" && shasum -a 256 "$name" > "$name.sha256")
  fi
}

checksum "$dest/bindport-completions.tar.gz"
checksum "$dest/bindport-manpage.tar.gz"
