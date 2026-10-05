#!/bin/sh
# GitRaptor installer for macOS and Linux (INF-GRP-004, ADR-GRP-014 § 5).
#
#   curl -fsSL https://github.com/rbonillajr/gitRaptor/releases/latest/download/install.sh | sh
#   sh install.sh --version 0.1.0
#
# Downloads the archive for this machine and SHA256SUMS from the GitHub Release, verifies the
# checksum and only then installs `raptor` and `raptor-mcp` into ~/.local/bin. It never edits
# PATH or shell profiles, never registers autostart and never touches GitRaptor's data.
#
# Environment:
#   RAPTOR_VERSION        version to install (default: the latest release)
#   RAPTOR_INSTALL_DIR    destination directory (default: $HOME/.local/bin)
#   RAPTOR_DOWNLOAD_BASE  base URL that holds the archives and SHA256SUMS (default: the release)
#
# SHA256SUMS comes from the same origin as the archive: it proves integrity, not authenticity.
# For authenticity, verify the archive with `gh attestation verify <archive> --repo rbonillajr/gitRaptor`.
set -eu

REPO="rbonillajr/gitRaptor"
version="${RAPTOR_VERSION:-}"
install_dir="${RAPTOR_INSTALL_DIR:-${HOME}/.local/bin}"

say() { printf 'raptor-install: %s\n' "$*"; }
fail() { printf 'raptor-install: error: %s\n' "$*" >&2; exit 1; }

while [ $# -gt 0 ]; do
  case "$1" in
    --version) [ $# -ge 2 ] || fail "--version needs a value"; version="$2"; shift 2 ;;
    --version=*) version="${1#--version=}"; shift ;;
    --install-dir) [ $# -ge 2 ] || fail "--install-dir needs a value"; install_dir="$2"; shift 2 ;;
    -h|--help) sed -n '2,18p' "$0" 2>/dev/null || true; exit 0 ;;
    *) fail "unknown argument: $1" ;;
  esac
done

command -v curl >/dev/null 2>&1 || fail "curl is required"
command -v tar >/dev/null 2>&1 || fail "tar is required"
if command -v sha256sum >/dev/null 2>&1; then
  sha256() { sha256sum "$1" | cut -d' ' -f1; }
elif command -v shasum >/dev/null 2>&1; then
  sha256() { shasum -a 256 "$1" | cut -d' ' -f1; }
else
  fail "sha256sum or shasum is required to verify the download"
fi

os=$(uname -s)
arch=$(uname -m)
case "$os" in
  Linux) suffix="unknown-linux-musl" ;;
  Darwin)
    suffix="apple-darwin"
    # A shell under Rosetta reports x86_64 on Apple silicon: install the native build.
    if [ "$arch" = x86_64 ] && [ "$(sysctl -n hw.optional.arm64 2>/dev/null || echo 0)" = 1 ]; then
      arch=arm64
    fi
    ;;
  *) fail "unsupported OS: $os (on Windows use install.ps1)" ;;
esac
case "$arch" in
  x86_64|amd64) cpu="x86_64" ;;
  arm64|aarch64) cpu="aarch64" ;;
  *) fail "unsupported CPU architecture: $arch" ;;
esac
target="${cpu}-${suffix}"

if [ -z "$version" ]; then
  # The latest release redirects to .../releases/tag/v<version>; no API token or jq needed.
  location=$(curl -fsSI "https://github.com/${REPO}/releases/latest" | tr -d '\r' \
    | sed -n 's#^[Ll]ocation: .*/releases/tag/v\(.*\)$#\1#p' | tail -1)
  [ -n "$location" ] || fail "could not resolve the latest release; pass --version"
  version="$location"
fi
version="${version#v}"

base="${RAPTOR_DOWNLOAD_BASE:-https://github.com/${REPO}/releases/download/v${version}}"
archive="raptor-${version}-${target}.tar.gz"

tmp=$(mktemp -d 2>/dev/null || mktemp -d -t raptor-install)
trap 'rm -rf "$tmp"' EXIT INT TERM

say "downloading ${archive}"
curl -fsSL --proto '=https,file' --tlsv1.2 -o "$tmp/$archive" "$base/$archive" \
  || fail "download failed: $base/$archive"
curl -fsSL --proto '=https,file' --tlsv1.2 -o "$tmp/SHA256SUMS" "$base/SHA256SUMS" \
  || fail "download failed: $base/SHA256SUMS"

expected=$(awk -v f="$archive" '$2 == f || $2 == "*" f { print $1 }' "$tmp/SHA256SUMS")
[ -n "$expected" ] || fail "$archive is not listed in SHA256SUMS"
actual=$(sha256 "$tmp/$archive")
[ "$expected" = "$actual" ] || fail "checksum mismatch for $archive (expected $expected, got $actual); nothing was installed"
say "checksum verified (sha256 $actual)"

tar -xzf "$tmp/$archive" -C "$tmp"
src="$tmp/raptor-${version}-${target}"
[ -f "$src/raptor" ] && [ -f "$src/raptor-mcp" ] && [ -f "$src/raptor-hook" ] || fail "unexpected archive layout"

mkdir -p "$install_dir"
# raptor-hook is the Guardrails dispatcher: it must sit next to raptor (US-GRD-001).
for bin in raptor raptor-mcp raptor-hook; do
  # Copy next to the destination, then rename: a running binary is never half-written.
  cp "$src/$bin" "$install_dir/.$bin.tmp.$$"
  chmod 755 "$install_dir/.$bin.tmp.$$"
  mv -f "$install_dir/.$bin.tmp.$$" "$install_dir/$bin"
done
say "installed raptor ${version} (${target}) into ${install_dir}"

case ":${PATH}:" in
  *":${install_dir}:"*) ;;
  *) say "note: ${install_dir} is not on your PATH; add it to your shell profile to run 'raptor'" ;;
esac
