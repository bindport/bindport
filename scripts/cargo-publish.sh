#!/usr/bin/env bash
set -euo pipefail

packages=(
  bindport-core
  bindport-adapters
  bindport-runner
  bindport-registry
  bindport
)

usage() {
  cat <<'USAGE'
Usage: mise run cargo-publish [--execute] [--allow-dirty] [--wait-seconds N] [vX.Y.Z|X.Y.Z]
       scripts/cargo-publish.sh [--execute] [--version X.Y.Z]

Publishes BindPort crates to crates.io in dependency order.

Defaults to a cargo publish dry-run for every package. Real publishing requires
--execute and an interactive confirmation unless --yes is also provided.

Options:
  --version <x.y.z>      Release version to verify. Also accepted through
                         RELEASE_VERSION.
  --dry-run              Dry-run every cargo publish command. This is default.
  --execute, --publish   Actually publish to crates.io.
  --allow-dirty          Pass --allow-dirty to dry-run cargo publish commands.
                         Rejected for real publishing.
  --wait-seconds <n>     Seconds to wait between real publishes. Defaults to 20.
  --yes                  Skip the interactive confirmation. Intended for CI.
  -h, --help            Show this help.
USAGE
}

die() {
  echo "cargo-publish: $*" >&2
  exit 1
}

current_cargo_version() {
  awk -F'"' '/^version = / { print $2; exit }' Cargo.toml
}

current_npm_version() {
  node -e "const fs=require('fs'); console.log(JSON.parse(fs.readFileSync('npm/bindport/package.json', 'utf8')).version)"
}

is_stable_semver() {
  [[ "$1" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]]
}

require_release_state() {
  [[ -z "$(git status --porcelain)" ]] || die "worktree must be clean for real publishing"

  local branch
  branch="$(git branch --show-current)"
  if [[ -n "$branch" && "$branch" != "main" ]]; then
    die "real publishing must run from main or a detached main checkout, currently on $branch"
  fi

  git fetch origin main --tags >/dev/null

  local head remote_main tag tag_commit
  head="$(git rev-parse HEAD)"
  remote_main="$(git rev-parse origin/main)"
  [[ "$head" == "$remote_main" ]] || die "publish checkout must match origin/main"

  tag="v$version"
  tag_commit="$(git rev-list -n 1 "$tag" 2>/dev/null || true)"
  [[ -n "$tag_commit" ]] || die "release tag $tag must exist before crates.io publishing"
  [[ "$tag_commit" == "$head" ]] || die "release tag $tag points at $tag_commit, not $head"
}

confirm_publish() {
  cat <<EOF
Cargo version: $workspace_version
npm version: $npm_version
Packages: ${packages[*]}
Mode: publish to crates.io

Ready to publish v$version to crates.io? [y/N]
EOF

  local answer
  read -r answer
  case "$answer" in
    y | Y | yes | YES)
      ;;
    *)
      echo "Aborted."
      exit 0
      ;;
  esac
}

release_version="${RELEASE_VERSION:-}"
mode="dry-run"
allow_dirty=false
yes=false
wait_seconds="${CARGO_PUBLISH_WAIT_SECONDS:-20}"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --version)
      release_version="${2:-}"
      shift 2
      ;;
    --dry-run)
      mode="dry-run"
      shift
      ;;
    --execute | --publish)
      mode="publish"
      shift
      ;;
    --allow-dirty)
      allow_dirty=true
      shift
      ;;
    --wait-seconds)
      wait_seconds="${2:-}"
      shift 2
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
      if [[ -n "$release_version" ]]; then
        usage >&2
        die "release version was provided more than once"
      fi
      release_version="$1"
      shift
      ;;
    *)
      usage >&2
      die "unknown argument: $1"
      ;;
  esac
done

for command in cargo git node; do
  command -v "$command" >/dev/null 2>&1 || die "$command is required"
done

if [[ ! "$wait_seconds" =~ ^[0-9]+$ ]]; then
  die "--wait-seconds must be a non-negative integer"
fi

root="$(git rev-parse --show-toplevel)"
cd "$root"

workspace_version="$(current_cargo_version)"
npm_version="$(current_npm_version)"
version="${release_version#v}"
if [[ -z "$release_version" ]]; then
  version="$workspace_version"
fi

is_stable_semver "$version" || die "version must be stable X.Y.Z or vX.Y.Z, got ${release_version:-$workspace_version}"
[[ ! "$version" =~ ^0\.0\. ]] || die "0.0.x is bootstrap-only and must not be published"
[[ "$workspace_version" == "$version" ]] || die "Cargo workspace version $workspace_version does not match $version"
[[ "$npm_version" == "$version" ]] || die "npm package version $npm_version does not match $version"

if [[ "$mode" == "publish" ]]; then
  [[ "$allow_dirty" == "false" ]] || die "--allow-dirty is only permitted for dry-runs"
  require_release_state
  if [[ "$yes" != "true" ]]; then
    confirm_publish
  fi
elif [[ -n "$(git status --porcelain)" && "$allow_dirty" != "true" ]]; then
  die "worktree is dirty; pass --allow-dirty for dry-runs or clean it"
fi

for index in "${!packages[@]}"; do
  package="${packages[$index]}"
  args=(publish -p "$package" --locked)
  if [[ "$mode" == "dry-run" ]]; then
    args+=(--dry-run)
    if [[ "$allow_dirty" == "true" ]]; then
      args+=(--allow-dirty)
    fi
  fi

  echo "cargo ${args[*]}"
  cargo "${args[@]}"

  if [[ "$mode" == "publish" && "$index" -lt "$((${#packages[@]} - 1))" && "$wait_seconds" -gt 0 ]]; then
    echo "Waiting ${wait_seconds}s for crates.io index propagation..."
    sleep "$wait_seconds"
  fi
done

if [[ "$mode" == "dry-run" ]]; then
  echo "Cargo publish dry-run completed for v$version."
else
  echo "Cargo publish completed for v$version."
fi
