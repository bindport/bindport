#!/usr/bin/env bash
set -euo pipefail

mdbook="${MDBOOK:-mdbook}"
out_dir="${BINDPORT_DOCS_OUT_DIR:-dist/docs}"
base_url="${BINDPORT_DOCS_BASE_URL:-}"
mdbook_args=()

while [[ $# -gt 0 ]]; do
  case "$1" in
    --base-url)
      if [[ $# -lt 2 ]]; then
        echo "--base-url requires a value" >&2
        exit 2
      fi
      base_url="$2"
      shift 2
      ;;
    --base-url=*)
      base_url="${1#--base-url=}"
      shift
      ;;
    --)
      shift
      mdbook_args+=("$@")
      break
      ;;
    *)
      mdbook_args+=("$1")
      shift
      ;;
  esac
done

if ! command -v "$mdbook" >/dev/null 2>&1; then
  echo "mdbook not found. Install with: cargo install mdbook --locked" >&2
  echo "Or set MDBOOK=/path/to/mdbook." >&2
  exit 127
fi

"$mdbook" build "${mdbook_args[@]}"

mkdir -p "$out_dir"
cp docs/llms.txt docs/llms-full.txt docs/robots.txt docs/status.schema.json "$out_dir"/
rm -f "$out_dir/sitemap.xml"
cargo run -p xtask -- docs-postprocess "$out_dir"

if [[ -n "$base_url" ]]; then
  cargo run -p xtask -- docs-sitemap "$out_dir" "$base_url"
fi
