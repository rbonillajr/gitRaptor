#!/usr/bin/env bash
# Suite 01 — Operation matrix (ADR-GRD-002 § 1).
# For each operation of BR-VAL-002 (plus variants), run it twice in a fresh
# temporary repo with probe dispatchers on every hook name:
#   1. observe: which hooks fire, in which order, and what had ALREADY changed
#      (r=refs, i=index, w=worktree, s=in-progress/worktree-admin state)
#      relative to the state before the operation, at the moment each hook ran;
#   2. deny: the governing hook exits 1; residual effects after the operation.
# A cell is "A" only if the denying hook ran with nothing changed AND the
# denied operation left no residual effect.
. "$(dirname "$0")/../lib/common.sh"

OUT="$RESULTS_DIR/01-matrix.tsv"
tsv case op hooks_observed deny_hook deny_exit residual moment > "$OUT"

# --- setups: each runs inside $SANDBOX, defines WT (worktree under test) -----
s_repo()      { init_repo "$SANDBOX/w"; WT="$SANDBOX/w"; }
s_remote()    { s_repo; init_remote "$WT" "$SANDBOX/r.git"; }
g()           { git -c core.hooksPath=/dev/null "$@"; }
s_staged()    { s_repo; (cd "$WT" && echo two >> a.txt && git add a.txt); }
s_dirty()     { s_repo; (cd "$WT" && echo two >> a.txt); }
s_ahead()     { s_remote; (cd "$WT" && echo two >> a.txt && g commit -qam two); }
s_amended()   { s_remote; (cd "$WT" && echo amend >> a.txt && g commit -q --amend -am amended); }
s_rbranch()   { s_remote; (cd "$WT" && g branch feat && g push -q origin feat); }
s_mirror()    { s_rbranch; (cd "$WT" && g branch -D feat >/dev/null); }
s_branch()    { s_repo; (cd "$WT" && g branch feat); }
s_branch_pk() { s_branch; (cd "$WT" && g pack-refs --all); }
s_two_dirty() { s_repo; (cd "$WT" && echo two >> a.txt && g commit -qam two && echo dirty >> a.txt); }
s_diverged()  { s_repo; (cd "$WT" && g switch -qc feat && echo f > f.txt && g add f.txt && g commit -qm f \
                  && g switch -q main && echo m > m.txt && g add m.txt && g commit -qm m); }
s_on_feat()   { s_diverged; (cd "$WT" && g switch -q feat); }
s_feat_ahead(){ s_repo; (cd "$WT" && g switch -qc feat && echo f > f.txt && g add f.txt && g commit -qm f && g switch -q main); }
s_pull_div()  { s_remote; g clone -q "$SANDBOX/r.git" "$SANDBOX/o"; (cd "$SANDBOX/o" && echo o > o.txt && g add o.txt && g commit -qm o && g push -q origin main);
                (cd "$WT" && echo l > l.txt && g add l.txt && g commit -qm l); }
s_wt_exists() { s_repo; (cd "$WT" && g worktree add -q ../wt2 -b feat2 2>/dev/null); export PROBE_EXTRA="$SANDBOX/wt2"; }
s_main_back() { s_repo; (cd "$WT" && echo two >> a.txt && g commit -qam two && g switch -qc feat HEAD~1); }
s_wt_target() { s_branch; export PROBE_EXTRA="$SANDBOX/wt2"; }

# Remote refs (R in residual): a denied push must not change the remote.
remote_fp() { [ -d "$SANDBOX/r.git" ] && git --git-dir="$SANDBOX/r.git" for-each-ref | shasum | cut -c1-10; }

# run_case <id> <setup> <op> <deny-spec> [deny-match]
run_case() {
  local id="$1" setup="$2" op="$3" deny="$4" match="${5:-}"
  local observed deny_exit residual moment base after
  # 1. observe
  new_sandbox; unset PROBE_EXTRA; $setup
  install_probes "$WT"
  export PROBE_WT="$WT" PROBE_GITDIR; PROBE_GITDIR="$(cd "$WT" && cd "$(git rev-parse --git-dir)" && pwd -P)"
  base="$(fp_all)"
  : > "$PROBE_LOG"
  (cd "$WT" && sandbox_guard && eval "$op") >/dev/null 2>&1
  observed="$(awk -F'\t' -v base="$base" '
    BEGIN { split(base, b, "\t") }
    { d=""; if ($8!=b[1]) d=d"r"; if ($9!=b[2]) d=d"i"; if ($10!=b[3]) d=d"w"; if ($11!=b[4]) d=d"s"
      n=$2; if ($2=="reference-transaction") n="ref-tx:"$3
      printf "%s[%s] ", n, (d==""?"A":d) }' "$PROBE_LOG")"
  mkdir -p "$RESULTS_DIR/01-matrix-detail"
  { echo "# op: $op"; echo "# base: $base"; cut -f1-3,5-11 "$PROBE_LOG" | sed "s#$SANDBOX#\$SANDBOX#g"; } > "$RESULTS_DIR/01-matrix-detail/$id.tsv"
  drop_sandbox
  # 2. deny
  if [ "$deny" = "-" ]; then deny_exit="-"; residual="-"; moment="C (no governing hook)"
  else
    new_sandbox; unset PROBE_EXTRA; $setup
    install_probes "$WT"
    export PROBE_WT="$WT" PROBE_GITDIR; PROBE_GITDIR="$(cd "$WT" && cd "$(git rev-parse --git-dir)" && pwd -P)"
    base="$(fp_all)"; rbase="$(remote_fp)"
    : > "$PROBE_LOG"
    export PROBE_DENY="$deny" PROBE_DENY_MATCH="$match"
    (cd "$WT" && sandbox_guard && eval "$op") >/dev/null 2>&1; deny_exit=$?
    unset PROBE_DENY PROBE_DENY_MATCH
    after="$(fp_all)"; rafter="$(remote_fp)"
    if ! cut -f2 "$PROBE_LOG" | grep -qx "${deny%%:*}"; then residual="hook-not-run"; moment="C (deny hook not run)"
    else
      residual="$(paste <(echo "$base" | tr '\t' '\n') <(echo "$after" | tr '\t' '\n') <(printf 'r\ni\nw\ns\n') \
        | awk -F'\t' '$1!=$2 {printf "%s", $3}')"
      [ "$rbase" != "$rafter" ] && residual="${residual}R"
      if [ -z "$residual" ] && [ "$deny_exit" != 0 ]; then moment="A"; residual="none"
      elif [ "$deny_exit" != 0 ]; then moment="B (partial effects: $residual)"
      else moment="not-impeded (exit 0)"; fi
    fi
    drop_sandbox
  fi
  tsv "$id" "$op" "$observed" "$deny${match:+ ~$match}" "$deny_exit" "$residual" "$moment" >> "$OUT"
  printf '%-22s %s\n' "$id" "$moment" >&2
}

