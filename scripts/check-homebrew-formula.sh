#!/usr/bin/env bash
set -euo pipefail

repo_root="$(git rev-parse --show-toplevel)"
cd "$repo_root"

bash -n scripts/homebrew-formula.sh

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

write_sha() {
  local asset="$1"
  local sha="$2"
  printf '%s  %s\n' "$sha" "$asset" > "$tmp/$asset.sha256"
}

write_sha bindport-linux-x64 1111111111111111111111111111111111111111111111111111111111111111
write_sha bindport-linux-arm64 2222222222222222222222222222222222222222222222222222222222222222
write_sha bindport-macos-x64 3333333333333333333333333333333333333333333333333333333333333333
write_sha bindport-macos-arm64 4444444444444444444444444444444444444444444444444444444444444444
write_sha bindport-completions.tar.gz 5555555555555555555555555555555555555555555555555555555555555555
write_sha bindport-manpage.tar.gz 6666666666666666666666666666666666666666666666666666666666666666

formula="$tmp/bindport.rb"
scripts/homebrew-formula.sh --version 0.6.0 --dist "$tmp" --output "$formula"

grep -q 'class Bindport < Formula' "$formula"
grep -q 'version "0.6.0"' "$formula"
grep -q 'url "https://github.com/bindport/bindport/releases/download/v0.6.0/bindport-linux-x64"' "$formula"
grep -q 'url "https://github.com/bindport/bindport/releases/download/v0.6.0/bindport-linux-arm64"' "$formula"
grep -q 'url "https://github.com/bindport/bindport/releases/download/v0.6.0/bindport-macos-x64"' "$formula"
grep -q 'url "https://github.com/bindport/bindport/releases/download/v0.6.0/bindport-macos-arm64"' "$formula"
grep -q 'resource "completions"' "$formula"
grep -q 'resource "manpage"' "$formula"
grep -q 'bash_completion.install "completions/bash/bindport"' "$formula"
grep -q 'zsh_completion.install "completions/zsh/_bindport"' "$formula"
grep -q 'fish_completion.install "completions/fish/bindport.fish"' "$formula"
grep -q 'man1.install "man/man1/bindport.1"' "$formula"

if grep -q "$tmp" "$formula"; then
  echo "formula contains a local temporary path" >&2
  exit 1
fi

if command -v ruby >/dev/null 2>&1; then
  ruby -c "$formula" >/dev/null
fi
