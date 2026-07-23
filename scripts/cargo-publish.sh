#!/usr/bin/env bash
set -euo pipefail

packages=(
  bindport-core
  bindport-adapters
  bindport-runner
  bindport-registry
  bindport-dashboard
  bindport
)

usage() {
  cat <<'USAGE'
Usage: mise run cargo-publish [--execute] [--allow-dirty] [--wait-seconds N] [vX.Y.Z|X.Y.Z]
       scripts/cargo-publish.sh [--execute] [--version X.Y.Z]

Publishes BindPort crates to crates.io in dependency order.

Defaults to a cargo publish dry-run for every package whose same-version
workspace dependencies are already present on crates.io. Packages behind an
unpublished workspace dependency are reported explicitly and their source shape
is validated by release-check. Real publishing requires --execute and an
interactive confirmation unless --yes is also provided. Already-published crate
versions are skipped so interrupted publishes can be resumed.

Options:
  --version <x.y.z>      Release version to verify. Also accepted through
                         RELEASE_VERSION.
  --dry-run              Dry-run publish commands that can resolve against the
                         current crates.io index. This is default.
  --execute, --publish   Actually publish to crates.io.
  --allow-dirty          Pass --allow-dirty to dry-run cargo publish commands.
                         Rejected for real publishing.
  --wait-seconds <n>     Maximum seconds to poll for index propagation between
                         real publishes. Defaults to 60.
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

is_already_published_error() {
  local output_file="$1"
  grep -Eiq "(crate version .+ is already (uploaded|published)|previously[[:space:]]+uploaded)" "$output_file"
}

workspace_dependencies() {
  local package="$1"
  printf '%s' "$workspace_metadata" | node -e '
    const fs = require("fs");
    const metadata = JSON.parse(fs.readFileSync(0, "utf8"));
    const packageName = process.argv[1];
    const workspaceNames = new Set(metadata.packages.map(item => item.name));
    const item = metadata.packages.find(candidate => candidate.name === packageName);
    if (!item) process.exit(2);
    for (const dependency of item.dependencies) {
      if (dependency.path && workspaceNames.has(dependency.name)) {
        console.log(dependency.name);
      }
    }
  ' "$package"
}

crate_version_is_published() {
  local package="$1"
  local status
  status="$(curl -sS -o /dev/null -w '%{http_code}' \
    -A "bindport-release/$version (https://github.com/bindport/bindport)" \
    "https://crates.io/api/v1/crates/$package/$version")" ||
    die "failed to query crates.io for $package v$version"
  case "$status" in
    200)
      return 0
      ;;
    404)
      return 1
      ;;
    *)
      die "crates.io returned HTTP $status for $package v$version"
      ;;
  esac
}

wait_for_crate_index() {
  local package="$1"
  local deadline=$((SECONDS + wait_seconds))
  local remaining

  while ! cargo info --registry crates-io "$package@$version" >/dev/null 2>&1; do
    remaining=$((deadline - SECONDS))
    if ((remaining <= 0)); then
      die "$package v$version was not visible in the crates.io index within ${wait_seconds}s"
    fi
    if ((remaining < 5)); then
      sleep "$remaining"
    else
      sleep 5
    fi
  done
}

run_cargo_publish() {
  local package="$1"
  shift
  local output_file status

  output_file="$(mktemp)"
  set +e
  cargo "$@" 2>&1 | tee "$output_file"
  status="${PIPESTATUS[0]}"
  set -e

  if [[ "$status" -eq 0 ]]; then
    rm -f "$output_file"
    publish_result="published"
    return 0
  fi

  if [[ "$mode" == "publish" ]] && is_already_published_error "$output_file"; then
    echo "Skipping $package v$version because it is already published."
    rm -f "$output_file"
    publish_result="skipped-existing"
    return 0
  fi

  rm -f "$output_file"
  return "$status"
}

release_version="${RELEASE_VERSION:-}"
mode="dry-run"
allow_dirty=false
yes=false
wait_seconds="${CARGO_PUBLISH_WAIT_SECONDS:-60}"

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

for command in cargo curl git node; do
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

workspace_metadata="$(cargo metadata --locked --format-version 1 --no-deps)"
skipped_existing=()
skipped_dry_runs=()

for index in "${!packages[@]}"; do
  package="${packages[$index]}"
  if [[ "$mode" == "dry-run" ]]; then
    unpublished_dependencies=()
    while IFS= read -r dependency; do
      if [[ -n "$dependency" ]] && ! crate_version_is_published "$dependency"; then
        unpublished_dependencies+=("$dependency")
      fi
    done < <(workspace_dependencies "$package")
    if [[ "${#unpublished_dependencies[@]}" -gt 0 ]]; then
      echo "Skipping $package publish dry-run; unpublished v$version workspace dependencies: ${unpublished_dependencies[*]}."
      skipped_dry_runs+=("$package")
      continue
    fi
  fi

  args=(publish -p "$package" --locked)
  if [[ "$mode" == "dry-run" ]]; then
    args+=(--dry-run)
    if [[ "$allow_dirty" == "true" ]]; then
      args+=(--allow-dirty)
    fi
  fi

  echo "cargo ${args[*]}"
  publish_result=""
  run_cargo_publish "$package" "${args[@]}"

  if [[ "$publish_result" == "skipped-existing" ]]; then
    skipped_existing+=("$package")
  fi

  if [[ "$mode" == "publish" && "$publish_result" == "published" && "$index" -lt "$((${#packages[@]} - 1))" && "$wait_seconds" -gt 0 ]]; then
    echo "Polling up to ${wait_seconds}s for $package v$version crates.io index propagation..."
    wait_for_crate_index "$package"
  fi
done

if [[ "$mode" == "dry-run" ]]; then
  echo "Cargo publish dry-run completed for v$version."
  if [[ "${#skipped_dry_runs[@]}" -gt 0 ]]; then
    echo "Skipped dry-runs that require unpublished workspace dependencies: ${skipped_dry_runs[*]}."
  fi
else
  echo "Cargo publish completed for v$version."
  if [[ "${#skipped_existing[@]}" -gt 0 ]]; then
    echo "Skipped already-published packages: ${skipped_existing[*]}."
  fi
fi
