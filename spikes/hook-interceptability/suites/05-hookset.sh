#!/usr/bin/env bash
# Suite 05 — Dispatcher set (ADR-GRD-001 § 2): which hooks change Git's
# behaviour merely by existing, and which ones fire on everyday commands.
#   H01/H02  push-to-checkout on a non-bare repo with updateInstead
#   H03/H04  proc-receive with receive.procReceiveRefs
#   H05      post-index-change firing on read-mostly commands (status, diff)
#   H06      spawns per everyday command with the full dispatcher set
. "$(dirname "$0")/../lib/common.sh"

OUT="$RESULTS_DIR/05-hookset.tsv"
tsv id observation > "$OUT"
rec() { local o; o="$(printf '%s' "$2" | sed "s#$SANDBOX#\$SANDBOX#g" | tr '\n' ' ')"; tsv "$1" "$o" >> "$OUT"; printf '%-34s %s\n' "$1" "$o" >&2; }
g() { git -c core.hooksPath=/dev/null "$@"; }

# noop_hooks <repo> <names...>: hooks dir with `exit 0` dispatchers (what a
# Guardrails dispatcher does when there is no previous hook to chain).
noop_hooks() {
  local repo="$1" d; shift
  d="$(common_dir "$repo")/gitraptor/hooks"; mkdir -p "$d"
  for h in "$@"; do printf '#!/bin/sh\nexit 0\n' > "$d/$h"; chmod +x "$d/$h"; done
  git -C "$repo" config core.hooksPath "$d"
}

push_to_checkout() {
  local id="$1" with="$2"
  new_sandbox
  init_repo "$SANDBOX/nb"; (cd "$SANDBOX/nb" && g config receive.denyCurrentBranch updateInstead)
  g clone -q "$SANDBOX/nb" "$SANDBOX/c"
  (cd "$SANDBOX/c" && echo new > new.txt && g add new.txt && g commit -qm new)
  [ "$with" = yes ] && noop_hooks "$SANDBOX/nb" push-to-checkout
  (cd "$SANDBOX/c" && g push -q origin main >/dev/null 2>&1); local rc=$?
  rec "$id" "push exit=$rc; remote ref moved: $( [ "$(g -C "$SANDBOX/nb" rev-parse main)" = "$(g -C "$SANDBOX/c" rev-parse main)" ] && echo yes || echo no); remote worktree updated: $( [ -f "$SANDBOX/nb/new.txt" ] && echo yes || echo NO); remote status: $(g -C "$SANDBOX/nb" status --porcelain | paste -sd, -)"
  drop_sandbox
}
push_to_checkout H01-push-to-checkout-absent  no
push_to_checkout H02-push-to-checkout-noop    yes

proc_receive() {
  local id="$1" with="$2"
  new_sandbox
  init_repo "$SANDBOX/w"; git init -q --bare "$SANDBOX/r.git"; (cd "$SANDBOX/w" && g push -q "$SANDBOX/r.git" main)
  g -C "$SANDBOX/r.git" config receive.procReceiveRefs refs/for
  [ "$with" = yes ] && noop_hooks "$SANDBOX/r.git" proc-receive
  local out; out="$(cd "$SANDBOX/w" && g push "$SANDBOX/r.git" HEAD:refs/for/main 2>&1 | grep -i -e error -e fatal -e rejected | head -2)"
  rec "$id" "push to refs/for/main: ${out:-ok}"
  out="$(cd "$SANDBOX/w" && g commit -q --allow-empty -m x && g push "$SANDBOX/r.git" main 2>&1 | grep -i -e error -e fatal | head -1)"
  rec "$id-normal-push" "push to refs/heads/main: ${out:-ok}"
  drop_sandbox
}
proc_receive H03-proc-receive-absent no
proc_receive H04-proc-receive-noop   yes

# H05/H06: count spawns per everyday command with the full probe set (fresh
# repo per command; <setup> runs with hooks disabled)
count() {
  local id="$1" setup="$2" cmd="$3"
  new_sandbox; init_repo "$SANDBOX/w"; WT="$SANDBOX/w"
  (cd "$WT" && eval "$setup") >/dev/null 2>&1
  install_probes "$WT"; export PROBE_WT="$WT" PROBE_GITDIR="$WT/.git"
  : > "$PROBE_LOG"
  (cd "$WT" && eval "$cmd") >/dev/null 2>&1
  rec "$id" "exit=$?; spawns=$(wc -l < "$PROBE_LOG" | tr -d ' '): $(cut -f2,3 "$PROBE_LOG" | sed -e 's/\t[0-9a-f]\{40\}$//' -e 's/\t.git\/.*$//' | tr '\t' ':' | sed 's/:$//' | sort | uniq -c | awk '{print $2"x"$1}' | paste -sd' ' -)"
  case "$id" in *rebase*|*switch*|*commit*)
    rec "$id-prepared-refs" "$(awk -F'\t' '$2=="reference-transaction" && $3=="prepared"{print $5}' "$PROBE_LOG" | tr '|' '\n' | awk 'NF{print $3}' | sort | uniq -c | awk '{print $2"x"$1}' | paste -sd' ' -)" ;;
  esac
  drop_sandbox
}
three_commits='g switch -qc r && for i in 1 2 3; do echo $i >> r.txt; g add r.txt; g commit -qm r$i; done; g switch -q main && echo m > m.txt && g add m.txt && g commit -qm m && g switch -q r'
count H05-status-clean       ':'  'git status'
count H05-status-after-touch ':'  'touch a.txt && sleep 1 && git status'
count H05-diff-after-touch   ':'  'touch a.txt && sleep 1 && git diff'
count H06-commit             ':'  'echo c >> a.txt && git commit -qam c'
count H06-switch-new-branch  ':'  'git switch -qc other'
count H06-checkout-file      ':'  'echo x >> a.txt && git checkout -- a.txt'
count H06-tag                ':'  'git tag t1'
count H06-stash-push-pop     ':'  'echo s >> a.txt && git stash -q && git stash pop -q'
count H06-rebase-3-commits   "$three_commits" 'git rebase -q main'
count H06-rebase-i-3-commits "$three_commits" 'GIT_SEQUENCE_EDITOR=: git rebase -q -i main'
echo "wrote $OUT" >&2