Z="$ZERO_OID"
run_case commit              s_staged    'git commit -qm two'                      pre-commit
run_case commit-no-verify    s_staged    'git commit -q --no-verify -m two'        reference-transaction:prepared refs/heads/main
run_case commit-a-rejected   s_dirty     'git commit -qam two'                     pre-commit
run_case commit-msg          s_staged    'git commit -qm two'                      commit-msg
run_case push                s_ahead     'git push -q origin main'                 pre-push
run_case push-no-verify      s_ahead     'git push -q --no-verify origin main'     pre-push
run_case force-push          s_amended   'git push -qf origin main'                pre-push
run_case force-push-plus     s_amended   'git push -q origin +main'                pre-push
run_case force-with-lease    s_amended   'git push -q --force-with-lease origin main' pre-push
run_case force-push-no-verify s_amended  'git push -qf --no-verify origin main'    pre-push
run_case send-pack-force     s_amended   'git send-pack --force ../r.git main'     pre-push
run_case push-mirror         s_mirror    'git push -q --mirror origin'             pre-push
run_case delete-remote       s_rbranch   'git push -q origin --delete feat'        pre-push
run_case delete-remote-colon s_rbranch   'git push -q origin :feat'                pre-push
run_case delete-local        s_branch    'git branch -D feat'                      reference-transaction:prepared refs/heads/feat
run_case delete-local-packed s_branch_pk 'git branch -D feat'                      reference-transaction:prepared refs/heads/feat
run_case update-ref-d        s_branch    'git update-ref -d refs/heads/feat'       reference-transaction:prepared refs/heads/feat
run_case update-ref-d-nodrf  s_branch    'git update-ref --no-deref -d refs/heads/feat' reference-transaction:prepared refs/heads/feat
run_case rename-branch       s_branch    'git branch -m feat feat2'                reference-transaction:prepared refs/heads/feat
run_case overwrite-branch    s_main_back 'git branch -f main feat' reference-transaction:prepared refs/heads/main
run_case reset-hard          s_two_dirty 'git reset -q --hard HEAD~1'              reference-transaction:prepared refs/heads/main
run_case reset-hard-head     s_dirty     'git reset -q --hard'                     -
run_case rebase              s_on_feat   'git rebase -q main'                      pre-rebase
run_case rebase-no-verify    s_on_feat   'git rebase -q --no-verify main'          pre-rebase
run_case rebase-reftx        s_on_feat   'git rebase -q --no-verify main'          reference-transaction:prepared refs/heads/feat
run_case pull-rebase         s_pull_div  'git pull -q --rebase origin main'        pre-rebase
run_case merge               s_diverged  'git merge -q --no-edit feat'             pre-merge-commit
run_case merge-reftx         s_diverged  'git merge -q --no-edit feat'             reference-transaction:prepared refs/heads/main
run_case merge-ff            s_feat_ahead 'git merge -q --ff-only feat'            reference-transaction:prepared refs/heads/main
run_case merge-no-commit     s_diverged  'git merge -q --no-commit --no-ff feat'   -
run_case worktree-add-b      s_wt_target 'git worktree add -q -b nb ../wt2'        reference-transaction:prepared refs/heads/nb
run_case worktree-add-exist  s_wt_target 'git worktree add -q ../wt2 feat'         reference-transaction:prepared
run_case worktree-add-postco s_wt_target 'git worktree add -q ../wt2 feat'         post-checkout
run_case worktree-add-detach s_wt_target 'git worktree add -q --detach ../wt2'     reference-transaction:prepared
run_case worktree-remove     s_wt_exists 'git worktree remove ../wt2'              reference-transaction:prepared
run_case switch-branch       s_branch    'git switch -q feat'                      reference-transaction:prepared
run_case tag                 s_repo      'git tag v1'                              reference-transaction:prepared refs/tags/v1
run_case pack-refs           s_branch    'git pack-refs --all'                     reference-transaction:prepared refs/heads/main
run_case gc                  s_branch    'git gc -q'                               reference-transaction:prepared refs/heads/main
run_case commit-tree-plumb   s_repo      'git update-ref refs/heads/x $(git commit-tree -m x HEAD^{tree})' reference-transaction:prepared refs/heads/x

echo "wrote $OUT" >&2
