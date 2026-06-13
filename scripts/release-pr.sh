#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: mise run release-pr -- [minor|patch|major|vX.Y.Z|X.Y.Z]

Creates a release prep branch, updates Cargo and npm package versions, runs the
release-prep gate, commits the version bump, pushes the branch, and opens a PR.

An explicit release argument is required. Use `minor` for the first v0.1.0
release while the repository is still at 0.0.0.
USAGE
}

die() {
  echo "release-pr: $*" >&2
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
    die "0.0.x is bootstrap-only; use minor or explicit 0.1.0 for the first release"
  fi
}

greater_than() {
  local old="$1"
  local new="$2"
  awk -v old="$old" -v new="$new" '
    BEGIN {
      split(old, o, ".")
      split(new, n, ".")
      for (i = 1; i <= 3; i++) {
        if ((n[i] + 0) > (o[i] + 0)) exit 0
        if ((n[i] + 0) < (o[i] + 0)) exit 1
      }
      exit 1
    }
  '
}

bump_version() {
  local version="$1"
  local level="$2"
  local major minor patch
  IFS=. read -r major minor patch <<<"$version"

  case "$level" in
    patch)
      patch=$((patch + 1))
      ;;
    minor)
      minor=$((minor + 1))
      patch=0
      ;;
    major)
      major=$((major + 1))
      minor=0
      patch=0
      ;;
    *)
      die "unsupported bump level: $level"
      ;;
  esac

  printf '%s.%s.%s\n' "$major" "$minor" "$patch"
}

update_npm_version() {
  local version="$1"
  # shellcheck disable=SC2016
  node -e '
    const fs = require("fs");
    const path = "npm/bindport/package.json";
    const version = process.argv[1];
    const packageJson = JSON.parse(fs.readFileSync(path, "utf8"));
    packageJson.version = version;
    fs.writeFileSync(path, `${JSON.stringify(packageJson, null, 2)}\n`);
  ' "$version"
}

confirm() {
  local new_version="$1"
  local request_label="$2"

  cat <<EOF
Current version: $CURRENT_VERSION
Requested release: $request_label
New version: $new_version
Branch: release/v$new_version

Ready to create release prep PR for v$new_version? [y/N]
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

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

if [[ "$#" -ne 1 ]]; then
  usage >&2
  exit 2
fi

request="$1"
case "$request" in
  patch | minor | major)
    ;;
  v[0-9]*.[0-9]*.[0-9]*)
    request="${request#v}"
    ;;
  [0-9]*.[0-9]*.[0-9]*)
    ;;
  *)
    usage >&2
    die "expected minor, patch, major, vX.Y.Z, or X.Y.Z"
    ;;
esac

for command in cargo git gh node npm mise; do
  command -v "$command" >/dev/null 2>&1 || die "$command is required"
done
gh auth status >/dev/null 2>&1 || die "gh is not authenticated; run 'gh auth login'"
cargo set-version --version >/dev/null 2>&1 || die "cargo-edit is required; run mise install --locked"

root="$(git rev-parse --show-toplevel)"
cd "$root"

[[ -z "$(git status --porcelain)" ]] || die "worktree must be clean"

branch="$(git branch --show-current)"
[[ "$branch" == "main" ]] || die "release PR must start from main, currently on $branch"

git fetch origin main >/dev/null
local_head="$(git rev-parse main)"
remote_head="$(git rev-parse origin/main)"
[[ "$local_head" == "$remote_head" ]] || die "main must match origin/main before release prep"

CURRENT_VERSION="$(current_cargo_version)"
npm_version="$(current_npm_version)"
is_stable_semver "$CURRENT_VERSION" || die "current Cargo version must be stable X.Y.Z, got $CURRENT_VERSION"
[[ "$npm_version" == "$CURRENT_VERSION" ]] || die "npm version $npm_version does not match Cargo version $CURRENT_VERSION"

if [[ "$request" == "patch" || "$request" == "minor" || "$request" == "major" ]]; then
  new_version="$(bump_version "$CURRENT_VERSION" "$request")"
  request_label="$request"
else
  new_version="$request"
  is_stable_semver "$new_version" || die "target version must be stable X.Y.Z, got $new_version"
  greater_than "$CURRENT_VERSION" "$new_version" || die "target version $new_version must be greater than $CURRENT_VERSION"
  request_label="explicit"
fi
reject_bootstrap_version "$new_version"

release_branch="release/v$new_version"
if git show-ref --verify --quiet "refs/heads/$release_branch"; then
  die "local branch already exists: $release_branch"
fi
if git ls-remote --exit-code --heads origin "$release_branch" >/dev/null 2>&1; then
  die "remote branch already exists: $release_branch"
fi

confirm "$new_version" "$request_label"

git switch -c "$release_branch"
cargo set-version --workspace "$new_version"
update_npm_version "$new_version"

cargo metadata --format-version 1 --no-deps >/dev/null
cargo metadata --locked --format-version 1 --no-deps >/dev/null
MISE_TRUSTED_CONFIG_PATHS="$root" scripts/release-prep.sh --version "$new_version"

git add Cargo.toml Cargo.lock npm/bindport/package.json
git diff --staged --quiet && die "version update produced no staged changes"
git commit -m "build: prepare v$new_version release"
git push -u origin "$release_branch"

gh pr create \
  --base main \
  --head "$release_branch" \
  --title "build: prepare v$new_version release" \
  --body "## Summary
- bump Cargo workspace and npm package versions to $new_version for release prep

## Verification
- cargo metadata --locked --format-version 1 --no-deps
- MISE_TRUSTED_CONFIG_PATHS=\$PWD scripts/release-prep.sh --version $new_version"
