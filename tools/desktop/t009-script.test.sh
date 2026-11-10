#!/bin/sh
# DSK002-T009-SCRIPT (TS-DSK-002 T009, critic PC-4): the manual verification script exists and carries
# the four steps of the Dev Spec; step 3 goes through the autostart row of the app and names the plist;
# step 4 names `not-offered` and `location`.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
doc="$repo/docs/dev-briefs/ts-dsk-002-t009-verificacion-macos.md"
fail() { echo "FAIL $*"; exit 1; }

[ -f "$doc" ] || fail "docs/dev-briefs/ts-dsk-002-t009-verificacion-macos.md does not exist"

# The text of "Paso <n>" up to the next "Paso" heading (or the end).
step() {
  awk -v n="$1" '
    tolower($0) ~ /^#+ .*paso [0-9]/ { on = ($0 ~ ("[Pp]aso " n "([^0-9]|$)")); if (on) print; next }
    on { print }
  ' "$doc"
}

for n in 1 2 3 4; do
  body=$(step "$n")
  [ -n "$body" ] || fail "no heading for Paso $n"
  [ "$(printf '%s\n' "$body" | wc -l | tr -d ' ')" -ge 3 ] || fail "Paso $n is an empty heading: it needs the commands and what to observe"
done
echo "ok   four steps"

step3=$(step 3)
printf '%s\n' "$step3" | grep -Fq '~/Library/LaunchAgents/dev.gitraptor.plist' || fail "Paso 3 must cite ~/Library/LaunchAgents/dev.gitraptor.plist"
printf '%s\n' "$step3" | grep -Eqi 'fila de autoarranque|autostart row' || fail "Paso 3 must go through the autostart row of the app (fila de autoarranque)"
printf '%s\n' "$step3" | grep -Eqi 'cancelar' || fail "Paso 3 must cover Cancelar (no plist)"
printf '%s\n' "$step3" | grep -Eqi 'activar' || fail "Paso 3 must cover Activar (plist with the bundle path)"
echo "ok   step 3"

step4=$(step 4)
printf '%s\n' "$step4" | grep -Fq 'not-offered' || fail "Paso 4 must mention not-offered"
printf '%s\n' "$step4" | grep -Fq 'location' || fail "Paso 4 must mention location"
echo "ok   step 4"
