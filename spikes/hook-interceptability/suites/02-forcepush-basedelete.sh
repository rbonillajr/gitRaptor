#!/usr/bin/env bash
# Suite 02 — Force-push and deletion of the base branch (US-GRD-001,
# ADR-GRD-002 § 1-2 and Validation 3-5), with the guard prototype
# (lib/guard-hook.sh) protecting `main`.
#
# expect: deny   = must fail and leave local and remote `main` untouched
#         allow  = must succeed (false-positive check)
#         bypass = declared voluntary skip: recorded as observed, not judged
#         probe  = behaviour of Git itself, recorded without a verdict
. "$(dirname "$0")/../lib/common.sh"

OUT="$RESULTS_DIR/02-forcepush-basedelete.tsv"
tsv id expect op exit local_main remote_main guard_log extra verdict > "$OUT"

g() { git -c core.hooksPath=/dev/null "$@"; }
# oid <git-dir> [branch]: value of the protected branch (default main)
oid() { g --git-dir="$1" rev-parse -q --verify "refs/heads/${2:-main}" 2>/dev/null || echo absent; }

# --- setups -----------------------------------------------------------------
s_base()      { init_repo "$SANDBOX/w"; init_remote "$SANDBOX/w" "$SANDBOX/r.git"; WT="$SANDBOX/w"; RUN="$WT"; }
s_amended()   { s_base; (cd "$WT" && echo amend >> a.txt && g commit -q --amend -am amended); }
s_ahead()     { s_base; (cd "$WT" && echo two >> a.txt && g commit -qam two); }
s_feat()      { s_base; (cd "$WT" && g switch -qc feat); }
s_feat_rforce(){ s_base; (cd "$WT" && g switch -qc feat && g push -q origin feat && echo x >> a.txt && g commit -q --amend -am x); }
s_missing()   { s_base; g clone -q "$SANDBOX/r.git" "$SANDBOX/o"; (cd "$SANDBOX/o" && echo o >> a.txt && g commit -qam o && g push -q origin main);
                (cd "$WT" && echo l >> a.txt && g commit -qam l); }
s_shallow_ff(){ s_ahead; (cd "$WT" && g push -q origin main && echo t >> a.txt && g commit -qam three); g clone -q --depth 1 "file://$SANDBOX/r.git" "$SANDBOX/s";
                (cd "$SANDBOX/s" && echo s >> a.txt && g commit -qam s); WT="$SANDBOX/s"; RUN="$WT"; }
s_graft()     { s_ahead; (cd "$WT" && g push -q origin main && g switch -q --orphan evil && echo evil > e.txt && g add e.txt && g commit -qm evil \
                  && g replace --graft evil "$(g rev-parse main)"); }
s_linked()    { s_amended; (cd "$WT" && g switch -qc feat && g worktree add -q ../wt2 main 2>/dev/null); RUN="$SANDBOX/wt2"; }
s_on_feat()   { s_base; (cd "$WT" && g switch -qc feat); }
s_packed()    { s_on_feat; (cd "$WT" && g pack-refs --all); }
s_loosepk()   { s_on_feat; (cd "$WT" && g pack-refs --all && g update-ref refs/heads/main "$(g rev-parse main)"); }
s_linked_del(){ s_base; (cd "$WT" && g switch -qc feat && g worktree add -q --detach ../wt2 2>/dev/null); RUN="$SANDBOX/wt2"; }
s_head_main() { s_base; }
s_wtconfig()  { s_linked_del; (cd "$WT" && g config extensions.worktreeConfig true && cd ../wt2 && g config --worktree core.hooksPath /dev/null); }
s_nfc()       { init_repo "$SANDBOX/w"; WT="$SANDBOX/w"; RUN="$WT"; (cd "$WT" && g branch "$(printf 'caf\303\251')" && g switch -qc feat); }

# scenario <id> <expect> <setup> <op> [base-branch]
# EXTRA=<cmd> records one more observation (run in the main worktree).
scenario() {
  local id="$1" expect="$2" setup="$3" op="$4" base="${5:-main}"
  local l0 l1 r0 r1 rc verdict glog
  new_sandbox; $setup
  [ -n "${NO_GUARD:-}" ] || install_guard "$WT" "$base"
  [ -n "${POST_INSTALL:-}" ] && (cd "$WT" && eval "$POST_INSTALL")
  export GUARD_LOG="$SANDBOX/guard.log"; : > "$GUARD_LOG"
  l0="$(oid "$(common_dir "$WT")" "$base")"; r0="$(oid "$SANDBOX/r.git" "$base" 2>/dev/null)"
  (cd "$RUN" && sandbox_guard && eval "$op") >"$SANDBOX/out" 2>&1; rc=$?
  l1="$(oid "$(common_dir "$WT")" "$base")"; r1="$(oid "$SANDBOX/r.git" "$base" 2>/dev/null)"
  local lm rm; [ "$l0" = "$l1" ] && lm=intact || lm="changed"
  [ "$r0" = "$r1" ] && rm=intact || rm="changed"
  [ -d "$SANDBOX/r.git" ] || rm="-"
  glog="$(cut -f2,3 "$GUARD_LOG" | tr '\t' ' ' | paste -sd';' -)"
  local extra="-"; [ -n "${EXTRA:-}" ] && extra="$(cd "$WT" && eval "$EXTRA" 2>&1 | paste -sd' ' -)"
  case "$expect" in
    deny)  { [ $rc -ne 0 ] && [ "$lm" = intact ] && [ "$rm" != changed ]; } && verdict=PASS || verdict=FAIL ;;
    allow) [ $rc -eq 0 ] && verdict=PASS || verdict=FAIL ;;
    *)     verdict="observed: exit=$rc local=$lm remote=$rm" ;;
  esac
  tsv "$id" "$expect" "$op" "$rc" "$lm" "$rm" "${glog:--}" "$extra" "$verdict" >> "$OUT"
  printf '%-28s %-7s %s\n' "$id" "$expect" "$verdict" >&2
  unset GUARD_LOG
  drop_sandbox
}

