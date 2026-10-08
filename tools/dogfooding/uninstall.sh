#!/usr/bin/env bash
# Removes the dogfooding LaunchAgent of GitRaptor (M1). Keeps the samples and the reports:
# delete ~/.gitraptor-dogfooding and bitacora/dogfooding/ by hand if you no longer want them.
#
#   tools/dogfooding/uninstall.sh
set -euo pipefail

label="com.gitraptor.dogfooding"
plist="$HOME/Library/LaunchAgents/$label.plist"

launchctl bootout "gui/$(id -u)/$label" 2>/dev/null || true
if [ -f "$plist" ]; then
  rm -f "$plist"
  echo "Removed $plist. The samples stay in ${GITRAPTOR_DOGFOODING_DIR:-$HOME/.gitraptor-dogfooding}."
else
  echo "Nothing to remove: $plist does not exist."
fi
