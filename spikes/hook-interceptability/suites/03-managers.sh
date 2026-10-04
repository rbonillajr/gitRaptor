#!/usr/bin/env bash
# Suite 03 — Coexistence with hook managers (ADR-GRD-001 § 6, US-GRD-002).
# For own hooks in .git/hooks, husky, lefthook and pre-commit:
#   M1  manager first, then Guardrails: the manager's hook still runs (main and
#       linked worktree) and the guard still denies `git branch -D main`;
#   M2  baseline without Guardrails in a linked worktree (does the manager
#       itself cover linked worktrees?);
#   M3  manager reinstalled AFTER Guardrails: where it writes, whether the
#       dispatchers or core.hooksPath change, and whether protection survives;
#   M4  Guardrails uninstalled: manager still runs, config byte-identical.
# Requires the managers installed by setup-tools.sh (SPIKE_TOOLS).
. "$(dirname "$0")/../lib/common.sh"

SPIKE_TOOLS="${SPIKE_TOOLS:-$SPIKE_ROOT/.tools}"
[ -x "$SPIKE_TOOLS/node_modules/.bin/husky" ] || { echo "run setup-tools.sh first (SPIKE_TOOLS=$SPIKE_TOOLS)" >&2; exit 2; }
export PATH="$SPIKE_TOOLS/node_modules/.bin:$SPIKE_TOOLS/venv/bin:$PATH"

OUT="$RESULTS_DIR/03-managers.tsv"
tsv manager step observation > "$OUT"
rec() {
  local o; o="$(printf '%s' "$2" | sed -e 's/\x1b\[[0-9;]*m//g' -e "s#$SANDBOX#\$SANDBOX#g" -e 's/[│ ]\{2,\}/ /g' | tr '\n' ' ')"
  tsv "$MGR" "$1" "$o" >> "$OUT"; printf '%-10s %-34s %s\n' "$MGR" "$1" "$o" >&2
}

