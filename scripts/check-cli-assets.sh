#!/usr/bin/env bash
set -euo pipefail

repo_root="$(git rev-parse --show-toplevel)"
cd "$repo_root"

required=(
  packaging/completions/bindport.bash
  packaging/completions/bindport.zsh
  packaging/completions/bindport.fish
  packaging/man/bindport.1
)

for file in "${required[@]}"; do
  if [[ ! -s "$file" ]]; then
    echo "missing CLI asset: $file" >&2
    exit 1
  fi
done

bash -n packaging/completions/bindport.bash
bash -n scripts/stage-cli-assets.sh

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
scripts/stage-cli-assets.sh "$tmp"

expected=(
  bindport-completions.tar.gz
  bindport-completions.tar.gz.sha256
  bindport-manpage.tar.gz
  bindport-manpage.tar.gz.sha256
)

for file in "${expected[@]}"; do
  if [[ ! -s "$tmp/$file" ]]; then
    echo "missing staged CLI artifact: $file" >&2
    exit 1
  fi
done

tar -tzf "$tmp/bindport-completions.tar.gz" | sort > "$tmp/completions.list"
tar -tzf "$tmp/bindport-manpage.tar.gz" | sort > "$tmp/manpage.list"

grep -qx 'completions/bash/bindport' "$tmp/completions.list"
grep -qx 'completions/fish/bindport.fish' "$tmp/completions.list"
grep -qx 'completions/zsh/_bindport' "$tmp/completions.list"
grep -qx 'man/man1/bindport.1' "$tmp/manpage.list"

(
  cd "$tmp"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum -c ./*.sha256
  else
    shasum -a 256 -c ./*.sha256
  fi
)
