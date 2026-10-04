# shellcheck shell=bash
# Common helpers for SPIKE-GRD-001. Source from a suite script.
#
# Safety (NFR-01): every repository is created under a fresh `mktemp -d`
# sandbox. `sandbox_guard` refuses to run any command outside that sandbox.

set -u

SPIKE_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LIB="$SPIKE_ROOT/lib"

# Git under test: GIT_BIN_DIR (directory holding `git`) or the first in PATH.
if [ -n "${GIT_BIN_DIR:-}" ]; then
  export PATH="$GIT_BIN_DIR:$PATH"
fi
GIT_VERSION="$(git --version | awk '{print $3}')"

# Neutralize the user's global/system configuration: suites must not depend
# on (nor read) the developer's real Git setup.
export GIT_CONFIG_NOSYSTEM=1
export GIT_AUTHOR_NAME=spike GIT_AUTHOR_EMAIL=spike@example.invalid
export GIT_COMMITTER_NAME=spike GIT_COMMITTER_EMAIL=spike@example.invalid
export GIT_AUTHOR_DATE='2026-01-01T00:00:00Z' GIT_COMMITTER_DATE='2026-01-01T00:00:00Z'
unset GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE GIT_CONFIG_PARAMETERS GIT_CONFIG_COUNT

ZERO_OID=0000000000000000000000000000000000000000

HOOK_NAMES="applypatch-msg pre-applypatch post-applypatch pre-commit pre-merge-commit
prepare-commit-msg commit-msg post-commit pre-rebase post-checkout post-merge pre-push
pre-receive update proc-receive post-receive post-update reference-transaction
push-to-checkout pre-auto-gc post-rewrite sendemail-validate fsmonitor-watchman
p4-changelist p4-prepare-changelist p4-post-changelist p4-pre-submit post-index-change"

# Results go to RESULTS_DIR (default: results/<os>-git<version>).
OS_TAG="$(uname -s | tr '[:upper:]' '[:lower:]')-$(uname -m)"
# REF_FORMAT=reftable (Git >= 2.45) creates the repos under test with reftable.
RESULTS_DIR="${RESULTS_DIR:-$SPIKE_ROOT/results/$OS_TAG-git$GIT_VERSION${REF_FORMAT:+-$REF_FORMAT}}"
mkdir -p "$RESULTS_DIR"

new_sandbox() {
  SANDBOX="$(mktemp -d "${TMPDIR:-/tmp}/spike-grd-001.XXXXXX")"
  SANDBOX="$(cd "$SANDBOX" && pwd -P)"
  export HOME="$SANDBOX/home"   # isolates ~/.gitconfig, husky, pre-commit caches
  export XDG_CONFIG_HOME="$HOME/.config"
  export PRE_COMMIT_HOME="$HOME/.cache/pre-commit"
  mkdir -p "$HOME"
  git config --global init.defaultBranch main
  git config --global advice.detachedHead false
  git config --global protocol.file.allow always
  export PROBE_LOG="$SANDBOX/probe.log"
  : > "$PROBE_LOG"
}

sandbox_guard() {
  case "$(pwd -P)" in
    "$SANDBOX"|"$SANDBOX"/*) ;;
    *) echo "FATAL: refusing to run outside the sandbox: $(pwd -P)" >&2; exit 99 ;;
  esac
}

drop_sandbox() {
  [ -n "${KEEP_SANDBOX:-}" ] && { echo "sandbox kept: $SANDBOX" >&2; return; }
  case "$SANDBOX" in */spike-grd-001.*) rm -rf "$SANDBOX" ;; esac
}

# init_repo <path>: repo with one commit on main (file a.txt).
init_repo() {
  git init -q ${REF_FORMAT:+--ref-format=$REF_FORMAT} "$1"
  ( cd "$1" && sandbox_guard && echo one > a.txt && git add a.txt && git -c core.hooksPath=/dev/null commit -qm one )
}

