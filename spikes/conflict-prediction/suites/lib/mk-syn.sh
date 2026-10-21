#!/usr/bin/env bash
# R-SYN: synthetic corpus of pairs with a known conflict (E6, E7). One repo, one case per
# `<case>/base`, `<case>/ours` and `<case>/theirs` branch. Writes `<repo>/../syn-pairs.tsv`:
# `<case>/ours <case>/theirs <case> <expected per merge-ort>`.
#   mk-syn.sh <dir>
set -euo pipefail
source "$(dirname "$0")/common.sh"
R="$1"
[ -e "$R" ] && { echo "$R exists" >&2; exit 1; }
mkdir -p "$R" && cd "$R"
git init -q -b main
git config core.autocrlf false
seq 1 40 | sed 's/^/line /' > README
git add README && git commit -qm root
PAIRS="$R/../syn-pairs.tsv"
: > "$PAIRS"

lines() { seq 1 "$1" | sed "s|^|$2 |"; }
commit() { git add -A && git commit -qm "$1" --allow-empty; }

# case <name> <expected> ; then define base_ ours_ theirs_ functions before calling run_case.
run_case() {
  local name="$1" expected="$2"
  git checkout -q -b "$name/base" main
  base; commit "$name base"
  git checkout -q -b "$name/ours" "$name/base"; ours; commit "$name ours"
  git checkout -q -b "$name/theirs" "$name/base"; theirs; commit "$name theirs"
  printf '%s/ours\t%s/theirs\t%s\t%s\n' "$name" "$name" "$name" "$expected" >> "$PAIRS"
  git checkout -q main
}

fn_file() { # a file with a function, line N replaced by $2
  { echo "fn main() {"; lines 30 "    let x ="; echo "}"; } > "$1"
}

