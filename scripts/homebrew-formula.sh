#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
Usage: scripts/homebrew-formula.sh --version X.Y.Z --dist DIR [--output PATH]

Generate the Homebrew formula for the bindport/bindport GitHub Release assets.
DIR must contain the release .sha256 files produced by the Release workflow.
When --output is omitted, the formula is written to stdout.
EOF
}

version=""
dist=""
output=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --version)
      if [[ $# -lt 2 ]]; then
        echo "--version requires a value" >&2
        usage
        exit 2
      fi
      version="$2"
      shift 2
      ;;
    --dist)
      if [[ $# -lt 2 ]]; then
        echo "--dist requires a value" >&2
        usage
        exit 2
      fi
      dist="$2"
      shift 2
      ;;
    --output)
      if [[ $# -lt 2 ]]; then
        echo "--output requires a value" >&2
        usage
        exit 2
      fi
      output="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      if [[ -z "$version" ]]; then
        version="$1"
        shift
      else
        echo "unexpected argument: $1" >&2
        usage
        exit 2
      fi
      ;;
  esac
done

version="${version#v}"
if [[ ! "$version" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]]; then
  echo "version must be stable X.Y.Z or vX.Y.Z, got ${version:-<empty>}" >&2
  exit 2
fi
if [[ "$version" =~ ^0\.0\. ]]; then
  echo "0.0.x is bootstrap-only and must not be packaged" >&2
  exit 2
fi
if [[ -z "$dist" ]]; then
  echo "--dist is required" >&2
  usage
  exit 2
fi
if [[ ! -d "$dist" ]]; then
  echo "dist directory does not exist: $dist" >&2
  exit 1
fi

asset_url() {
  local asset="$1"
  printf 'https://github.com/bindport/bindport/releases/download/v%s/%s' "$version" "$asset"
}

sha_for() {
  local asset="$1"
  local file="$dist/$asset.sha256"
  local sha

  if [[ ! -s "$file" ]]; then
    echo "missing checksum file: $file" >&2
    exit 1
  fi

  read -r sha _ < "$file"
  if [[ ! "$sha" =~ ^[0-9a-f]{64}$ ]]; then
    echo "invalid sha256 in $file" >&2
    exit 1
  fi

  printf '%s' "$sha"
}

render_formula() {
  local linux_x64_sha linux_arm64_sha macos_x64_sha macos_arm64_sha completions_sha manpage_sha
  linux_x64_sha="$(sha_for bindport-linux-x64)"
  linux_arm64_sha="$(sha_for bindport-linux-arm64)"
  macos_x64_sha="$(sha_for bindport-macos-x64)"
  macos_arm64_sha="$(sha_for bindport-macos-arm64)"
  completions_sha="$(sha_for bindport-completions.tar.gz)"
  manpage_sha="$(sha_for bindport-manpage.tar.gz)"

  cat <<EOF
class Bindport < Formula
  desc "Proxy-neutral local development port registry and runner"
  homepage "https://github.com/bindport/bindport"
  version "$version"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "$(asset_url bindport-macos-arm64)"
      sha256 "$macos_arm64_sha"
    else
      url "$(asset_url bindport-macos-x64)"
      sha256 "$macos_x64_sha"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "$(asset_url bindport-linux-arm64)"
      sha256 "$linux_arm64_sha"
    else
      url "$(asset_url bindport-linux-x64)"
      sha256 "$linux_x64_sha"
    end
  end

  resource "completions" do
    url "$(asset_url bindport-completions.tar.gz)"
    sha256 "$completions_sha"
  end

  resource "manpage" do
    url "$(asset_url bindport-manpage.tar.gz)"
    sha256 "$manpage_sha"
  end

  def install
    binary = Dir["bindport-*"].find { |path| File.file?(path) }
    odie "bindport release binary was not staged" if binary.nil?

    chmod 0755, binary
    bin.install binary => "bindport"

    resource("completions").stage do
      bash_completion.install "completions/bash/bindport"
      zsh_completion.install "completions/zsh/_bindport"
      fish_completion.install "completions/fish/bindport.fish"
    end

    resource("manpage").stage do
      man1.install "man/man1/bindport.1"
    end
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/bindport --version")
    assert_match "BindPort", shell_output("#{bin}/bindport --help")
  end
end
EOF
}

if [[ -n "$output" ]]; then
  mkdir -p "$(dirname "$output")"
  tmp="$(mktemp)"
  trap 'rm -f "$tmp"' EXIT
  render_formula > "$tmp"
  mv "$tmp" "$output"
else
  render_formula
fi
