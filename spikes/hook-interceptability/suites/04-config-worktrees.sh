#!/usr/bin/env bash
# Suite 04 — Activation key, worktree coverage and crossed repos
# (ADR-GRD-001 § 1, § 4, § 5; ADR-GRD-002 § 2 M-02).
#   C*  byte-for-byte config after install + uninstall
#   W*  effective core.hooksPath per worktree and protection with worktree
#       config, include, includeIf (gitdir:, onbranch:)
#   X*  GIT_DIR / GIT_WORK_TREE pointing to another repo: which dispatcher
#       runs and with which cwd
. "$(dirname "$0")/../lib/common.sh"

OUT="$RESULTS_DIR/04-config-worktrees.tsv"
tsv id observation > "$OUT"
rec() { local o; o="$(printf '%s' "$2" | sed -e "s#$SANDBOX#\$SANDBOX#g" -e 's/\t/\\t/g' | tr '\n' ' ')"; tsv "$1" "$o" >> "$OUT"; printf '%-34s %s\n' "$1" "$o" >&2; }
g() { git -c core.hooksPath=/dev/null "$@"; }
effective() { git -C "$1" config --show-scope --show-origin --get core.hooksPath 2>/dev/null | tr '\t' ' ' || echo "<unset>"; }
del_main() { (cd "$1" && git branch -qD main >/dev/null 2>&1); [ $? -ne 0 ] && echo denied || echo DELETED; }

# --- C: byte-identical config ---------------------------------------------------
bytes_case() {
  local id="$1" prep="$2" before after
  new_sandbox; init_repo "$SANDBOX/w"; WT="$SANDBOX/w"
  (cd "$WT" && eval "$prep")
  cp "$WT/.git/config" "$SANDBOX/config.before"
  install_guard "$WT"; uninstall_guard "$WT"
  if cmp -s "$SANDBOX/config.before" "$WT/.git/config"; then rec "$id" "byte-identical"
  else rec "$id" "DIFFERS: $(diff "$SANDBOX/config.before" "$WT/.git/config" | grep '^[<>]' | paste -sd' ' -)"; fi
  drop_sandbox
}
bytes_case C01-no-previous-value   ':'
bytes_case C02-previous-local      'g config core.hooksPath .husky/_'
bytes_case C03-previous-handwritten "awk '{print} /^\\[core\\]/{print \"\\thookspath=.husky/_   # husky\"}' .git/config > c.t && mv c.t .git/config"
bytes_case C04-previous-global-only 'git config --global core.hooksPath /tmp/global-hooks'
bytes_case C05-second-core-section "printf '[core]\n\thooksPath = .husky/_\n' >> .git/config"
bytes_case C06-no-trailing-newline "printf '%s' \"\$(cat .git/config)\" > .git/config.t && mv .git/config.t .git/config"

# --- W: worktree coverage -----------------------------------------------------------
new_sandbox; init_repo "$SANDBOX/w"; WT="$SANDBOX/w"
install_guard "$WT"
(cd "$WT" && g switch -qc feat && g worktree add -q --detach ../after 2>/dev/null)
rec W01-worktree-created-after-install "effective=[$(effective "$SANDBOX/after")]; branch -D main from it: $(del_main "$SANDBOX/after")"
(cd "$SANDBOX/after" && mkdir -p sub && cd sub && git branch -qD main >/dev/null 2>&1); rec W02-from-subdirectory "exit=$? (non-zero = denied)"
drop_sandbox

wt_case() {
  local id="$1" prep="$2" from="$3"
  new_sandbox; init_repo "$SANDBOX/w"; WT="$SANDBOX/w"
  (cd "$WT" && g switch -qc feat && g worktree add -q --detach ../wt2 2>/dev/null)
  install_guard "$WT"
  (cd "$WT" && eval "$prep")
  rec "$id" "main-wt=[$(effective "$WT")] linked=[$(effective "$SANDBOX/wt2")]; branch -D main from $from: $(del_main "$SANDBOX/$from")"
  drop_sandbox
}
wt_case W03-worktreeconfig-linked  'g config extensions.worktreeConfig true && git -C ../wt2 config --worktree core.hooksPath /dev/null' wt2
wt_case W04-worktreeconfig-main    'g config extensions.worktreeConfig true && g config --worktree core.hooksPath /dev/null' w
wt_case W05-include-after-key      'printf "[core]\n\thooksPath = /dev/null\n" > .git/extra.inc && g config include.path extra.inc' w
wt_case W06-includeif-onbranch     'printf "[core]\n\thooksPath = /dev/null\n" > .git/extra.inc && g config includeIf.onbranch:feat.path extra.inc' w
wt_case W07-includeif-gitdir-linked 'printf "[core]\n\thooksPath = /dev/null\n" > .git/extra.inc && g config "includeIf.gitdir:$SANDBOX/w/.git/worktrees/**.path" extra.inc' wt2
wt_case W08-global-key-only        'git config --global core.hooksPath /dev/null' wt2
wt_case W09-relative-key           'g config core.hooksPath .git/gitraptor/hooks' wt2

# --- X: crossed GIT_DIR / GIT_WORK_TREE (M-02) ----------------------------------------
new_sandbox
init_repo "$SANDBOX/a"; init_repo "$SANDBOX/b"
(cd "$SANDBOX/a" && g switch -qc feat)
install_probes "$SANDBOX/a"; export PROBE_WT="$SANDBOX/a" PROBE_GITDIR="$SANDBOX/a/.git"
: > "$PROBE_LOG"
(cd "$SANDBOX/b" && GIT_DIR="$SANDBOX/a/.git" git branch -qD main >/dev/null 2>&1)
rec X01-GIT_DIR-to-protected-from-other-cwd "hooks run from: $(awk -F'\t' '$2=="reference-transaction"&&$3=="prepared"{print "cwd="$6" GIT_DIR="$7}' "$PROBE_LOG" | sort -u | paste -sd';' -)"
: > "$PROBE_LOG"
(cd "$SANDBOX/b" && GIT_DIR="$SANDBOX/a/.git" GIT_WORK_TREE="$SANDBOX/b" git commit -q --allow-empty -m x >/dev/null 2>&1)
rec X02-GIT_DIR+WORK_TREE-crossed "hooks run from: $(awk -F'\t' '$2=="pre-commit"{print "cwd="$6" GIT_DIR="$7}' "$PROBE_LOG" | paste -sd';' -)"
: > "$PROBE_LOG"
(cd "$SANDBOX/a" && GIT_DIR="$SANDBOX/b/.git" git branch -q zz >/dev/null 2>&1)
rec X03-GIT_DIR-to-unprotected-from-protected-cwd "dispatchers of a ran: $(wc -l < "$PROBE_LOG" | tr -d ' ') times (b's config decides)"
drop_sandbox
echo "wrote $OUT" >&2
