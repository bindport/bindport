#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: scripts/npm-stage-platform-package.sh <platform> <binary> <dist-dir>

Packs one scoped platform npm package using a release-built BindPort binary.
Platforms: darwin-arm64, darwin-x64, linux-arm64, linux-x64.
USAGE
}

die() {
  echo "npm-stage-platform-package: $*" >&2
  exit 1
}

[[ "$#" -eq 3 ]] || {
  usage >&2
  exit 2
}

platform="$1"
binary="$2"
dist_dir="$3"

case "$platform" in
  darwin-arm64)
    package_dir="npm/bindport-darwin-arm64"
    ;;
  darwin-x64)
    package_dir="npm/bindport-darwin-x64"
    ;;
  linux-arm64)
    package_dir="npm/bindport-linux-arm64"
    ;;
  linux-x64)
    package_dir="npm/bindport-linux-x64"
    ;;
  *)
    usage >&2
    die "unsupported platform: $platform"
    ;;
esac

[[ -f "$binary" ]] || die "binary not found: $binary"
[[ -d "$package_dir" ]] || die "package directory not found: $package_dir"
[[ -f LICENSE ]] || die "repository LICENSE not found"

tmp_root="$(mktemp -d)"
npm_cache_root=""
if [[ -z "${npm_config_cache:-${NPM_CONFIG_CACHE:-}}" ]]; then
  npm_cache_root="$(mktemp -d)"
  export npm_config_cache="$npm_cache_root/cache"
fi
cleanup() {
  rm -rf "$tmp_root"
  if [[ -n "$npm_cache_root" ]]; then
    rm -rf "$npm_cache_root"
  fi
}
trap cleanup EXIT

tmp_package="$tmp_root/$(basename "$package_dir")"
mkdir -p "$tmp_package"
cp -R "$package_dir"/. "$tmp_package"/
cp LICENSE "$tmp_package/LICENSE"
mkdir -p "$tmp_package/bin"
cp "$binary" "$tmp_package/bin/bindport"
chmod 755 "$tmp_package/bin/bindport"
mkdir -p "$dist_dir"

(
  cd "$tmp_package"
  npm pack --pack-destination "$dist_dir"
)
