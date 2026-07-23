#!/usr/bin/env bash
set -euo pipefail

mdbook="${MDBOOK:-mdbook}"
out_dir="${BINDPORT_DOCS_OUT_DIR:-dist/docs}"
repo_root="$(git rev-parse --show-toplevel)"
cd "$repo_root"

if [[ "$out_dir" != /* ]]; then
  out_dir="$repo_root/$out_dir"
fi

if ! command -v "$mdbook" >/dev/null 2>&1; then
  echo "mdbook not found. Install with: cargo install mdbook --locked" >&2
  echo "Or set MDBOOK=/path/to/mdbook." >&2
  exit 127
fi

cargo build -p xtask >/dev/null
xtask="${BINDPORT_XTASK:-$repo_root/target/debug/xtask}"

copy_static() {
  if [[ -f "$out_dir/index.html" ]]; then
    cp docs/llms.txt docs/llms-full.txt docs/robots.txt docs/config.schema.json docs/status.schema.json "$out_dir"/
    "$xtask" docs-postprocess "$out_dir"
  fi
}

args=("$@")
if [[ ${#args[@]} -eq 0 ]]; then
  args=(-n 127.0.0.1 -p 4321)
fi

"$mdbook" serve "${args[@]}" &
server_pid="$!"

while kill -0 "$server_pid" 2>/dev/null; do
  copy_static || true
  sleep 2
done &
copy_pid="$!"

cleanup() {
  kill "$copy_pid" 2>/dev/null || true
  kill "$server_pid" 2>/dev/null || true
}
trap cleanup EXIT INT TERM

wait "$server_pid"
