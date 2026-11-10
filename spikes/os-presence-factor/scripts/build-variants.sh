#!/bin/bash
# SPIKE-GRD-002 E9/E15: packaging variants of presence-probe under variants/.
# usage: scripts/build-variants.sh   (from spikes/os-presence-factor)
set -eu
cd "$(dirname "$0")/.."
V="$PWD/variants"; rm -rf "$V"; mkdir -p "$V"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-4}"

cargo build --release --quiet
cp target/release/presence-probe "$V/linker-adhoc"                       # default on arm64: ad hoc, linker-signed
cp "$V/linker-adhoc" "$V/unsigned"; codesign --remove-signature "$V/unsigned"
cp "$V/linker-adhoc" "$V/adhoc"; codesign -f -s - -i dev.gitraptor.spike.adhoc "$V/adhoc" 2>/dev/null
cp "$V/linker-adhoc" "$V/adhoc-hardened"; codesign -f -s - -o runtime -i dev.gitraptor.spike.hardened "$V/adhoc-hardened" 2>/dev/null

PROBE_INFO_PLIST="$PWD/bundle/Info.plist" cargo build --release --quiet
cp target/release/presence-probe "$V/plist-adhoc"
cp "$V/plist-adhoc" "$V/plist-hardened"; codesign -f -s - -o runtime "$V/plist-hardened" 2>/dev/null
# Restore the plain build so target/release is the default binary again.
touch build.rs; cargo build --release --quiet

# Local development identity, if any (never printed or recorded).
DEVID="$(security find-identity -v -p codesigning 2>/dev/null | awk '/Apple Development/ {print $2; exit}')"
if [ -n "$DEVID" ]; then
  cp "$V/plist-adhoc" "$V/plist-devsigned"; codesign -f -s "$DEVID" -o runtime "$V/plist-devsigned" 2>/dev/null
fi

# E15: test app bundle; the headless binary sits next to the main executable (Tauri externalBin).
APP="$V/GitRaptorSpikeApp.app"
mkdir -p "$APP/Contents/MacOS"
cp bundle/App-Info.plist "$APP/Contents/Info.plist"
cp "$V/linker-adhoc" "$APP/Contents/MacOS/GitRaptorSpikeApp"
cp "$V/linker-adhoc" "$APP/Contents/MacOS/raptor"
codesign -f -s - "$APP/Contents/MacOS/raptor" 2>/dev/null
codesign -f -s - "$APP" 2>/dev/null
if [ -n "$DEVID" ]; then
  DAPP="$V/GitRaptorSpikeAppDev.app"; cp -R "$APP" "$DAPP"
  codesign -f -s "$DEVID" -o runtime "$DAPP/Contents/MacOS/raptor" 2>/dev/null
  codesign -f -s "$DEVID" -o runtime "$DAPP" 2>/dev/null
fi

for f in "$V"/*; do
  printf '%-28s ' "$(basename "$f")"
  codesign -dv "$f" 2>&1 | awk -F= '/^Signature=|^flags|CodeDirectory/ {printf "%s ", $0}' ; echo
done