# 1. Same function, same line.
base() { fn_file f.rs; }
ours() { sed -i '' 's/^    let x = 10$/    let x = 10 + ours;/' f.rs; }
theirs() { sed -i '' 's/^    let x = 10$/    let x = 10 + theirs;/' f.rs; }
run_case same-line conflict
# 2. Contiguous lines (Git conflicts on adjacent changes).
ours() { sed -i '' 's/^    let x = 10$/    let x = 10 + ours;/' f.rs; }
theirs() { sed -i '' 's/^    let x = 11$/    let x = 11 + theirs;/' f.rs; }
run_case adjacent-lines conflict
# 3. Same file, distant lines: overlap without conflict.
ours() { sed -i '' 's/^    let x = 3$/    let x = 3 + ours;/' f.rs; }
theirs() { sed -i '' 's/^    let x = 25$/    let x = 25 + theirs;/' f.rs; }
run_case overlap-clean clean
# 4. Add/add with different content.
base() { :; }
ours() { echo ours > new.txt; }
theirs() { echo theirs > new.txt; }
run_case add-add conflict
# 5. Add/add with the same content.
ours() { echo same > new.txt; }
theirs() { echo same > new.txt; }
run_case add-add-same clean
# 6. Modify/delete.
base() { lines 10 a > md.txt; }
ours() { echo more >> md.txt; }
theirs() { git rm -q md.txt; }
run_case modify-delete conflict
# 7. Rename with modification on the other side (clean).
base() { lines 30 r > old.txt; }
ours() { git mv old.txt renamed.txt; }
theirs() { sed -i '' 's/^r 5$/r 5 theirs/' old.txt; }
run_case rename-modify clean
# 8. Rename/rename to different names.
ours() { git mv old.txt ours-name.txt; }
theirs() { git mv old.txt theirs-name.txt; }
run_case rename-rename conflict
# 9. Rename with conflicting modification.
ours() { git mv old.txt renamed.txt; sed -i '' 's/^r 5$/r 5 ours/' renamed.txt; }
theirs() { sed -i '' 's/^r 5$/r 5 theirs/' old.txt; }
run_case rename-modify-conflict conflict
# 10. Directory rename vs a new file in the old directory (merge.directoryRenames=conflict).
base() { mkdir -p d; lines 20 one > d/one.txt; lines 20 two > d/two.txt; }
ours() { git mv d e; }
theirs() { lines 20 new > d/new.txt; }
run_case dir-rename conflict
# 11. Directory/file.
base() { :; }
ours() { echo file > x; }
theirs() { mkdir -p x; echo inside > x/y; }
run_case dir-file conflict
# 12. Binary modified on both sides.
base() { head -c 4096 /dev/zero | tr '\0' '\1' > bin.dat; printf '\0' >> bin.dat; }
ours() { printf 'ours\0' >> bin.dat; }
theirs() { printf 'theirs\0' >> bin.dat; }
run_case binary conflict
# 13. Submodule (gitlink) moved to different commits on each side.
SUB_A=$(git rev-parse main)
base() { git update-index --add --cacheinfo "160000,$SUB_A,sub"; }
ours() { git update-index --cacheinfo "160000,$(git rev-parse same-line/ours),sub"; }
theirs() { git update-index --cacheinfo "160000,$(git rev-parse same-line/theirs),sub"; }
commit_sub() { git commit -qm "$1" --allow-empty; }
git checkout -q -b submodule/base main; base; commit_sub "submodule base"
git checkout -q -b submodule/ours submodule/base; ours; commit_sub "submodule ours"
git checkout -q -b submodule/theirs submodule/base; theirs; commit_sub "submodule theirs"
git checkout -q -f main
printf 'submodule/ours\tsubmodule/theirs\tsubmodule\tconflict\n' >> "$PAIRS"
# 14. LFS pointers (text pointers in the tree; no LFS filter involved).
lfs() { printf 'version https://git-lfs.github.com/spec/v1\noid sha256:%064d\nsize %s\n' "$1" "$2" > big.bin; }
base() { lfs 1 100; printf 'big.bin filter=lfs diff=lfs merge=lfs -text\n' > .gitattributes; }
ours() { lfs 2 200; }
theirs() { lfs 3 300; }
run_case lfs-pointer conflict
# 15. CRLF file, same line.
base() { lines 20 c | sed 's/$/\r/' > crlf.txt; }
ours() { sed -i '' 's/^c 7\r$/c 7 ours\r/' crlf.txt; }
theirs() { sed -i '' 's/^c 7\r$/c 7 theirs\r/' crlf.txt; }
run_case crlf conflict
# 16. merge=union in .gitattributes (merge-ort resolves; A2 ignores attributes, L-04).
base() { printf 'u.txt merge=union\n' > .gitattributes; lines 5 u > u.txt; }
ours() { echo ours >> u.txt; }
theirs() { echo theirs >> u.txt; }
run_case union-attr clean
# 17. -merge in .gitattributes (merge-ort conflicts on any two-sided change; A2 merges).
base() { printf 'n.txt -merge\n' > .gitattributes; lines 30 n > n.txt; }
ours() { sed -i '' 's/^n 2$/n 2 ours/' n.txt; }
theirs() { sed -i '' 's/^n 28$/n 28 theirs/' n.txt; }
run_case nomerge-attr conflict
# 18. F2 (gix-merge 0.21 data loss): one side removes a/b and fills a/b/ only with renames.
base() { mkdir -p a x; lines 20 ab > a/b; lines 20 x1 > x/1; lines 20 x2 > x/2; }
ours() { git rm -q a/b; mkdir -p a/b; git mv x/1 a/b/1; git mv x/2 a/b/2; }
theirs() { sed -i '' 's/^ab 3$/ab 3 theirs/' a/b; }
run_case f2-dir-renames conflict
# 18b. F2 with a conflict inside the renamed directory: ours replaces the file a/b with a
# directory filled only by renames (one of them modified); theirs modifies the rename source.
base() { mkdir -p a x; lines 20 ab > a/b; lines 20 x1 > x/1; lines 20 x2 > x/2; }
ours() { git rm -q a/b; mkdir -p a/b; git mv x/1 a/b/1; git mv x/2 a/b/2; sed -i '' 's/^x1 5$/x1 5 ours/' a/b/1; }
theirs() { sed -i '' 's/^x1 5$/x1 5 theirs/' x/1; }
run_case f2-rename-conflict conflict
# 19. Lines that imitate conflict markers, with a real conflict below them (E7).
base() { { echo "<<<<<<< not a marker"; echo "======="; echo ">>>>>>> fake"; lines 20 m; } > mimic.txt; }
ours() { sed -i '' 's/^m 10$/m 10 ours/' mimic.txt; }
theirs() { sed -i '' 's/^m 10$/m 10 theirs/' mimic.txt; }
run_case mimic-markers conflict
# 20. Several hunks in one file, with auto-merged changes between them (E7 coordinates).
base() { lines 60 h > h.txt; }
ours() { sed -i '' -e 's/^h 5$/h 5 ours/' -e 's/^h 20$/h 20a\
h 20b ours/' -e 's/^h 40$/h 40 ours/' -e '/^h 50$/d' h.txt; }
theirs() { sed -i '' -e '1i\
inserted at top' -e 's/^h 5$/h 5 theirs/' -e 's/^h 30$/h 30 theirs/' -e 's/^h 40$/h 40 theirs/' h.txt; }
run_case multi-hunk conflict
# 21. Symlink changed to different targets.
base() { ln -s target-a link; }
ours() { rm link; ln -s target-ours link; }
theirs() { rm link; ln -s target-theirs link; }
run_case symlink conflict
# 22. Mode change on one side, content on the other (clean).
base() { lines 10 e > exec.sh; }
ours() { chmod +x exec.sh; }
theirs() { echo more >> exec.sh; }
run_case mode-content clean

