#!/usr/bin/env bash
# Suite 07 — Cost per invocation with a native dispatcher, portable to Windows
# (SPIKE-GRD-001 § 11, "coste en Windows"; US-GRD-001). Same scenarios as
# suite 06, timed with lib/bench.rs (no Python, no shell around the timed
# command) and with one more variant:
#   V0  no hooks
#   V1  sh dispatcher: exits in sh for every reference-transaction state but
#       `prepared`, execs the native stub otherwise (ADR-GRD-001 § 2 design)
#   V2  sh dispatcher that always execs the native stub
#   VN  the native stub itself as the hook file, no sh at all (the "native
#       dispatcher" of ADR-GRD-001 § 2, Enmienda)
# Also checks that Git runs a native executable as a hook, with no extension
# and (Windows) as `<hook>.exe`.
. "$(dirname "$0")/../lib/common.sh"

N="${BENCH_N:-100}"
OUT="$RESULTS_DIR/07-cost-native.tsv"
tsv scenario variant hook_spawns n p50_ms p95_ms max_ms failures > "$OUT"

new_sandbox
EXE=""; case "$(uname -s)" in MINGW*|MSYS*|CYGWIN*) EXE=".exe" ;; esac
STUB="$SANDBOX/raptor-stub$EXE"; BENCH="$SANDBOX/bench$EXE"; DENY="$SANDBOX/deny$EXE"
rustc -O -o "$STUB" "$LIB/hookstub.rs" || { echo "rustc not available" >&2; exit 2; }
rustc -O -o "$BENCH" "$LIB/bench.rs" || exit 2
printf 'fn main() { std::process::exit(1) }\n' > "$SANDBOX/deny.rs"
rustc -O -o "$DENY" "$SANDBOX/deny.rs" || exit 2

