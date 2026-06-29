#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: mise run release-finish [--yes] [--skip-github-release] [--skip-cargo-publish] [vX.Y.Z|X.Y.Z]
       scripts/release-finish.sh [--yes] [--version X.Y.Z]

Finishes a reviewed release from main: creates the GitHub Release, waits for the
release workflow to pass, verifies the tag/release, and publishes Cargo crates.

Options:
  --version <x.y.z>              Release version to finish. Also accepted
                                 through RELEASE_VERSION.
  --yes                          Skip the interactive confirmation.
  --skip-github-release          Do not dispatch or wait for release.yml.
  --skip-cargo-publish           Do not publish crates.io packages.
  --cargo-wait-seconds <n>       Seconds to wait between Cargo publishes.
                                 Defaults to 60.
  --workflow-timeout-seconds <n> Seconds to wait for release.yml. Defaults to
                                 3600.
  --poll-seconds <n>             Seconds between GitHub workflow polls.
                                 Defaults to 15.
  -h, --help                     Show this help.
USAGE
}

die() {
  echo "release-finish: $*" >&2
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

remote_tag_commit() {
  local tag="$1"
  local commit

  commit="$(git ls-remote --tags origin "refs/tags/$tag^{}" | awk '{ print $1 }')"
  if [[ -z "$commit" ]]; then
    commit="$(git ls-remote --tags origin "refs/tags/$tag" | awk '{ print $1 }')"
  fi

  printf '%s\n' "$commit"
}

confirm_finish() {
  if [[ "$yes" == "true" ]]; then
    return 0
  fi

  cat <<EOF
Cargo version: $workspace_version
npm version: $npm_version
Release tag: $release_tag
Commit: $local_head
GitHub Release: $github_release_plan
Cargo publish: $cargo_publish_plan

Ready to finish v$version? [y/N]
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

release_exists_at_head() {
  local tag_commit

  gh release view "$release_tag" --repo "$repo" >/dev/null 2>&1 || return 1
  tag_commit="$(git rev-list -n 1 "$release_tag" 2>/dev/null || true)"
  [[ "$tag_commit" == "$local_head" ]]
}

release_run_ids() {
  gh run list \
    --repo "$repo" \
    --workflow release.yml \
    --branch main \
    --commit "$local_head" \
    --event workflow_dispatch \
    --limit 20 \
    --json databaseId \
    --jq '.[].databaseId'
}

find_release_run_id() {
  local existing_run_ids="$1"
  local elapsed=0
  local run_id run_ids

  while true; do
    run_ids="$(release_run_ids)"
    while IFS= read -r run_id; do
      [[ -n "$run_id" ]] || continue
      if ! printf '%s\n' "$existing_run_ids" | grep -Fxq "$run_id"; then
        printf '%s\n' "$run_id"
        return 0
      fi
    done <<< "$run_ids"

    if ((elapsed >= 60)); then
      die "release workflow run did not appear for $local_head"
    fi

    echo "Waiting for release workflow run to appear..." >&2
    sleep 5
    elapsed=$((elapsed + 5))
  done
}

wait_for_release_run() {
  local run_id="$1"
  local elapsed=0
  local status conclusion url

  url="$(gh run view "$run_id" --repo "$repo" --json url --jq .url)"
  echo "Watching release workflow: $url"

  while true; do
    status="$(gh run view "$run_id" --repo "$repo" --json status --jq .status)"
    conclusion="$(gh run view "$run_id" --repo "$repo" --json conclusion --jq '.conclusion // ""')"

    if [[ "$status" == "completed" ]]; then
      if [[ "$conclusion" == "success" ]]; then
        echo "Release workflow completed successfully."
        return 0
      fi

      die "release workflow completed with conclusion '$conclusion': $url"
    fi

    if ((elapsed >= workflow_timeout_seconds)); then
      die "release workflow timed out after ${workflow_timeout_seconds}s: $url"
    fi

    echo "Release workflow status: $status. Waiting ${poll_seconds}s..."
    sleep "$poll_seconds"
    elapsed=$((elapsed + poll_seconds))
  done
}

verify_release_exists() {
  local tag_commit

  gh release view "$release_tag" --repo "$repo" >/dev/null ||
    die "GitHub Release $release_tag was not found"

  git fetch origin main --tags >/dev/null
  tag_commit="$(git rev-list -n 1 "$release_tag" 2>/dev/null || true)"
  [[ "$tag_commit" == "$local_head" ]] ||
    die "release tag $release_tag points at ${tag_commit:-missing}, not $local_head"
}

release_version="${RELEASE_VERSION:-}"
yes=false
skip_github_release=false
skip_cargo_publish=false
cargo_wait_seconds="${CARGO_PUBLISH_WAIT_SECONDS:-60}"
workflow_timeout_seconds="${RELEASE_WORKFLOW_TIMEOUT_SECONDS:-3600}"
poll_seconds="${RELEASE_WORKFLOW_POLL_SECONDS:-15}"

while [[ "$#" -gt 0 ]]; do
  case "$1" in
    --version)
      release_version="${2:-}"
      shift 2
      ;;
    --yes)
      yes=true
      shift
      ;;
    --skip-github-release)
      skip_github_release=true
      shift
      ;;
    --skip-cargo-publish)
      skip_cargo_publish=true
      shift
      ;;
    --cargo-wait-seconds)
      cargo_wait_seconds="${2:-}"
      shift 2
      ;;
    --workflow-timeout-seconds)
      workflow_timeout_seconds="${2:-}"
      shift 2
      ;;
    --poll-seconds)
      poll_seconds="${2:-}"
      shift 2
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

