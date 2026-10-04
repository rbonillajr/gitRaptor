# shellcheck shell=sh
# Fingerprints read straight from disk (refs, worktree) or with a hook-free
# read-only plumbing command (index). A probe must not refresh the index. Inputs (environment):
#   PROBE_COMMON  git common dir      PROBE_GITDIR  per-worktree git dir
#   PROBE_WT      worktree root       PROBE_EXTRA   space-separated paths whose existence matters

_h() { shasum | cut -c1-10; }

# Local refs that matter for "before any effect": everything except
# remote-tracking refs (a fetch inside `pull` updates them on purpose) and
# pseudo-refs (ORIG_HEAD, FETCH_HEAD, AUTO_MERGE live outside refs/).
fp_refs() {
  (
    cd "$PROBE_COMMON" || exit 0
    find refs -type f ! -name '*.lock' ! -path 'refs/remotes/*' 2>/dev/null | LC_ALL=C sort | while read -r f; do printf '%s %s\n' "$f" "$(cat "$f")"; done
    grep -v ' refs/remotes/' packed-refs 2>/dev/null
    cat HEAD 2>/dev/null
    for d in worktrees/*; do [ -f "$d/HEAD" ] && cat "$d/HEAD"; done
    [ -d reftable ] && cat reftable/* 2>/dev/null
  ) | _h
}

# Index content (mode, oid, stage, path) of the real index, ignoring stat-only
# refreshes and Git's temporary index (GIT_INDEX_FILE). ls-files runs no hooks.
fp_idx() { env -u GIT_INDEX_FILE git --git-dir="$PROBE_GITDIR" ls-files -s 2>/dev/null | _h; }

fp_wt() {
  (
    cd "$PROBE_WT" 2>/dev/null || { echo absent; exit 0; }
    find . -name .git -prune -o -type f -print | LC_ALL=C sort | while read -r f; do
      printf '%s %s\n' "$f" "$(shasum < "$f")"
    done
  ) | _h
}

# In-progress operation state and worktree administration.
fp_state() {
  (
    for f in MERGE_HEAD rebase-merge rebase-apply CHERRY_PICK_HEAD; do
      [ -e "$PROBE_GITDIR/$f" ] && echo "$f"
    done
    ls "$PROBE_COMMON/worktrees" 2>/dev/null
    for p in ${PROBE_EXTRA:-}; do [ -e "$p" ] && echo "exists:$p"; done
    :
  ) | _h
}

fp_all() { printf '%s\t%s\t%s\t%s' "$(fp_refs)" "$(fp_idx)" "$(fp_wt)" "$(fp_state)"; }
