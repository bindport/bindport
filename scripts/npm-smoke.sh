#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: scripts/npm-smoke.sh

Builds local npm tarballs with a fake platform binary, installs them into a
temporary package, and verifies npm/npx execution paths. If bun is installed,
the same local install is checked through bunx.
USAGE
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64)
    platform="linux-x64"
    package_dir="bindport-linux-x64"
    ;;
  Linux-aarch64 | Linux-arm64)
    platform="linux-arm64"
    package_dir="bindport-linux-arm64"
    ;;
  Darwin-x86_64)
    platform="darwin-x64"
    package_dir="bindport-darwin-x64"
    ;;
  Darwin-arm64)
    platform="darwin-arm64"
    package_dir="bindport-darwin-arm64"
    ;;
  *)
    echo "unsupported smoke-test platform: $(uname -s)-$(uname -m)" >&2
    exit 1
    ;;
esac

root="$(git rev-parse --show-toplevel)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
version="$(node "$root/scripts/npm-package-utils.js" current-version)"
export npm_config_cache="${npm_config_cache:-$tmp/npm-cache}"

pack_dir="$tmp/packs"
mkdir -p "$pack_dir"

node "$root/scripts/npm-package-utils.js" validate "$version"

(
  cd "$root/npm/bindport"
  npm pack --pack-destination "$pack_dir" >/dev/null
)

platform_source="$root/npm/$package_dir"
platform_tmp="$tmp/$package_dir"
mkdir -p "$platform_tmp"
cp -R "$platform_source"/. "$platform_tmp"/
mkdir -p "$platform_tmp/bin"
cat > "$platform_tmp/bin/bindport" <<'BIN'
#!/usr/bin/env sh
printf 'bindport npm smoke %s\n' "$*"
BIN
chmod 755 "$platform_tmp/bin/bindport"
(
  cd "$platform_tmp"
  npm pack --pack-destination "$pack_dir" >/dev/null
)

forwarding_expected="bindport npm smoke -- /bin/sh -c printf \"PORT=%s\\n\" \"\$PORT\""

assert_output() {
  expected="$1"
  shift
  "$@" | grep -F "$expected" >/dev/null
}

project="$tmp/project"
mkdir -p "$project"
(
  cd "$project"
  npm init -y >/dev/null
  npm install --silent --offline --ignore-scripts --no-audit --no-fund --omit=optional \
    "$pack_dir/bindport-$version.tgz" \
    "$pack_dir/bindport-$platform-$version.tgz"
  assert_output "bindport npm smoke --version" npx --no-install bindport --version
  assert_output "$forwarding_expected" npx --no-install bindport -- /bin/sh -c "printf \"PORT=%s\\n\" \"\$PORT\""
  assert_output "bindport npm smoke --help" npm exec -- bindport --help
  assert_output "$forwarding_expected" npm exec -- bindport -- /bin/sh -c "printf \"PORT=%s\\n\" \"\$PORT\""
  if command -v bun >/dev/null 2>&1; then
    assert_output "bindport npm smoke doctor" bunx --no-install bindport doctor
    assert_output "$forwarding_expected" bunx --no-install bindport -- /bin/sh -c "printf \"PORT=%s\\n\" \"\$PORT\""
  fi
)

nested_project="$tmp/nested-project"
mkdir -p "$nested_project"
(
  cd "$nested_project"
  npm init -y >/dev/null
  npm install --silent --offline --ignore-scripts --no-audit --no-fund --omit=optional \
    "$pack_dir/bindport-$version.tgz" \
    "$pack_dir/bindport-$platform-$version.tgz"
  mkdir -p node_modules/bindport/node_modules/@bindport
  mv "node_modules/@bindport/$platform" "node_modules/bindport/node_modules/@bindport/$platform"
  rmdir node_modules/@bindport 2>/dev/null || true
  assert_output "bindport npm smoke --version" npx --no-install bindport --version
  assert_output "$forwarding_expected" npx --no-install bindport -- /bin/sh -c "printf \"PORT=%s\\n\" \"\$PORT\""
  assert_output "$forwarding_expected" npm exec -- bindport -- /bin/sh -c "printf \"PORT=%s\\n\" \"\$PORT\""
)

if command -v pnpm >/dev/null 2>&1; then
  pnpm_project="$tmp/pnpm-project"
  mkdir -p "$pnpm_project"
  (
    cd "$pnpm_project"
    cat > package.json <<EOF
{
  "private": true,
  "devDependencies": {
    "bindport": "file:$pack_dir/bindport-$version.tgz",
    "@bindport/$platform": "file:$pack_dir/bindport-$platform-$version.tgz"
  }
}
EOF
    pnpm install --offline --ignore-scripts --config.store-dir="$tmp/pnpm-store" >/dev/null
    assert_output "bindport npm smoke --version" pnpm exec -- bindport --version
    assert_output "$forwarding_expected" pnpm exec -- bindport -- /bin/sh -c "printf \"PORT=%s\\n\" \"\$PORT\""
  )
fi

echo "npm wrapper smoke passed for $platform."
