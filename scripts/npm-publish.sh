#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: scripts/npm-publish.sh --version <x.y.z> [--dist <dir>] [--execute] [--yes]

Publishes BindPort npm tarballs in dependency order. By default this runs
`npm publish --dry-run`; pass --execute to publish to npm.

The dist directory must contain tarballs produced by release.yml:
  bindport-linux-x64-X.Y.Z.tgz
  bindport-linux-arm64-X.Y.Z.tgz
  bindport-darwin-x64-X.Y.Z.tgz
  bindport-darwin-arm64-X.Y.Z.tgz
  bindport-X.Y.Z.tgz
USAGE
}

die() {
  echo "npm-publish: $*" >&2
  exit 1
}

version="${RELEASE_VERSION:-}"
dist_dir="dist"
execute=false
yes=false

while [[ "$#" -gt 0 ]]; do
  case "$1" in
    --version)
      [[ "$#" -ge 2 ]] || die "--version requires a value"
      version="${2:-}"
      shift 2
      ;;
    --dist)
      [[ "$#" -ge 2 ]] || die "--dist requires a value"
      dist_dir="${2:-}"
      shift 2
      ;;
    --execute)
      execute=true
      shift
      ;;
    --yes)
      yes=true
      shift
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    v[0-9]*.[0-9]*.[0-9]* | [0-9]*.[0-9]*.[0-9]*)
      [[ -z "$version" ]] || die "release version was provided more than once"
      version="$1"
      shift
      ;;
    *)
      usage >&2
      die "unknown argument: $1"
      ;;
  esac
done

version="${version#v}"
[[ "$version" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]] ||
  die "version must be stable x.y.z or vx.y.z"

for command in npm node; do
  command -v "$command" >/dev/null 2>&1 || die "$command is required"
done
export npm_config_cache="${npm_config_cache:-${TMPDIR:-/tmp}/bindport-npm-cache}"

verify_checksum() {
  local checksum_file="$1"
  local checksum_name
  checksum_name="$(basename "$checksum_file")"
  (
    cd "$(dirname "$checksum_file")"
    if command -v sha256sum >/dev/null 2>&1; then
      sha256sum -c "$checksum_name"
    else
      shasum -a 256 -c "$checksum_name"
    fi
  )
}

root="$(git rev-parse --show-toplevel)"
cd "$root"

node scripts/npm-package-utils.js validate "$version"

[[ -d "$dist_dir" ]] || die "dist directory not found: $dist_dir"
dist_dir="$(cd "$dist_dir" && pwd)"
packages=(
  "bindport-darwin-arm64-$version.tgz"
  "bindport-darwin-x64-$version.tgz"
  "bindport-linux-arm64-$version.tgz"
  "bindport-linux-x64-$version.tgz"
  "bindport-$version.tgz"
)

for package in "${packages[@]}"; do
  [[ -f "$dist_dir/$package" ]] || die "missing npm tarball: $dist_dir/$package"
  [[ -f "$dist_dir/$package.sha256" ]] ||
    die "missing npm tarball checksum: $dist_dir/$package.sha256"
  verify_checksum "$dist_dir/$package.sha256"
done

mode="dry-run"
publish_args=(publish --access public --dry-run)
if [[ "$execute" == "true" ]]; then
  mode="publish"
  publish_args=(publish --access public)
  if [[ "${NPM_CONFIG_PROVENANCE:-${npm_config_provenance:-false}}" == "true" ]]; then
    publish_args+=(--provenance)
  fi
fi

if [[ "$yes" == "false" ]]; then
  cat <<EOF
npm version: $version
dist: $dist_dir
mode: $mode
packages:
  ${packages[*]}

Ready to $mode BindPort npm packages? [y/N]
EOF
  read -r answer
  case "$answer" in
    y | Y | yes | YES)
      ;;
    *)
      echo "Aborted."
      exit 0
      ;;
  esac
fi

for package in "${packages[@]}"; do
  npm "${publish_args[@]}" "$dist_dir/$package"
done