# variant_hooks <repo> <variant> <names...>
variant_hooks() {
  local repo="$1" v="$2" d h; shift 2
  d="$(common_dir "$repo")/gitraptor/hooks"
  case "$(common_dir "$repo")" in "$SANDBOX"/*) rm -rf "$d" 2>/dev/null ;; esac
  git -C "$repo" config --unset core.hooksPath 2>/dev/null
  [ "$v" = V0 ] && return
  mkdir -p "$d"
  for h in "$@"; do
    case "$v" in
      V1) if [ "$h" = reference-transaction ]; then
            printf '#!/bin/sh\n[ "$1" = prepared ] || exit 0\nexec %s %s "$@"\n' "'$STUB'" "'$h'" > "$d/$h"
          else printf '#!/bin/sh\nexec %s %s "$@"\n' "'$STUB'" "'$h'" > "$d/$h"; fi ;;
      V2) printf '#!/bin/sh\nexec %s %s "$@"\n' "'$STUB'" "'$h'" > "$d/$h" ;;
      VN) cp "$STUB" "$d/$h" ;;
    esac
    chmod +x "$d/$h"
  done
  git -C "$repo" config core.hooksPath "$d"
}

# spawns <repo> <setup> <hooks> <argv...>: hook invocations of one run
spawns() {
  local repo="$1" setup="$2" hooks="$3" d h; shift 3
  d="$(common_dir "$repo")/gitraptor/hooks"; mkdir -p "$d"
  for h in $hooks; do printf '#!/bin/sh\necho x >> %s\ncat >/dev/null\n' "'$SANDBOX/spawns'" > "$d/$h"; chmod +x "$d/$h"; done
  git -C "$repo" config core.hooksPath "$d"
  [ "$setup" != - ] && (cd "$repo" && sh -c "$setup") >/dev/null 2>&1
  : > "$SANDBOX/spawns"; (cd "$repo" && "$@") >/dev/null 2>&1
  rm -r "$(common_dir "$repo")/gitraptor"; git -C "$repo" config --unset core.hooksPath
  wc -l < "$SANDBOX/spawns" | tr -d ' '
}

# bench <scenario> <repo> <setup|-> <n> <hooks> <argv...>
bench() {
  local scen="$1" repo="$2" setup="$3" n="$4" hooks="$5"; shift 5
  local sp v r; sp="$(spawns "$repo" "$setup" "$hooks" "$@")"
  for v in V0 V1 V2 VN; do
    # shellcheck disable=SC2086
    variant_hooks "$repo" "$v" $hooks
    r="$("$BENCH" "$n" "$repo" "$setup" "$@")"
    tsv "$scen" "$v" "$sp" "$r" >> "$OUT"
    printf '%-26s %s spawns=%-4s %s\n' "$scen" "$v" "$sp" "$r" >&2
  done
  variant_hooks "$repo" V0
}

ALL="$(echo $HOOK_NAMES | tr ' ' '\n' | grep -v -x -e push-to-checkout -e proc-receive -e post-index-change | paste -sd' ' -)"
MIN="pre-push pre-rebase reference-transaction"

# Floor: one process spawn on this machine (native binary, sh, sh + exec)
for c in "$STUB" "sh -c :" "sh -c exec\ '$STUB'"; do
  # shellcheck disable=SC2086
  case "$c" in "sh -c exec"*) r="$("$BENCH" "$N" "$SANDBOX" - sh -c "exec '$STUB'")" ;;
    "sh -c :") r="$("$BENCH" "$N" "$SANDBOX" - sh -c :)" ;; *) r="$("$BENCH" "$N" "$SANDBOX" - "$STUB")" ;; esac
  c="${c//$SANDBOX/\$SANDBOX}"
  tsv "spawn-floor" "$c" - "$r" >> "$OUT"; printf '%-26s %-40s %s\n' spawn-floor "$c" "$r" >&2
done

init_repo "$SANDBOX/w"; W="$SANDBOX/w"
OID="$(git -C "$W" rev-parse HEAD)"

# Does Git run a native executable as a hook? (extensionless, and `.exe`)
for name in reference-transaction "reference-transaction$EXE"; do
  d="$(common_dir "$W")/gitraptor/hooks"; mkdir -p "$d"; cp "$DENY" "$d/$name"; chmod +x "$d/$name"
  git -C "$W" config core.hooksPath "$d"
  if git -C "$W" update-ref refs/heads/native-probe "$OID" 2>/dev/null; then res=not-run; else res=ran-and-denied; fi
  git -C "$W" -c core.hooksPath=/dev/null update-ref -d refs/heads/native-probe 2>/dev/null
  rm -r "$(common_dir "$W")/gitraptor"; git -C "$W" config --unset core.hooksPath
  tsv "native-hook" "$name" - "$res" - - - - >> "$OUT"; printf '%-26s %-40s %s\n' native-hook "$name" "$res" >&2
  [ -z "$EXE" ] && break
done

bench update-ref-1-tx "$W" "git -c core.hooksPath=/dev/null update-ref -d refs/heads/x 2>/dev/null; true" "$N" reference-transaction git update-ref refs/heads/x "$OID"
bench commit-min-set "$W" - "$N" "$MIN" git commit -q --allow-empty -m x
bench commit-full-set "$W" - "$N" "$ALL" git commit -q --allow-empty -m x
bench switch-c-min-set "$W" "git -c core.hooksPath=/dev/null switch -q main 2>/dev/null; git -c core.hooksPath=/dev/null branch -D s 2>/dev/null; true" "$N" "$MIN" git switch -q -c s

git init -q --bare "$SANDBOX/r.git"; (cd "$W" && git remote add origin "$SANDBOX/r.git" && git -c core.hooksPath=/dev/null push -q origin main)
bench push-ff-min-set "$W" "git -c core.hooksPath=/dev/null switch -q main 2>/dev/null; git -c core.hooksPath=/dev/null commit -q --allow-empty -m p" "$(( N / 2 ))" "$MIN" git push -q origin main

# fetch of 1000 refs into a fresh clone, and pack-refs of 1000 loose branches
(cd "$W" && for i in $(seq 1 1000); do echo "create refs/heads/b$i $OID"; done | git -c core.hooksPath=/dev/null update-ref --stdin && git -c core.hooksPath=/dev/null push -q origin 'refs/heads/*:refs/heads/*')
git init -q "$SANDBOX/f"; git -C "$SANDBOX/f" remote add origin "$SANDBOX/r.git"
F_SETUP="git -c core.hooksPath=/dev/null symbolic-ref --delete refs/remotes/origin/HEAD 2>/dev/null; git for-each-ref --format='delete %(refname)' refs/remotes | git -c core.hooksPath=/dev/null update-ref --stdin"
bench fetch-1000-refs "$SANDBOX/f" "$F_SETUP" "${BENCH_FETCH_N:-3}" reference-transaction git fetch -q origin
P_SETUP="git for-each-ref --format='delete %(refname)' 'refs/heads/p*' | git -c core.hooksPath=/dev/null update-ref --stdin; for i in \$(seq 1 1000); do echo \"create refs/heads/p\$i $OID\"; done | git -c core.hooksPath=/dev/null update-ref --stdin"
bench pack-refs-1000-loose "$W" "$P_SETUP" "${BENCH_FETCH_N:-3}" reference-transaction git pack-refs --all

drop_sandbox
echo "wrote $OUT" >&2
