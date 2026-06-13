#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: mise run release-publish -- [--dry-run] [vX.Y.Z|X.Y.Z]

Dispatches the stable release workflow for the current Cargo/npm version.
If no version is provided, the script uses the version in Cargo.toml.
USAGE
}

die() {
  echo "release-publish: $*" >&2
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

reject_bootstrap_version() {
  if [[ "$1" =~ ^0\.0\. ]]; then
    die "0.0.x is bootstrap-only and must not be released"
  fi
}

confirm() {
  local mode="publish"
  if [[ "$dry_run" == "true" ]]; then
    mode="dry-run"
  fi

  cat <<EOF
Cargo version: $cargo_version
npm version: $npm_version
Release tag: v$release_version
Commit: $local_head
Workflow mode: $mode

Ready to dispatch the stable release workflow for v$release_version? [y/N]
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

dry_run=false
request=""

while [[ "$#" -gt 0 ]]; do
  case "$1" in
    -h | --help)
      usage
      exit 0
      ;;
    --dry-run)
      dry_run=true
      ;;
    v[0-9]*.[0-9]*.[0-9]*)
      [[ -z "$request" ]] || die "only one version may be provided"
      request="${1#v}"
      ;;
    [0-9]*.[0-9]*.[0-9]*)
      [[ -z "$request" ]] || die "only one version may be provided"
      request="$1"
      ;;
    *)
      usage >&2
      die "expected --dry-run, vX.Y.Z, or X.Y.Z"
      ;;
  esac
  shift
done

for command in git gh node; do
  command -v "$command" >/dev/null 2>&1 || die "$command is required"
done
gh auth status >/dev/null 2>&1 || die "gh is not authenticated; run 'gh auth login'"
repo="$(gh repo view --json nameWithOwner --jq .nameWithOwner)"
[[ -n "$repo" ]] || die "could not resolve GitHub repository"

root="$(git rev-parse --show-toplevel)"
cd "$root"

[[ -z "$(git status --porcelain)" ]] || die "worktree must be clean"

branch="$(git branch --show-current)"
[[ "$branch" == "main" ]] || die "stable release workflow must be dispatched from main, currently on $branch"

git fetch origin main --tags >/dev/null
local_head="$(git rev-parse main)"
remote_head="$(git rev-parse origin/main)"
[[ "$local_head" == "$remote_head" ]] || die "main must match origin/main before release publishing"

upstream="$(git rev-parse --abbrev-ref --symbolic-full-name '@{upstream}' 2>/dev/null || true)"
[[ "$upstream" == "origin/main" ]] || die "main must track origin/main before release publishing"

cargo_version="$(current_cargo_version)"
npm_version="$(current_npm_version)"
is_stable_semver "$cargo_version" || die "current Cargo version must be stable X.Y.Z, got $cargo_version"
[[ "$npm_version" == "$cargo_version" ]] || die "npm version $npm_version does not match Cargo version $cargo_version"
reject_bootstrap_version "$cargo_version"

if [[ -n "$request" ]]; then
  is_stable_semver "$request" || die "requested version must be stable X.Y.Z, got $request"
  [[ "$request" == "$cargo_version" ]] || die "requested version $request does not match Cargo.toml version $cargo_version"
fi

release_version="$cargo_version"
release_tag="v$release_version"

if git show-ref --verify --quiet "refs/tags/$release_tag"; then
  local_tag_commit="$(git rev-list -n 1 "$release_tag")"
  [[ "$local_tag_commit" == "$local_head" ]] || die "local tag $release_tag already exists at $local_tag_commit, not $local_head"
fi
if git ls-remote --exit-code --tags origin "refs/tags/$release_tag" >/dev/null 2>&1; then
  remote_tag_commit="$(git ls-remote --tags origin "refs/tags/$release_tag^{}" | awk '{ print $1 }')"
  if [[ -z "$remote_tag_commit" ]]; then
    remote_tag_commit="$(git ls-remote --tags origin "refs/tags/$release_tag" | awk '{ print $1 }')"
  fi
  [[ "$remote_tag_commit" == "$local_head" ]] || die "remote tag $release_tag already exists at $remote_tag_commit, not $local_head"
fi

gh workflow view release.yml --repo "$repo" >/dev/null 2>&1 \
  || die "release.yml workflow is not available on GitHub yet"

confirm

gh workflow run release.yml \
  --repo "$repo" \
  --ref main \
  -f "version=$release_version" \
  -f "dry_run=$dry_run"

echo "Dispatched release workflow for $release_tag."
echo "View runs with: gh run list --workflow release.yml --limit 1"