g() { git -c core.hooksPath=/dev/null "$@"; }
dispatch_hash() { (cd "$(common_dir "$WT")/gitraptor/hooks" 2>/dev/null && cat ./* | shasum | cut -c1-10) || echo none; }
hookspath() { git -C "$1" config --show-scope --show-origin --get-all core.hooksPath 2>/dev/null | tr '\t' ' ' | paste -sd';' - ; }

# manager-specific install; each logs "<mgr>" to $SANDBOX/mgr.log on pre-commit
inst_own() {
  printf '#!/bin/sh\necho own >> "%s"\n' "$SANDBOX/mgr.log" > "$WT/.git/hooks/pre-commit"
  chmod +x "$WT/.git/hooks/pre-commit"
}
inst_husky() {
  (cd "$WT" && echo '{"name":"x","private":true}' > package.json && husky >/dev/null 2>&1
   printf 'echo husky >> "%s"\n' "$SANDBOX/mgr.log" > .husky/pre-commit
   g add package.json .husky/pre-commit && g commit -qm husky)
}
reinst_husky() { (cd "$WT" && husky 2>&1; echo "exit=$?"); }
inst_lefthook() {
  (cd "$WT" && printf 'pre-commit:\n  commands:\n    log:\n      run: echo lefthook >> "%s"\n' "$SANDBOX/mgr.log" > lefthook.yml \
   && g add lefthook.yml && g commit -qm lefthook && lefthook install >/dev/null 2>&1)
}
reinst_lefthook() { (cd "$WT" && lefthook install 2>&1 | grep -i -e 'hooksPath is set' -e 'not supported' -e 'anyway' -e sync; echo "exit=${PIPESTATUS[0]}"); }
reinst_lefthook_force() { (cd "$WT" && lefthook install --force 2>&1 | grep -i -e anyway -e renamed -e sync; echo "exit=${PIPESTATUS[0]}"); }
reinst_lefthook_reset() { (cd "$WT" && lefthook install --reset-hooks-path 2>&1 | grep -i -e reset -e unset -e sync; echo "exit=${PIPESTATUS[0]}"); }
# lefthook re-syncs its hooks on `lefthook run` when lefthook.yml changes
reinst_lefthook_autosync() { (cd "$WT" && printf 'pre-push:\n  commands:\n    p:\n      run: "true"\n' >> lefthook.yml && g add lefthook.yml && g commit -qm lh2 \
  && echo y >> c.txt && git add c.txt && git commit -qm autosync 2>&1 | grep -i -e sync -e hooksPath; echo "commit-exit=${PIPESTATUS[0]}"); }
inst_precommit() {
  (cd "$WT" && cat > .pre-commit-config.yaml <<EOF
repos:
- repo: local
  hooks:
  - id: log
    name: log
    entry: sh -c 'echo pre-commit >> "$SANDBOX/mgr.log"'
    language: system
    always_run: true
    pass_filenames: false
EOF
   g add .pre-commit-config.yaml && g commit -qm pc && pre-commit install >/dev/null 2>&1)
}
reinst_precommit() { (cd "$WT" && pre-commit install 2>&1; echo "exit=$?"); }
reinst_precommit_overwrite() { (cd "$WT" && pre-commit install --overwrite 2>&1; echo "exit=$?"); }

ran() { local n; n=$(grep -c "^$1\$" "$SANDBOX/mgr.log" 2>/dev/null); echo "${n:-0}"; }
try_commit() { (cd "$1" && echo x >> c.txt && git add c.txt && git commit -qm c >/dev/null 2>&1); echo $?; }
try_delete_main() { (cd "$WT" && git switch -q -c tmp-$RANDOM 2>/dev/null; git branch -qD main >/dev/null 2>&1); [ $? -ne 0 ] && echo denied || echo DELETED; }

for MGR in own husky lefthook pre-commit; do
  key="${MGR/-/}"
  # M2: baseline without Guardrails, linked worktree
  new_sandbox; init_repo "$SANDBOX/w"; WT="$SANDBOX/w"; "inst_$key"
  (cd "$WT" && g worktree add -q ../wt2 -b wt2 2>/dev/null)
  : > "$SANDBOX/mgr.log"; try_commit "$SANDBOX/wt2" >/dev/null
  rec M2-baseline-linked-wt "no guardrails: manager hook ran $(ran "$MGR")x in linked worktree; hooksPath=[$(hookspath "$WT")]"
  drop_sandbox

  # M1 + M4
  new_sandbox; init_repo "$SANDBOX/w"; WT="$SANDBOX/w"; "inst_$key"
  (cd "$WT" && g worktree add -q ../wt2 -b wt2 2>/dev/null)
  cfg_before="$(shasum < "$WT/.git/config" | cut -c1-10)"
  install_guard "$WT"
  : > "$SANDBOX/mgr.log"; rc=$(try_commit "$WT")
  rec M1-commit-main-wt "commit exit=$rc; manager hook ran $(ran "$MGR")x"
  : > "$SANDBOX/mgr.log"; rc=$(try_commit "$SANDBOX/wt2")
  rec M1-commit-linked-wt "commit exit=$rc; manager hook ran $(ran "$MGR")x"
  rec M1-guard-branch-D-main "$(try_delete_main)"
  uninstall_guard "$WT"
  cfg_after="$(shasum < "$WT/.git/config" | cut -c1-10)"
  : > "$SANDBOX/mgr.log"; rc=$(try_commit "$WT")
  rec M4-after-uninstall "commit exit=$rc; manager hook ran $(ran "$MGR")x; config $( [ "$cfg_before" = "$cfg_after" ] && echo byte-identical || echo "DIFFERS ($cfg_before→$cfg_after)")"
  drop_sandbox

  # M3: manager reinstalled after Guardrails, one sandbox per variant
  case "$key" in
    own) variants="" ;;
    husky) variants="husky" ;;
    lefthook) variants="lefthook lefthook_force lefthook_reset lefthook_autosync" ;;
    precommit) variants="precommit precommit_overwrite" ;;
  esac
  for v in $variants; do
    new_sandbox; init_repo "$SANDBOX/w"; WT="$SANDBOX/w"; "inst_$key"
    install_guard "$WT"
    h0="$(dispatch_hash)"; p0="$(hookspath "$WT")"
    msg="$("reinst_$v")"
    h1="$(dispatch_hash)"; p1="$(hookspath "$WT")"
    rec "M3[$v]-output" "${msg:-<silent>}"
    rec "M3[$v]-hookspath" "$( [ "$p0" = "$p1" ] && echo "unchanged" || echo "CHANGED: [$p0] -> [$p1]")"
    rec "M3[$v]-dispatchers" "$( [ "$h0" = "$h1" ] && echo "unchanged" || echo "CHANGED (hash $h0 -> $h1): $(cd "$(common_dir "$WT")/gitraptor/hooks" && ls | grep -v -x -f <(printf '%s\n' $HOOK_NAMES) | paste -sd, -)")"
    : > "$SANDBOX/mgr.log"; rc=$(try_commit "$WT")
    rec "M3[$v]-commit" "commit exit=$rc; manager hook ran $(ran "$MGR")x"
    rec "M3[$v]-guard-branch-D-main" "$(try_delete_main)"
    drop_sandbox
  done
done
echo "wrote $OUT" >&2
