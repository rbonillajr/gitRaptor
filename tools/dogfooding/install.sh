#!/usr/bin/env bash
# Installs the dogfooding log of GitRaptor (M1) as a user LaunchAgent on macOS.
#
#   tools/dogfooding/install.sh [--interval <minutes>] [--dry-run]
#
# What it installs, and where:
#   - ~/Library/LaunchAgents/com.gitraptor.dogfooding.plist: a LaunchAgent of your user (no sudo)
#     that runs tools/dogfooding/sample.mjs every 15 minutes (or --interval) and at login.
#   - ~/.gitraptor-dogfooding/: the samples (<date>.jsonl), the marks and the log. Outside every
#     repo and outside the GitRaptor profile (NFR-01).
#   - The reports go to bitacora/dogfooding/ of this checkout (local, not committed).
# The sampler runs only `raptor status --resources --json`, `raptor sessions --json` and
# `raptor events --json`, and the last two only when the daemon is already running.
#
# Idempotent: running it again replaces the plist and reloads the agent; the samples stay.
# Undo it with tools/dogfooding/uninstall.sh.
set -euo pipefail

label="com.gitraptor.dogfooding"
interval_min=15
dry_run=0
while [ $# -gt 0 ]; do
  case "$1" in
    --interval) interval_min="$2"; shift 2 ;;
    --dry-run) dry_run=1; shift ;;
    -h|--help) sed -n '2,17p' "$0"; exit 0 ;;
    *) echo "install.sh: unknown option $1" >&2; exit 2 ;;
  esac
done
case "$interval_min" in
  ''|*[!0-9]*) echo "install.sh: --interval takes whole minutes" >&2; exit 2 ;;
esac
if [ "$interval_min" -lt 5 ] || [ "$interval_min" -gt 60 ]; then
  echo "install.sh: --interval must be between 5 and 60 minutes" >&2
  exit 2
fi
if [ "$(uname -s)" != "Darwin" ] && [ "$dry_run" -eq 0 ]; then
  echo "install.sh: launchd is macOS only; on another OS run sample.mjs from cron or a timer" >&2
  exit 2
fi

here="$(cd "$(dirname "$0")" && pwd -P)"
repo="$(cd "$here/../.." && pwd -P)"
# A linked worktree is removed when its branch closes: the agent must point at the main checkout.
common="$(git -C "$repo" rev-parse --path-format=absolute --git-common-dir 2>/dev/null || true)"
main_checkout="${common%/.git}"
if [ -n "$common" ] && [ "$main_checkout" != "$repo" ]; then
  if [ -f "$main_checkout/tools/dogfooding/sample.mjs" ]; then
    echo "install.sh: this is a linked worktree; using the main checkout $main_checkout"
    repo="$main_checkout"
    here="$repo/tools/dogfooding"
  elif [ "$dry_run" -eq 0 ]; then
    echo "install.sh: run it from the main checkout once tools/dogfooding is merged and pulled" >&2
    exit 1
  fi
fi
node_bin="$(command -v node || true)"
raptor_bin="${GITRAPTOR_DOGFOODING_RAPTOR:-$(command -v raptor || true)}"
data_dir="${GITRAPTOR_DOGFOODING_DIR:-$HOME/.gitraptor-dogfooding}"
reports_dir="${GITRAPTOR_DOGFOODING_REPORTS:-$repo/bitacora/dogfooding}"
agents_dir="$HOME/Library/LaunchAgents"
plist="$agents_dir/$label.plist"

[ -n "$node_bin" ] || { echo "install.sh: node is not in PATH (needs Node 22 or later)" >&2; exit 1; }
[ -n "$raptor_bin" ] || { echo "install.sh: raptor is not in PATH (cargo install --path apps/cli)" >&2; exit 1; }
# launchd starts with a bare PATH: give it the folders of node, raptor and git.
path_env="$(dirname "$node_bin"):$(dirname "$raptor_bin"):/usr/local/bin:/opt/homebrew/bin:/usr/bin:/bin"

escape() { printf '%s' "$1" | sed -e 's/[&|\\]/\\&/g' -e 's/</\&lt;/g' -e 's/>/\&gt;/g'; }
rendered="$(sed \
  -e "s|__NODE__|$(escape "$node_bin")|g" \
  -e "s|__SCRIPT__|$(escape "$here/sample.mjs")|g" \
  -e "s|__RAPTOR__|$(escape "$raptor_bin")|g" \
  -e "s|__DATA_DIR__|$(escape "$data_dir")|g" \
  -e "s|__REPORTS_DIR__|$(escape "$reports_dir")|g" \
  -e "s|__PATH__|$(escape "$path_env")|g" \
  -e "s|__INTERVAL__|$((interval_min * 60))|g" \
  "$here/$label.plist")"

cat <<EOF
GitRaptor dogfooding log (M1)
  LaunchAgent : $plist
  Every       : $interval_min minutes, and at login
  Runs        : $node_bin $here/sample.mjs
  raptor      : $raptor_bin
  Samples     : $data_dir
  Reports     : $reports_dir
EOF

if [ "$dry_run" -eq 1 ]; then
  echo "--- dry run: nothing installed. The plist would be:"
  printf '%s\n' "$rendered"
  exit 0
fi

# Refuse a data folder inside a repo or the profile before anything is installed.
GITRAPTOR_DOGFOODING_DIR="$data_dir" "$node_bin" -e '
  import("'"$here"'/lib.mjs").then(({ unsafeDataDir, defaultDataDir }) => {
    const why = unsafeDataDir(defaultDataDir());
    if (why) { console.error("install.sh: refusing the data folder: " + why); process.exit(2); }
  });'

mkdir -p "$data_dir" "$reports_dir" "$agents_dir"
tmp="$(mktemp "$agents_dir/.$label.XXXXXX")"
printf '%s\n' "$rendered" > "$tmp"
plutil -lint "$tmp" >/dev/null
mv "$tmp" "$plist"

domain="gui/$(id -u)"
launchctl bootout "$domain/$label" 2>/dev/null || true
launchctl bootstrap "$domain" "$plist"
echo "Installed. First sample now; see the report in $reports_dir and the log in $data_dir/sampler.log."
