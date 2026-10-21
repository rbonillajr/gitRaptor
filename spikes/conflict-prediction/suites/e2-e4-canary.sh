#!/usr/bin/env bash
# E2 — Zero execution, E3 — no downloads (M-02), E4 — same objects and zero attributes (L-04).
#
# R-CAN: the SEC-09 canary of crates/testkit/src/canary.rs (every configurable program is a
# script that leaves a marker), extended with `merge.evil.driver`, `merge=evil` / `filter=evil`
# by the three routes (working tree, index, tree) and `info/attributes`, `core.fsmonitor`,
# hooks, `refs/replace/*` and `info/grafts`. A `trap/` directory first in PATH catches any
# program launched by name. A1 (control) must fire the canary; A2, A2-W and B must not.
#
# R-PC: a partial clone (`--filter=blob:none`) of R-SYN from a local promisor remote whose
# `uploadpack` is a marker script: any lazy fetch leaves a marker.
#   e2-e4-canary.sh
set -uo pipefail
source "$(dirname "$0")/lib/common.sh"
OUT="$SPIKE_DIR/results/e2-e4-canary.tsv"
mkdir -p "$SPIKE_DIR/results"
printf 'exp\tcase\tengine\tgit\tresult\tconflicts\tmarkers\tintact_changes\terror\n' > "$OUT"
C="$CKP_ROOT/can"
[ -e "$C" ] && { echo "$C exists: remove it first" >&2; exit 1; }
mkdir -p "$C/bin" "$C/markers" "$C/trap" "$C/profile"
REPO="$C/repo"

arm() { # <name> <tail> -> path of a marker script
  printf '#!/bin/sh\n: > "%s/markers/%s"\n%s\n' "$C" "$1" "$2" > "$C/bin/$1"
  chmod +x "$C/bin/$1"
  echo "$C/bin/$1"
}
for p in git sh bash env gpg ssh less more git-upload-pack git-lfs; do
  printf '#!/bin/sh\n: > "%s/markers/trap-%s"\nexit 1\n' "$C" "$p" > "$C/trap/$p"
  chmod +x "$C/trap/$p"
done

# --- R-CAN --------------------------------------------------------------------------------
git init -q -b main "$REPO"
cd "$REPO"
seq 1 30 | sed 's/^/line /' > a.txt
seq 1 30 | sed 's/^/line /' > b.txt
printf '*.txt merge=evil filter=evil diff=evil\n' > .gitattributes   # in the tree
git add -A && git commit -qm base
git checkout -q -b ours
sed -i '' 's/^line 10$/line 10 ours/' a.txt; sed -i '' 's/^line 2$/line 2 ours/' b.txt
git commit -qam ours
git checkout -q -b theirs main
sed -i '' 's/^line 10$/line 10 theirs/' a.txt; sed -i '' 's/^line 28$/line 28 theirs/' b.txt
git commit -qam theirs
git checkout -q -b clean-theirs main
sed -i '' 's/^line 25$/line 25 clean/' a.txt
git commit -qam clean-theirs
git checkout -q main
# A replacement and a graft that would change what the pair means if they were honoured.
git checkout -q -b decoy main
sed -i '' 's/^line 10$/line 10 decoy-identical-to-ours/' a.txt; git commit -qam decoy
git checkout -q main
git replace "$(git rev-parse theirs)" "$(git rev-parse clean-theirs)"
mkdir -p .git/info
printf '%s\n' "$(git rev-parse ours)" > .git/info/grafts   # `ours` as a root: no merge-base
# Programs, armed after the commits.
git config merge.evil.name evil
git config merge.evil.driver "$(arm merge-driver 'exit 0') %O %A %B"
git config merge.default evil
git config filter.evil.clean "$(arm filter-clean cat)"
git config filter.evil.smudge "$(arm filter-smudge cat)"
git config filter.evil.process "$(arm filter-process 'exit 1')"
git config filter.evil.required true
git config diff.evil.textconv "$(arm textconv 'cat "$1"')"
git config diff.external "$(arm diff-external 'exit 0')"
git config gpg.program "$(arm gpg 'exit 1')"
git config core.pager "$(arm pager cat)"
git config credential.helper "$(arm credential 'exit 0')"
git config core.fsmonitor "$(arm fsmonitor-hook 'exit 1')"
mkdir -p .git/hooks
for h in post-index-change reference-transaction post-checkout post-merge; do arm "hook-$h" 'exit 0' > /dev/null; cp "$C/bin/hook-$h" ".git/hooks/$h"; done
# merge=evil by the working tree, the index and info/attributes as well.
printf '*.txt merge=evil filter=evil\n' > .git/info/attributes
printf '*.txt merge=evil filter=evil\n*.md merge=evil\n' > .gitattributes
git update-index --add .gitattributes 2>/dev/null || true
printf '* merge=evil\n' > .gitattributes   # on disk, different from the index
: > /dev/null
rm -f "$C/markers/"*