# init_remote <work> <bare>: bare remote with main pushed, origin configured.
init_remote() {
  git init -q --bare "$2"
  ( cd "$1" && git remote add origin "$2" && git -c core.hooksPath=/dev/null push -q origin main 2>/dev/null )
}

common_dir() { (cd "$1" && cd "$(git rev-parse --git-common-dir)" && pwd -P); }

# install_probes <repo> [deny-spec]: logging dispatchers for every hook name,
# activated with an absolute core.hooksPath in the common config (ADR-GRD-001 § 1).
install_probes() {
  local repo="$1" cdir hooks
  cdir="$(common_dir "$repo")"
  hooks="$cdir/gitraptor/hooks"
  mkdir -p "$hooks"
  for h in $HOOK_NAMES; do
    sed "s#@LIB@#$LIB#g" "$LIB/probe-hook.sh" > "$hooks/$h"
    chmod 0700 "$hooks/$h"
  done
  chmod 0700 "$cdir/gitraptor" "$hooks"
  git config --file "$cdir/config" core.hooksPath "$hooks"
  export PROBE_COMMON="$cdir"
}

# Hooks that change Git's behaviour just by existing (ADR-GRD-001 § 2): only
# installed when the previous hooks directory already had them.
SKIP_UNLESS_PREV="push-to-checkout proc-receive post-index-change"

# install_guard <repo> [protected-branch]: prototype of the Guardrails
# decision for US-GRD-001 (force-push and base-branch deletion), in sh,
# constants only. Chains the previous hook: the local core.hooksPath value as
# is (relative = relative to the hook's cwd, like Git) or <common>/hooks.
# GUARD_PRUNE=0 uses the naive deletion rule (no pack-refs prune exception).
install_guard() {
  local repo="$1" base="${2:-main}" prune="${GUARD_PRUNE:-1}" cdir hooks prev h
  cdir="$(common_dir "$repo")"
  hooks="$cdir/gitraptor/hooks"
  prev="$(git config --file "$cdir/config" core.hooksPath || true)"
  mkdir -p "$hooks"
  printf '%s' "$prev" > "$cdir/gitraptor/prev-hookspath"
  for h in $HOOK_NAMES; do
    case " $SKIP_UNLESS_PREV " in *" $h "*) [ -x "${prev:-$cdir/hooks}/$h" ] || continue ;; esac
    sed -e "s#@HOOK@#$h#g" -e "s#@BASE@#$base#g" -e "s#@PREV@#${prev:-$cdir/hooks}#g" \
      -e "s#@COMMON@#$cdir#g" -e "s#@PRUNE@#$prune#g" \
      "$LIB/guard-hook.sh" > "$hooks/$h"
    chmod 0700 "$hooks/$h"
  done
  chmod 0700 "$cdir/gitraptor" "$hooks"
  git config --file "$cdir/config" core.hooksPath "$hooks"
}

# uninstall_guard <repo>: restore the previous local value (or remove the key)
# only if the key is still ours; then remove the folder (ADR-GRD-001 § 4).
uninstall_guard() {
  local cdir prev cur
  cdir="$(common_dir "$1")"
  prev="$(cat "$cdir/gitraptor/prev-hookspath")"
  cur="$(git config --file "$cdir/config" core.hooksPath || true)"
  if [ "$cur" = "$cdir/gitraptor/hooks" ]; then
    if [ -n "$prev" ]; then git config --file "$cdir/config" core.hooksPath "$prev"
    else git config --file "$cdir/config" --unset core.hooksPath; fi
  fi
  case "$cdir" in "$SANDBOX"/*) rm -r "$cdir/gitraptor" ;; esac
}

# Fingerprint of refs / index / worktree / in-progress state (no git calls).
# shellcheck source=fp.sh
. "$LIB/fp.sh"

# TSV writer
tsv() { local IFS=$'\t'; printf '%s\n' "$*"; }

# hooks_fired: ordered, de-duplicated hook sequence from the probe log.
hooks_fired() { awk -F'\t' '{print $2 ($3!="" && $2=="reference-transaction" ? "("$3")" : "")}' "$PROBE_LOG" | paste -sd' ' -; }
