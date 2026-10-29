#!/bin/sh
# Tests for install.sh against fake archives in a temporary folder. Never touches the real
# profile, PATH or $HOME: HOME is a throwaway too.  Run: sh packaging/install/install.test.sh
set -eu

here=$(cd "$(dirname "$0")" && pwd)
installer="$here/install.sh"
root=$(mktemp -d)
trap 'rm -rf "$root"' EXIT INT TERM
V="0.1.0-alpha.1"
case "$(uname -s)" in
  Linux) suffix="unknown-linux-musl" ;;
  Darwin) suffix="apple-darwin" ;;
esac
case "$(uname -m)" in
  x86_64|amd64) cpu=x86_64 ;;
  *) cpu=aarch64 ;;
esac
# Under Rosetta uname -m says x86_64 while the installer picks the native build: mirror it.
if [ "$(uname -s)" = Darwin ] && [ "$(sysctl -n hw.optional.arm64 2>/dev/null || echo 0)" = 1 ]; then cpu=aarch64; fi
name="raptor-$V-$cpu-$suffix"

pass=0
check() { if [ "$1" = ok ]; then pass=$((pass + 1)); printf 'ok   %s\n' "$2"; else printf 'FAIL %s\n' "$2"; exit 1; fi; }

sha() { if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1"; else shasum -a 256 "$1"; fi; }

# dist/<case>/ holds the archive and SHA256SUMS the installer downloads.
mkdist() { # $1 = case dir
  mkdir -p "$1/stage/$name"
  for b in raptor raptor-mcp raptor-hook; do
    printf '#!/bin/sh\n# gitraptor\necho "raptor %s"\n' "$V" > "$1/stage/$name/$b"
    chmod 755 "$1/stage/$name/$b"
  done
  echo readme > "$1/stage/$name/README.md"; echo license > "$1/stage/$name/LICENSE"
}
pack() { # $1 = case dir
  tar -C "$1/stage" -czf "$1/$name.tar.gz" "$name"
  (cd "$1" && sha "$name.tar.gz" | awk '{print $1"  "$2}' > SHA256SUMS)
}
run() { # $1 = case dir, rest = installer args
  d="$1"; shift
  HOME="$root/home" RAPTOR_DOWNLOAD_BASE="file://$d" RAPTOR_INSTALL_DIR="$d/bin" sh "$installer" "$@"
}
mkdir -p "$root/home"

# 1. happy path
c="$root/ok"; mkdist "$c"; pack "$c"
if run "$c" --version "$V" >/dev/null 2>&1 && [ "$("$c/bin/raptor" --version)" = "raptor $V" ] \
   && [ -x "$c/bin/raptor-mcp" ] && [ -x "$c/bin/raptor-hook" ]; then check ok "installs and verifies"; else check no "installs and verifies"; fi

# 2. tampered checksum installs nothing
c="$root/bad"; mkdist "$c"; pack "$c"
printf '%064d  %s\n' 0 "$name.tar.gz" > "$c/SHA256SUMS"
if run "$c" --version "$V" >/dev/null 2>&1 || [ -e "$c/bin/raptor" ]; then check no "rejects a tampered checksum"; else check ok "rejects a tampered checksum"; fi

# 3. invalid versions never reach a URL or a path
c="$root/ver"; mkdist "$c"; pack "$c"
for v in '../x' '0.1.0/evil' '0.1' 'latest' '1.0.0;id'; do
  if run "$c" --version "$v" >/dev/null 2>&1; then check no "rejects version $v"; fi
done
check ok "rejects malformed versions"

# 4. an archive with an unexpected entry (path escape / extra file) installs nothing
c="$root/evil"; mkdist "$c"; echo x > "$c/stage/$name/extra"; pack "$c"
if run "$c" --version "$V" >/dev/null 2>&1 || [ -e "$c/bin/raptor" ]; then check no "rejects an unexpected archive entry"; else check ok "rejects an unexpected archive entry"; fi

# 5. an archive with a link installs nothing
c="$root/link"; mkdist "$c"; rm "$c/stage/$name/raptor-hook"; ln -s /bin/sh "$c/stage/$name/raptor-hook"; pack "$c"
if run "$c" --version "$V" >/dev/null 2>&1 || [ -e "$c/bin/raptor" ]; then check no "rejects a link in the archive"; else check ok "rejects a link in the archive"; fi

# 6. a world-writable destination is refused
c="$root/ww"; mkdist "$c"; pack "$c"; mkdir "$c/bin"; chmod 777 "$c/bin"
if run "$c" --version "$V" >/dev/null 2>&1; then check no "refuses a world-writable directory"; else check ok "refuses a world-writable directory"; fi

# 7. a relative install dir is refused
c="$root/rel"; mkdist "$c"; pack "$c"
if HOME="$root/home" RAPTOR_DOWNLOAD_BASE="file://$c" RAPTOR_INSTALL_DIR="bin" sh "$installer" --version "$V" >/dev/null 2>&1; then
  check no "refuses a relative install dir"; else check ok "refuses a relative install dir"; fi

# 8. quarantine is removed (macOS only) and said out loud
if [ "$(uname -s)" = Darwin ]; then
  c="$root/q"; mkdist "$c"
  xattr -w com.apple.quarantine "0083;00000000;Safari;" "$c/stage/$name/raptor"
  pack "$c"   # bsdtar keeps the attribute in the archive (._ entry is not expected): see below
  out=$(run "$c" --version "$V" 2>&1) || { echo "$out"; check no "removes the quarantine"; }
  if xattr -p com.apple.quarantine "$c/bin/raptor" >/dev/null 2>&1; then check no "removes the quarantine"; fi
  check ok "no quarantine left on installed binaries"
fi

# 9. uninstall removes only the 3 files and leaves the rest
c="$root/un"; mkdist "$c"; pack "$c"; run "$c" --version "$V" >/dev/null 2>&1
echo keep > "$c/bin/other"; mkdir -p "$root/home/Library/Application Support/gitraptor"
HOME="$root/home" RAPTOR_INSTALL_DIR="$c/bin" sh "$installer" --uninstall >/dev/null 2>&1
if [ ! -e "$c/bin/raptor" ] && [ ! -e "$c/bin/raptor-mcp" ] && [ ! -e "$c/bin/raptor-hook" ] \
   && [ -f "$c/bin/other" ] && [ -d "$root/home/Library/Application Support/gitraptor" ]; then
  check ok "uninstall removes only the 3 binaries"; else check no "uninstall removes only the 3 binaries"; fi

# 10. uninstall refuses while the autostart is registered
c="$root/un2"; mkdist "$c"; pack "$c"; run "$c" --version "$V" >/dev/null 2>&1
mkdir -p "$root/home/.config/systemd/user"; : > "$root/home/.config/systemd/user/gitraptor.service"
if HOME="$root/home" RAPTOR_INSTALL_DIR="$c/bin" sh "$installer" --uninstall >/dev/null 2>&1 || [ ! -e "$c/bin/raptor" ]; then
  check no "uninstall refuses while autostart is on"; else check ok "uninstall refuses while autostart is on"; fi

# 11. uninstall removes nothing when one file is foreign or a symlink
rm -f "$root/home/.config/systemd/user/gitraptor.service"
c="$root/un3"; mkdir -p "$c/bin"; printf '#!/bin/sh\necho other tool\n' > "$c/bin/raptor"; chmod 755 "$c/bin/raptor"
HOME="$root/home" RAPTOR_INSTALL_DIR="$c/bin" sh "$installer" --uninstall >/dev/null 2>&1 || true
if [ -e "$c/bin/raptor" ]; then check ok "uninstall keeps a foreign raptor"; else check no "uninstall keeps a foreign raptor"; fi


# 12. a symlink among the 3 names aborts the uninstall before anything is removed
c="$root/un4"; mkdist "$c"; pack "$c"; run "$c" --version "$V" >/dev/null 2>&1
rm "$c/bin/raptor-hook"; ln -s /bin/sh "$c/bin/raptor-hook"
HOME="$root/home" RAPTOR_INSTALL_DIR="$c/bin" sh "$installer" --uninstall >/dev/null 2>&1 || true
if [ -e "$c/bin/raptor" ] && [ -e "$c/bin/raptor-mcp" ]; then check ok "uninstall aborts on a symlink before removing"; else check no "uninstall aborts on a symlink before removing"; fi
printf '%s checks passed\n' "$pass"
