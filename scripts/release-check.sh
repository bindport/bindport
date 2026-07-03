#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: mise run release-check <x.y.z>
       scripts/release-check.sh --version <x.y.z>

Validates a manual BindPort release checkout without creating tags,
publishing packages, or mutating source files.

Options:
  --version <x.y.z>          Release version to verify. Also accepted through
                             RELEASE_VERSION.
  --npm-package-name <name>  Expected npm package name. Defaults to bindport.
                             Also accepted through NPM_PACKAGE_NAME.
  --skip-ci                 Skip mise run ci. Also set RUN_CI=false.
  --allow-dirty             Allow Cargo package dry-runs with uncommitted
                             release-prep version bump changes.
  --publish-ready           Run full crates.io publish dry-runs. Also set
                             PUBLISH_READY=true.
  -h, --help                Show this help.
USAGE
}

release_version="${RELEASE_VERSION:-}"
npm_package_name="${NPM_PACKAGE_NAME:-bindport}"
run_ci="${RUN_CI:-true}"
publish_ready="${PUBLISH_READY:-false}"
allow_dirty="${ALLOW_DIRTY:-false}"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --version)
      release_version="${2:-}"
      shift 2
      ;;
    --npm-package-name)
      npm_package_name="${2:-}"
      shift 2
      ;;
    --skip-ci)
      run_ci=false
      shift
      ;;
    --allow-dirty)
      allow_dirty=true
      shift
      ;;
    --publish-ready)
      publish_ready=true
      shift
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    v[0-9]*.[0-9]*.[0-9]* | [0-9]*.[0-9]*.[0-9]*)
      if [[ -n "$release_version" ]]; then
        echo "release version was provided more than once" >&2
        usage >&2
        exit 2
      fi
      release_version="$1"
      shift
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if [[ -z "$release_version" ]]; then
  echo "release version is required" >&2
  usage >&2
  exit 2
fi

version="${release_version#v}"
if [[ ! "$version" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]]; then
  echo "version must be stable SemVer x.y.z or vx.y.z, got $release_version" >&2
  exit 1
fi

if [[ "$version" =~ ^0\.0\. ]]; then
  echo "0.0.x is a bootstrap version range and must not be release-prepped" >&2
  exit 1
fi

for command in cargo git git-cliff npm node; do
  if ! command -v "$command" >/dev/null 2>&1; then
    echo "required command not found: $command" >&2
    exit 1
  fi
done

repo_root="$(git rev-parse --show-toplevel)"
cd "$repo_root"

workspace_version="$(awk -F'"' '/^version = / { print $2; exit }' Cargo.toml)"
npm_version="$(node -e "const fs=require('fs'); console.log(JSON.parse(fs.readFileSync('npm/bindport/package.json', 'utf8')).version)")"
npm_name="$(node -e "const fs=require('fs'); console.log(JSON.parse(fs.readFileSync('npm/bindport/package.json', 'utf8')).name)")"
npm_private="$(node -e "const fs=require('fs'); console.log(JSON.parse(fs.readFileSync('npm/bindport/package.json', 'utf8')).private === true ? 'true' : 'false')")"

if [[ "$workspace_version" != "$version" ]]; then
  echo "Cargo workspace version $workspace_version does not match $version" >&2
  exit 1
fi

if [[ "$npm_version" != "$version" ]]; then
  echo "npm package version $npm_version does not match $version" >&2
  exit 1
fi

if [[ "$npm_name" != "$npm_package_name" ]]; then
  echo "npm package name $npm_name does not match $npm_package_name" >&2
  exit 1
fi

if [[ "$npm_private" == "true" ]]; then
  echo "npm package is private; npm publishing is disabled." >&2
  exit 1
fi
node scripts/npm-package-utils.js validate "$version"
node scripts/check-binstall-metadata.js
scripts/check-cli-assets.sh
changelog_tmp="$(mktemp)"
trap 'rm -f "$changelog_tmp"' EXIT
changelog_args=(--output "$changelog_tmp")
if ! git rev-parse --verify --quiet "refs/tags/v$version" >/dev/null; then
  changelog_args=(--tag "v$version" "${changelog_args[@]}")
fi
RUST_LOG=error git-cliff "${changelog_args[@]}"
if ! cmp -s "$changelog_tmp" CHANGELOG.md; then
  echo "CHANGELOG.md is not up to date; run mise run changelog $version" >&2
  exit 1
fi
echo "CHANGELOG.md is up to date"

git diff --check

if [[ "$run_ci" == "true" ]]; then
  if ! command -v mise >/dev/null 2>&1; then
    echo "mise is required when RUN_CI=true" >&2
    exit 1
  fi

  mise run ci
fi

cargo_dirty_flags=()
if [[ "$allow_dirty" == "true" ]]; then
  cargo_dirty_flags+=(--allow-dirty)
fi

cargo package -p bindport "${cargo_dirty_flags[@]}" --list
if [[ "$publish_ready" == "true" ]]; then
  cargo_publish_args=(--version "$version" --dry-run)
  if [[ "$allow_dirty" == "true" ]]; then
    cargo_publish_args+=(--allow-dirty)
  fi
  scripts/cargo-publish.sh "${cargo_publish_args[@]}"
else
  echo "Skipping Cargo publish dry-run; use --publish-ready before crates.io publishing."
fi
node scripts/npm-package-utils.js pack-check --dry-run

echo "Release check completed for v$version."