# --- force-push (remote main) -------------------------------------------------
scenario F01-push-f                deny  s_amended  'git push -qf origin main'
scenario F02-push-plus             deny  s_amended  'git push -q origin +main'
scenario F03-force-with-lease      deny  s_amended  'git push -q --force-with-lease origin main'
scenario F04-push-ff               allow s_ahead    'git push -q origin main'
scenario F05-force-own-branch      allow s_feat_rforce 'git push -qf origin feat'
scenario F06-remote-tip-missing    deny  s_missing  'git push -qf origin main'
scenario F07-shallow-ff            probe s_shallow_ff 'git push -q origin main'
scenario F08-replace-graft         deny  s_graft    'git push -q origin evil:main'
scenario F08b-replace-graft-noguard probe s_graft   'git -c core.hooksPath=/dev/null push -q origin evil:main'
scenario F09-no-verify             bypass s_amended 'git push -qf --no-verify origin main'
scenario F10-from-linked-worktree  deny  s_linked   'git push -qf origin main'
scenario F11-case-alias-Main       deny  s_amended  'git push -qf origin main:refs/heads/Main'
scenario F11b-case-alias-noguard   probe s_amended  'git -c core.hooksPath=/dev/null push -qf origin main:refs/heads/Main'
scenario F12-mirror                deny  s_amended  'git push -q --mirror origin'
scenario F13-wildcard-refspec      deny  s_amended  "git push -q origin '+refs/heads/*:refs/heads/*'"
scenario F14-default-upstream      deny  s_amended  'git branch -q -u origin/main && git push -qf'
scenario F15-send-pack             bypass s_amended 'git send-pack --force ../r.git main'
scenario F16-hookspath-override    bypass s_amended 'git -c core.hooksPath=/dev/null push -qf origin main'
scenario F17-git-config-env        bypass s_amended 'GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=core.hooksPath GIT_CONFIG_VALUE_0=/dev/null git push -qf origin main'

# --- deletion of the base branch -----------------------------------------------
scenario D01-branch-D              deny  s_on_feat  'git branch -qD main'
scenario D02-update-ref-d          deny  s_on_feat  'git update-ref -d refs/heads/main'
scenario D03-push-colon            deny  s_base     'git push -q origin :main'
scenario D04-push-delete           deny  s_base     'git push -q origin --delete main'
scenario D05-from-linked-worktree  deny  s_linked_del 'git branch -qD main'
scenario D06-packed-only           deny  s_packed   'git branch -qD main'
scenario D07-loose-and-packed      deny  s_loosepk  'git branch -qD main'
scenario D08-case-alias-Main       deny  s_on_feat  'git branch -qD Main'
scenario D08b-case-alias-noguard   probe s_on_feat  'git -c core.hooksPath=/dev/null branch -qD Main'
scenario D09-update-ref-d-HEAD     deny  s_head_main 'git update-ref -d HEAD'
scenario D10-rename-away           deny  s_on_feat  'git branch -qm main other'
EXTRA='echo branches: $(git -c core.hooksPath=/dev/null branch --format="%(refname:short)")' \
scenario D11-rename-onto-base      probe s_on_feat  'git commit -q --allow-empty -m f && git branch -qM feat main'
scenario D12-pack-refs             allow s_on_feat  'git pack-refs --all'
scenario D13-gc                    allow s_on_feat  'git gc -q'
GUARD_PRUNE=0 scenario D14-pack-refs-naive-rule probe s_on_feat 'git pack-refs --all'
scenario D15-hookspath-override    bypass s_on_feat 'git -c core.hooksPath=/dev/null branch -qD main'
scenario D16-git-config-env        bypass s_on_feat 'GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=core.hooksPath GIT_CONFIG_VALUE_0=/dev/null git branch -qD main'
POST_INSTALL='git config core.hooksPath .git/gitraptor/hooks' \
scenario D17-relative-hookspath-linked probe s_linked_del 'git branch -qD main'
POST_INSTALL='git config core.hooksPath .git/gitraptor/hooks' \
scenario D17b-relative-hookspath-main  deny  s_on_feat 'git branch -qD main'
scenario D18-worktreeconfig-override  probe s_wtconfig  'git branch -qD main'
scenario D19-nfd-alias-of-nfc-base deny  s_nfc      "git branch -qD \"\$(printf 'cafe\\314\\201')\"" "$(printf 'caf\303\251')"
POST_INSTALL='git config core.precomposeUnicode false' \
scenario D19b-nfd-alias-no-precompose probe s_nfc   "git branch -qD \"\$(printf 'cafe\\314\\201')\"" "$(printf 'caf\303\251')"
scenario D21-case-alias-overwrite  deny  s_on_feat  'git commit -q --allow-empty -m f && git branch -f Main feat'
scenario D21b-case-alias-overwrite-noguard probe s_on_feat 'git commit -q --allow-empty -m f && git -c core.hooksPath=/dev/null branch -f Main feat'
scenario D20-hand-delete-loose-ref bypass s_on_feat 'rm .git/refs/heads/main'

echo "wrote $OUT" >&2
