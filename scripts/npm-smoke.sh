#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: scripts/npm-smoke.sh

Builds local npm tarballs with a fake platform binary, installs them into a
temporary package, and verifies npm/npx execution paths. If bun is installed,
the same local install is checked through bunx. Also verifies that the wrapper
resolves every supported platform package path.
USAGE
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

root="$(git rev-parse --show-toplevel)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
version="$(node "$root/scripts/npm-package-utils.js" current-version)"
export npm_config_cache="${npm_config_cache:-$tmp/npm-cache}"

pack_dir="$tmp/packs"
mkdir -p "$pack_dir"

node "$root/scripts/npm-package-utils.js" validate "$version"

platform_rows=(
  "darwin-arm64 bindport-darwin-arm64 Darwin arm64"
  "darwin-x64 bindport-darwin-x64 Darwin x86_64"
  "linux-arm64 bindport-linux-arm64 Linux aarch64"
  "linux-x64 bindport-linux-x64 Linux x86_64"
)

current_platform_key() {
  case "$(uname -s)-$(uname -m)" in
    Linux-x86_64)
      echo "linux-x64"
      ;;
    Linux-aarch64 | Linux-arm64)
      echo "linux-arm64"
      ;;
    Darwin-x86_64)
      echo "darwin-x64"
      ;;
    Darwin-arm64)
      echo "darwin-arm64"
      ;;
    *)
      echo "unsupported smoke-test platform: $(uname -s)-$(uname -m)" >&2
      exit 1
      ;;
  esac
}

row_field() {
  row="$1"
  field="$2"
  read -r platform_value package_dir_value os_value arch_value <<< "$row"
  case "$field" in
    platform)
      echo "$platform_value"
      ;;
    package_dir)
      echo "$package_dir_value"
      ;;
    os)
      echo "$os_value"
      ;;
    arch)
      echo "$arch_value"
      ;;
    *)
      echo "unknown platform row field: $field" >&2
      exit 1
      ;;
  esac
}

platform_row() {
  requested="$1"
  for row in "${platform_rows[@]}"; do
    if [[ "$(row_field "$row" platform)" == "$requested" ]]; then
      echo "$row"
      return
    fi
  done
  echo "unknown npm smoke platform: $requested" >&2
  exit 1
}

write_fake_binary() {
  binary="$1"
  platform="$2"
  mkdir -p "$(dirname "$binary")"
  cat > "$binary" <<BIN
#!/usr/bin/env sh
printf 'bindport npm smoke $platform %s\n' "\$*"
BIN
  chmod 755 "$binary"
}

assert_output() {
  expected="$1"
  shift
  "$@" | grep -F "$expected" >/dev/null
}

wrapper_tmp="$tmp/bindport"
mkdir -p "$wrapper_tmp"
cp -R "$root/npm/bindport"/. "$wrapper_tmp"/
cp "$root/LICENSE" "$wrapper_tmp/LICENSE"
(
  cd "$wrapper_tmp"
  npm pack --pack-destination "$pack_dir" >/dev/null
)

platform="$(current_platform_key)"
native_row="$(platform_row "$platform")"
package_dir="$(row_field "$native_row" package_dir)"
platform_source="$root/npm/$package_dir"
platform_tmp="$tmp/$package_dir"
mkdir -p "$platform_tmp"
cp -R "$platform_source"/. "$platform_tmp"/
cp "$root/LICENSE" "$platform_tmp/LICENSE"
write_fake_binary "$platform_tmp/bin/bindport" "$platform"
(
  cd "$platform_tmp"
  npm pack --pack-destination "$pack_dir" >/dev/null
)
for package in "$pack_dir/bindport-$version.tgz" "$pack_dir/bindport-$platform-$version.tgz"; do
  tar -tzf "$package" | grep -qx 'package/LICENSE'
done

forwarding_expected="bindport npm smoke $platform -- /bin/sh -c printf \"PORT=%s\\n\" \"\$PORT\""

project="$tmp/project"
mkdir -p "$project"
(
  cd "$project"
  npm init -y >/dev/null
  npm install --silent --offline --ignore-scripts --no-audit --no-fund --omit=optional \
    "$pack_dir/bindport-$version.tgz" \
    "$pack_dir/bindport-$platform-$version.tgz"
  assert_output "bindport npm smoke $platform --version" npx --no-install bindport --version
  assert_output "$forwarding_expected" npx --no-install bindport -- /bin/sh -c "printf \"PORT=%s\\n\" \"\$PORT\""
  assert_output "bindport npm smoke $platform --help" npm exec -- bindport --help
  assert_output "$forwarding_expected" npm exec -- bindport -- /bin/sh -c "printf \"PORT=%s\\n\" \"\$PORT\""
  if command -v bun >/dev/null 2>&1; then
    assert_output "bindport npm smoke $platform doctor" bunx --no-install bindport doctor
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
  assert_output "bindport npm smoke $platform --version" npx --no-install bindport --version
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
    assert_output "bindport npm smoke $platform --version" pnpm exec -- bindport --version
    assert_output "$forwarding_expected" pnpm exec -- bindport -- /bin/sh -c "printf \"PORT=%s\\n\" \"\$PORT\""
  )
fi

matrix_project="$tmp/platform-matrix"
fake_uname_dir="$matrix_project/fake-bin"
mkdir -p \
  "$fake_uname_dir" \
  "$matrix_project/node_modules/.bin" \
  "$matrix_project/node_modules/bindport/bin" \
  "$matrix_project/node_modules/@bindport"
cp "$root/npm/bindport/bin/bindport" "$matrix_project/node_modules/bindport/bin/bindport"
chmod 755 "$matrix_project/node_modules/bindport/bin/bindport"
ln -s ../bindport/bin/bindport "$matrix_project/node_modules/.bin/bindport"
cat > "$fake_uname_dir/uname" <<'UNAME'
#!/usr/bin/env sh
case "$1" in
  -s)
    printf '%s\n' "${BINDPORT_NPM_SMOKE_UNAME_S:?}"
    ;;
  -m)
    printf '%s\n' "${BINDPORT_NPM_SMOKE_UNAME_M:?}"
    ;;
  *)
    /usr/bin/uname "$@"
    ;;
esac
UNAME
chmod 755 "$fake_uname_dir/uname"

for row in "${platform_rows[@]}"; do
  matrix_platform="$(row_field "$row" platform)"
  matrix_package_dir="$(row_field "$row" package_dir)"
  matrix_os="$(row_field "$row" os)"
  matrix_arch="$(row_field "$row" arch)"
  matrix_package="$matrix_project/node_modules/@bindport/$matrix_platform"
  mkdir -p "$matrix_package"
  cp -R "$root/npm/$matrix_package_dir"/. "$matrix_package"/
  write_fake_binary "$matrix_package/bin/bindport" "$matrix_platform"

  PATH="$fake_uname_dir:$PATH" \
    BINDPORT_NPM_SMOKE_UNAME_S="$matrix_os" \
    BINDPORT_NPM_SMOKE_UNAME_M="$matrix_arch" \
    assert_output "bindport npm smoke $matrix_platform --version" \
    "$matrix_project/node_modules/.bin/bindport" --version
done

echo "npm wrapper smoke passed for $platform and all supported platform package paths."
