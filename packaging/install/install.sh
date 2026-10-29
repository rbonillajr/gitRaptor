#!/bin/sh
# GitRaptor installer for macOS and Linux (INF-GRP-004, ADR-GRP-014 § 5 and the 2026-10-28 amendment).
#
#   curl -fsSL https://github.com/rbonillajr/gitRaptor/releases/download/v0.1.0-alpha.1/install.sh | sh
#   sh install.sh --version 0.1.0-alpha.1
#   sh install.sh --system            # /usr/local/bin, uses sudo only to copy the 3 files
#   sh install.sh --uninstall         # removes only raptor, raptor-mcp and raptor-hook
#
# Downloads the archive for this machine and SHA256SUMS from the GitHub Release, verifies the
# checksum and the archive layout and only then installs `raptor`, `raptor-mcp` and `raptor-hook`
# into ~/.local/bin. It never edits PATH or shell profiles, never registers autostart and never
# touches GitRaptor's data (the profile and the Time Machine store).
#
# macOS: the early-access builds are not notarized. A download made by a browser carries the
# com.apple.quarantine attribute, and an archive saved that way may carry it into the extracted
# files (curl alone does not set it). When (and only when) the attribute is present on the 3
# installed files, the installer removes it and says so.
#
# Environment:
#   RAPTOR_VERSION        version to install (default: the one this script was published with,
#                         else the latest stable release; prereleases need an explicit version)
#   RAPTOR_INSTALL_DIR    destination directory (default: $HOME/.local/bin)
#   RAPTOR_DOWNLOAD_BASE  base URL that holds the archives and SHA256SUMS (default: the release);
#                         a file:// URL is accepted only here, for tests
#
# SHA256SUMS comes from the same origin as the archive: it proves integrity, not authenticity, and
# the early-access builds are verifiable only if the release is attested (RELEASE_ATTEST).
# For authenticity, verify the archive with `gh attestation verify <archive> --repo rbonillajr/gitRaptor`.
set -eu
umask 022

REPO="rbonillajr/gitRaptor"
# Stamped by the release pipeline for prereleases; empty in the repository.
default_version=""
version="${RAPTOR_VERSION:-}"
install_dir="${RAPTOR_INSTALL_DIR:-${HOME}/.local/bin}"
system=0
uninstall=0
BINS="raptor raptor-mcp raptor-hook"

say() { printf 'raptor-install: %s\n' "$*"; }
fail() { printf 'raptor-install: error: %s\n' "$*" >&2; exit 1; }

while [ $# -gt 0 ]; do
  case "$1" in
    --version) [ $# -ge 2 ] || fail "--version needs a value"; version="$2"; shift 2 ;;
    --version=*) version="${1#--version=}"; shift ;;
    --install-dir) [ $# -ge 2 ] || fail "--install-dir needs a value"; install_dir="$2"; shift 2 ;;
    --system) system=1; install_dir="/usr/local/bin"; shift ;;
    --uninstall) uninstall=1; shift ;;
    -h|--help) awk 'NR > 1 { if ($0 ~ /^set -eu/) exit; print }' "$0" 2>/dev/null || true; exit 0 ;;
    *) fail "unknown argument: $1" ;;
  esac
done

case "$install_dir" in
  /*) ;;
  *) fail "the install directory must be an absolute path: $install_dir" ;;
esac

# sudo only when the directory is not writable by this user. A directory the user can write
# (Homebrew on Intel owns /usr/local/bin) needs no privilege, and using sudo there would let a
# planted symlink in it redirect a root-owned write.
priv() { if [ "$system" = 1 ] && [ "$(id -u)" != 0 ] && [ ! -w "$install_dir" ]; then sudo "$@"; else "$@"; fi; }

if [ "$uninstall" = 1 ]; then
  # Only the 3 files this installer puts there, and only when they are regular files that are
  # GitRaptor's. Never `rm -r`, never the profile, never the Time Machine store (NFR-01).
  if [ -f "$HOME/Library/LaunchAgents/dev.gitraptor.plist" ] || [ -f "$HOME/.config/systemd/user/gitraptor.service" ]; then
    fail "the login autostart is still registered; run 'raptor daemon disable' first, then uninstall"
  fi
  # Nothing in the directory is executed. A file is removed only if it is a regular file that
  # carries GitRaptor's name inside it; any symlink or foreign file aborts before anything goes.
  for bin in $BINS; do
    f="$install_dir/$bin"
    [ -e "$f" ] || [ -L "$f" ] || continue
    if [ -L "$f" ] || [ ! -f "$f" ] || ! grep -aq gitraptor "$f"; then
      fail "$f is not a GitRaptor binary installed by this script; nothing was removed"
    fi
  done
  removed=0
  for bin in $BINS; do
    if [ -f "$install_dir/$bin" ] && [ ! -L "$install_dir/$bin" ]; then
      priv rm -f "$install_dir/$bin"; removed=$((removed + 1)); say "removed $install_dir/$bin"
    fi
  done
  [ "$removed" -gt 0 ] || say "nothing to remove in $install_dir"
  say "your data was not touched (macOS: ~/Library/Application Support/gitraptor). Forget it only if you want to lose the history."
  exit 0
fi

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

[ -n "$version" ] || version="$default_version"
if [ -z "$version" ]; then
  # The latest release redirects to .../releases/tag/v<version>; no API token or jq needed.
  # It skips prereleases and drafts: while only alphas exist, pass --version.
  location=$(curl -fsSI --proto '=https' --tlsv1.2 "https://github.com/${REPO}/releases/latest" | tr -d '\r' \
    | sed -n 's#^[Ll]ocation: .*/releases/tag/v\(.*\)$#\1#p' | tail -1)
  [ -n "$location" ] || fail "could not resolve the latest stable release; pass --version (for example 0.1.0-alpha.1)"
  version="$location"