markers() { ls "$C/markers" 2>/dev/null | tr '\n' ',' | sed 's/,$//'; }
row() { printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$@" | tee -a "$OUT"; }

run_case() { # <exp> <case> <engine> <gitver> <gitbin> <repo> <pairs> [replace-flag]
  local exp=$1 case=$2 engine=$3 gv=$4 gb=$5 repo=$6 pairs=$7
  rm -f "$C/markers/"*
  local res tsv
  tsv="$C/out-$exp-$case-$engine-$gv.tsv"
  res=$(PATH="$C/trap" "$CKP" intact "$repo" -- "$CKP" batch "$engine" "$repo" "$pairs" \
        --git "$gb" --store "$C/profile/$exp-$case-$gv.git" --scratch "$C/profile" 2>/dev/null)
  # batch output is discarded by `intact`; run again outside it for the result itself.
  PATH="$C/trap" "$CKP" batch "$engine" "$repo" "$pairs" --git "$gb" \
      --store "$C/profile/$exp-$case-$gv-2.git" --scratch "$C/profile" > "$tsv" 2>/dev/null
  local changes
  changes=$(echo "$res" | grep -o 'changes=[0-9]*' | head -1 | cut -d= -f2)
  row "$exp" "$case" "$engine" "$gv" "$(cut -f3 "$tsv" | sort -u | tr '\n' ' ')" \
      "$(cut -f6 "$tsv" | tr '\n' ' ')" "$(markers)" "${changes:-?}" "$(cut -f9 "$tsv" | head -1 | cut -c1-120)"
}

printf 'ours\ttheirs\n' > "$C/pair-conflict.tsv"
printf 'ours\tclean-theirs\n' > "$C/pair-clean.tsv"
touch "$C/.gitraptor-testkit-root"

ENGINES=("a1 -" "a2 -" "a2w -")
while read -r gv gb; do ENGINES+=("b $gv:$gb"); done < <(git_versions)

for e in "${ENGINES[@]}"; do
  set -- $e; engine=$1; gv=${2%%:*}; gb=${2#*:}; [ "$gb" = - ] && gb=/usr/bin/git && gv=-
  # E2: content conflict on a .txt with merge=evil by every route, and a clean pair.
  run_case E2 conflict "$engine" "$gv" "$gb" "$REPO" "$C/pair-conflict.tsv"
  run_case E2 clean "$engine" "$gv" "$gb" "$REPO" "$C/pair-clean.tsv"
done

# E4: replacement and grafts. With them honoured, `theirs` would be `clean-theirs` (clean
# merge) and `ours` would have no merge-base. Expected: the original objects (a.txt conflict).
for flag in unset true false; do
  if [ "$flag" = unset ]; then git -C "$REPO" config --unset core.useReplaceRefs 2>/dev/null; else git -C "$REPO" config core.useReplaceRefs "$flag"; fi
  for e in "a2 -" "a2w -"; do
    set -- $e
    run_case E4 "replace-and-grafts(useReplaceRefs=$flag)" "$1" - /usr/bin/git "$REPO" "$C/pair-conflict.tsv"
  done
done
git -C "$REPO" config --unset core.useReplaceRefs 2>/dev/null
while read -r gv gb; do
  run_case E4 "replace-and-grafts" b "$gv" "$gb" "$REPO" "$C/pair-conflict.tsv"
done < <(git_versions)

# --- R-PC: partial clone with a promisor remote whose upload-pack leaves a marker ---------
PC="$C/pc/repo"
mkdir -p "$C/pc"
git -C "$CKP_ROOT/syn/repo" config uploadpack.allowFilter true
git -C "$CKP_ROOT/syn/repo" config uploadpack.allowAnySHA1InWant true
git clone -q --filter=blob:none "file://$CKP_ROOT/syn/repo" "$PC"
echo "R-PC: $(git -C "$PC" rev-list --objects --missing=print --all | grep -c '^?') missing objects"
git -C "$PC" config remote.origin.uploadpack "$(arm upload-pack 'exit 1')"
for b in $(cut -f1,2 "$CKP_ROOT/syn/syn-pairs.tsv" | tr '\t' '\n' | sort -u); do
  git -C "$PC" branch -q -f "$b" "origin/$b" 2>/dev/null
done
printf 'same-line/ours\tsame-line/theirs\n' > "$C/pair-pc.tsv"
for e in "${ENGINES[@]}"; do
  set -- $e; engine=$1; gv=${2%%:*}; gb=${2#*:}; [ "$gb" = - ] && gb=/usr/bin/git && gv=-
  [ "$engine" = a1 ] && continue
  run_case E3 partial-clone "$engine" "$gv" "$gb" "$PC" "$C/pair-pc.tsv"
done
echo "E2-E4 done: $OUT"