# 23. Criss-cross history: two merge-bases (virtual merge-base).
git checkout -q -b cc/base main
lines 30 cc > cc.txt; commit "cc base"
git checkout -q -b cc/a cc/base; sed -i '' 's/^cc 5$/cc 5 a/' cc.txt; commit "cc a"
git checkout -q -b cc/b cc/base; sed -i '' 's/^cc 25$/cc 25 b/' cc.txt; commit "cc b"
git checkout -q -b cc/ours cc/a; git merge -q --no-edit cc/b; sed -i '' 's/^cc 15$/cc 15 ours/' cc.txt; commit "cc ours"
git checkout -q -b cc/theirs cc/b; git merge -q --no-edit cc/a; sed -i '' 's/^cc 15$/cc 15 theirs/' cc.txt; commit "cc theirs"
git checkout -q main
printf 'cc/ours\tcc/theirs\tcriss-cross\tconflict\n' >> "$PAIRS"
git checkout -q -b cc2/ours cc/ours; sed -i '' 's/^cc 2$/cc 2 o/' cc.txt; commit "cc2 ours"
git checkout -q -b cc2/theirs cc/theirs; sed -i '' 's/^cc 28$/cc 28 t/' cc.txt; commit "cc2 theirs"
git checkout -q main
printf 'cc2/ours\tcc2/theirs\tcriss-cross-clean\tconflict\n' >> "$PAIRS"

# 24. Demo of the BRD § 13 (Q-CKP-25): 4 sessions; agent-1 and agent-2 touch the same function.
git checkout -q -b demo/base main
mkdir -p src; { echo "pub fn total(items: &[u32]) -> u32 {"; lines 8 "    // step"; echo "    items.iter().sum()"; echo "}"; } > src/cart.rs
lines 20 "doc" > src/README.md; lines 20 "cfg" > src/config.toml
commit "demo base"
git checkout -q -b demo/agent-1 demo/base; sed -i '' 's/^    items.iter().sum()$/    items.iter().map(|i| i * 2).sum()/' src/cart.rs; commit "agent-1"
git checkout -q -b demo/agent-2 demo/base; sed -i '' 's/^    items.iter().sum()$/    items.iter().filter(|i| **i > 0).sum()/' src/cart.rs; commit "agent-2"
git checkout -q -b demo/agent-3 demo/base; echo "extra" >> src/README.md; commit "agent-3"
git checkout -q -b demo/agent-4 demo/base; sed -i '' 's/^cfg 1$/cfg 1 = true/' src/config.toml; sed -i '' 's/^    \/\/ step 1$/    \/\/ step 1 (agent-4)/' src/cart.rs; commit "agent-4"
git checkout -q main
for p in "1 2 conflict" "1 3 clean" "1 4 clean" "2 3 clean" "2 4 clean" "3 4 clean"; do
  set -- $p
  printf 'demo/agent-%s\tdemo/agent-%s\tdemo-%s-%s\t%s\n' "$1" "$2" "$1" "$2" "$3" >> "$PAIRS"
done
git commit-graph write --reachable 2>/dev/null || true
echo "R-SYN: $(wc -l < "$PAIRS" | tr -d ' ') pairs in $R"