fi
version="${version#v}"
# The version goes into a URL and a path: nothing but a SemVer string gets through.
printf '%s' "$version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$' \
  || fail "invalid version: $version"

if [ -n "${RAPTOR_DOWNLOAD_BASE:-}" ]; then
  base="$RAPTOR_DOWNLOAD_BASE"
  say "warning: using a non-default download source: $base"
  proto='=https,file'
else
  base="https://github.com/${REPO}/releases/download/v${version}"
  proto='=https'
fi
archive="raptor-${version}-${target}.tar.gz"

tmp=$(mktemp -d 2>/dev/null || mktemp -d -t raptor-install)
trap 'rm -rf "$tmp"' EXIT
trap 'exit 130' INT TERM

say "downloading ${archive}"
curl -fsSL --proto "$proto" --proto-redir '=https' --tlsv1.2 -o "$tmp/$archive" "$base/$archive" \
  || fail "download failed: $base/$archive"
curl -fsSL --proto "$proto" --proto-redir '=https' --tlsv1.2 -o "$tmp/SHA256SUMS" "$base/SHA256SUMS" \
  || fail "download failed: $base/SHA256SUMS"

expected=$(awk -v f="$archive" '$2 == f || $2 == "*" f { print $1 }' "$tmp/SHA256SUMS")
[ -n "$expected" ] || fail "$archive is not listed in SHA256SUMS"
actual=$(sha256 "$tmp/$archive")
[ "$expected" = "$actual" ] || fail "checksum mismatch for $archive (expected $expected, got $actual); nothing was installed"
say "checksum verified (sha256 $actual)"

# Before extracting anything: the archive holds exactly the expected folder and files, with no
# absolute path, no `..` and no link.
dir="raptor-${version}-${target}"
tar -tzf "$tmp/$archive" > "$tmp/list" || fail "unreadable archive"
while IFS= read -r entry; do
  case "$entry" in
    "$dir"|"$dir/"|"$dir/raptor"|"$dir/raptor-mcp"|"$dir/raptor-hook"|"$dir/README.md"|"$dir/LICENSE") ;;
    *) fail "unexpected entry in the archive: $entry; nothing was installed" ;;
  esac
done < "$tmp/list"
tar -tvzf "$tmp/$archive" | cut -c1 | grep -Eq '^[lh]' && fail "the archive contains a link; nothing was installed"
mkdir "$tmp/x"
tar -xzf "$tmp/$archive" -C "$tmp/x"
src="$tmp/x/$dir"
for bin in $BINS; do
  [ -f "$src/$bin" ] && [ ! -L "$src/$bin" ] || fail "unexpected archive layout: $bin"
done

# A writable-by-everyone directory would let anyone swap the binaries after the install, and a
# group-writable one must not be written as root.
if [ -d "$install_dir" ]; then
  [ -z "$(find "$install_dir" -maxdepth 0 -perm -002)" ] \
    || fail "$install_dir is writable by everyone; refusing to install there"
  if [ "$system" = 1 ] && [ ! -w "$install_dir" ] \
     && [ -z "$(find "$install_dir" -maxdepth 0 -user 0 ! -perm -020)" ]; then
    fail "$install_dir is not owned by root or is group-writable; refusing to write it as root"
  fi
fi

priv mkdir -p "$install_dir"
# raptor-hook is the Guardrails dispatcher: it must sit next to raptor (US-GRD-001).
for bin in $BINS; do
  # Copy next to the destination, then rename: a running binary is never half-written. The
  # temporary name comes from mktemp (created exclusively), never from a guessable PID.
  tmpf=$(priv mktemp "$install_dir/.$bin.XXXXXX")
  priv cp "$src/$bin" "$tmpf"
  priv chmod 755 "$tmpf"
  priv mv -f "$tmpf" "$install_dir/$bin"
done
say "installed raptor ${version} (${target}) into ${install_dir}"

if [ "$os" = Darwin ]; then
  # Only the 3 exact files, only when the attribute is there, never `xattr -c` or `-r`.
  for bin in $BINS; do
    if xattr -p com.apple.quarantine "$install_dir/$bin" >/dev/null 2>&1; then
      priv xattr -d com.apple.quarantine "$install_dir/$bin"
      say "notice: removed the macOS quarantine from $install_dir/$bin. This build is NOT notarized (early access); its integrity was verified by SHA256."
    fi
  done
  for bin in $BINS; do
    codesign --verify "$install_dir/$bin" >/dev/null 2>&1 || say "warning: codesign could not verify $install_dir/$bin"
  done
fi

out=$("$install_dir/raptor" --version 2>/dev/null || true)
[ "$out" = "raptor $version" ] || fail "the installed binary reports '$out', expected 'raptor $version'"
say "ok: $out"

case ":${PATH}:" in
  *":${install_dir}:"*) ;;
  *) say "note: ${install_dir} is not on your PATH; add it to your shell profile to run 'raptor'" ;;
esac