for command in cargo git gh node; do
  command -v "$command" >/dev/null 2>&1 || die "$command is required"
done

for numeric_value in "$cargo_wait_seconds" "$workflow_timeout_seconds" "$poll_seconds"; do
  [[ "$numeric_value" =~ ^[0-9]+$ ]] || die "timeout and wait values must be non-negative integers"
done
[[ "$poll_seconds" -gt 0 ]] || die "--poll-seconds must be greater than zero"

gh auth status >/dev/null 2>&1 || die "gh is not authenticated; run 'gh auth login'"
repo="$(gh repo view --json nameWithOwner --jq .nameWithOwner)"
[[ -n "$repo" ]] || die "could not resolve GitHub repository"

root="$(git rev-parse --show-toplevel)"
cd "$root"

[[ -z "$(git status --porcelain)" ]] || die "worktree must be clean"

branch="$(git branch --show-current)"
[[ "$branch" == "main" ]] || die "release finish must run from main, currently on $branch"

git fetch origin main --tags >/dev/null
local_head="$(git rev-parse main)"
remote_head="$(git rev-parse origin/main)"
[[ "$local_head" == "$remote_head" ]] || die "main must match origin/main"

upstream="$(git rev-parse --abbrev-ref --symbolic-full-name '@{upstream}' 2>/dev/null || true)"
[[ "$upstream" == "origin/main" ]] || die "main must track origin/main"

workspace_version="$(current_cargo_version)"
npm_version="$(current_npm_version)"
version="${release_version#v}"
if [[ -z "$release_version" ]]; then
  version="$workspace_version"
fi

is_stable_semver "$version" || die "version must be stable X.Y.Z or vX.Y.Z, got ${release_version:-$workspace_version}"
[[ ! "$version" =~ ^0\.0\. ]] || die "0.0.x is bootstrap-only and must not be released"
[[ "$workspace_version" == "$version" ]] || die "Cargo workspace version $workspace_version does not match $version"
[[ "$npm_version" == "$version" ]] || die "npm package version $npm_version does not match $version"

release_tag="v$version"
existing_remote_tag_commit="$(remote_tag_commit "$release_tag")"
if [[ -n "$existing_remote_tag_commit" && "$existing_remote_tag_commit" != "$local_head" ]]; then
  die "remote tag $release_tag exists at $existing_remote_tag_commit, not $local_head"
fi

if [[ "$skip_github_release" == "true" ]]; then
  github_release_plan="skip by request"
elif release_exists_at_head; then
  github_release_plan="already exists at $local_head"
else
  gh workflow view release.yml --repo "$repo" >/dev/null 2>&1 ||
    die "release.yml workflow is not available on GitHub yet"
  github_release_plan="dispatch release.yml and wait for success"
fi

if [[ "$skip_cargo_publish" == "true" ]]; then
  cargo_publish_plan="skip by request"
else
  cargo_publish_plan="publish crates.io packages, resuming already-published versions"
fi

confirm_finish

if [[ "$skip_github_release" == "false" ]]; then
  if release_exists_at_head; then
    echo "GitHub Release $release_tag already exists at $local_head."
  else
    existing_release_run_ids="$(release_run_ids)"
    gh workflow run release.yml \
      --repo "$repo" \
      --ref main \
      -f "version=$version" \
      -f "dry_run=false"

    run_id="$(find_release_run_id "$existing_release_run_ids")"
    wait_for_release_run "$run_id"
    verify_release_exists
  fi
fi

if [[ "$skip_cargo_publish" == "false" ]]; then
  scripts/cargo-publish.sh \
    --version "$version" \
    --execute \
    --yes \
    --wait-seconds "$cargo_wait_seconds"
fi

echo "Finished release $release_tag."
