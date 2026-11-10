#!/bin/sh
# DSK002-RELEASE-JOB (TS-DSK-002 T008): static shape of the `desktop-app` job of release.yml,
# of tauri.release.conf.json and of .gitignore. The real build runs in the CI of the PR (D-E).
set -eu
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
wf="$repo/.github/workflows/release.yml"
conf="$repo/apps/desktop/src-tauri/tauri.release.conf.json"
fail() { echo "FAIL $*"; exit 1; }

# The text of a top-level job (two-space indent) up to the next one.
job() {
  awk -v j="  $1:" '
    $0 == j { on = 1; print; next }
    on && /^  [A-Za-z0-9_-]+:/ { exit }
    on { print }
  ' "$wf"
}

desktop=$(job desktop-app)
[ -n "$desktop" ] || fail "release.yml has no job desktop-app"
has() { printf '%s\n' "$desktop" | grep -Fq -- "$1" || fail "desktop-app: missing: $1"; }

has "name: desktop app (aarch64-apple-darwin)"
printf '%s\n' "$desktop" | grep -Eq '^    needs: \[ *plan, *build *\]$' || fail "desktop-app: needs must be [plan, build]"
has "runs-on: macos-latest"
printf '%s\n' "$desktop" | grep -Eq '^    permissions:' || fail "desktop-app: no permissions block"
perms=$(printf '%s\n' "$desktop" | awk '/^    permissions:/ { on = 1; next } on && /^      / { print; next } on { exit }')
[ "$(printf '%s\n' "$perms" | sed 's/ *#.*//' | tr -d ' ')" = "contents:read" ] || fail "desktop-app: permissions must be exactly contents: read"
if printf '%s\n' "$desktop" | grep -q 'secrets\.'; then fail "desktop-app: must not use secrets.*"; fi
has "tauri build"
has "--bundles dmg"
has "--config src-tauri/tauri.release.conf.json"
has "hdiutil attach -nobrowse -readonly"
has "GitRaptor.app/Contents/MacOS/raptor"
has "raptor-mcp"
has "--version"
# The .dmg is renamed to GitRaptor-<version>-aarch64-apple-darwin.dmg (version of needs.plan.outputs)
# and uploaded as the artifact archive-desktop-aarch64-apple-darwin (critic PC-1).
printf '%s\n' "$desktop" | grep -Eq 'GitRaptor-.*-aarch64-apple-darwin\.dmg' || fail "desktop-app: the .dmg is not renamed to GitRaptor-<version>-aarch64-apple-darwin.dmg"
printf '%s\n' "$desktop" | grep -q 'needs\.plan\.outputs\.version\|\$VERSION\|\${VERSION}' || fail "desktop-app: the version of the .dmg name must come from needs.plan.outputs.version"
has "name: archive-desktop-aarch64-apple-darwin"
has "actions/upload-artifact"
echo "ok   desktop-app job shape"

assemble=$(job assemble)
printf '%s\n' "$assemble" | grep -E '^    needs:' | grep -q 'desktop-app' || fail "assemble: needs must list desktop-app"
printf '%s\n' "$assemble" | grep -E 'sha256sum' | grep -q 'GitRaptor-' || fail "assemble: must hash GitRaptor-*.dmg (its .sha256 and its line in SHA256SUMS)"
echo "ok   assemble needs desktop-app and hashes the .dmg"

[ -f "$conf" ] || fail "tauri.release.conf.json does not exist"
node -e '
const assert = require("assert");
const got = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8"));
assert.deepStrictEqual(got, { bundle: { externalBin: ["binaries/raptor", "binaries/raptor-mcp"], targets: ["dmg"], macOS: { signingIdentity: "-" } } });
' "$conf" 2>/dev/null || fail "tauri.release.conf.json is not exactly externalBin raptor and raptor-mcp, targets dmg, signingIdentity -"
echo "ok   tauri.release.conf.json is exact"

grep -Fxq 'apps/desktop/src-tauri/binaries/' "$repo/.gitignore" || fail ".gitignore lacks apps/desktop/src-tauri/binaries/"
echo "ok   .gitignore ignores the staged sidecars"
