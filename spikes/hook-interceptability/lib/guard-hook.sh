#!/bin/sh
# Guard prototype (SPIKE-GRD-001): what `raptor hook` must decide for
# US-GRD-001, written as a constants-only sh dispatcher. NOT production code:
# no daemon, no NFC normalization, no allowlisted environment.
#   @HOOK@  hook name     @BASE@  protected base branch     @PREV@  previous hooks dir
#   @COMMON@  git common dir     @PRUNE@  1 = recognise the loose-ref prune of pack-refs
HOOK='@HOOK@'
COMMON='@COMMON@'
PRUNE='@PRUNE@'
BASE='refs/heads/@BASE@'
PREV='@PREV@/@HOOK@'
Z40=0000000000000000000000000000000000000000
Z64=0000000000000000000000000000000000000000000000000000000000000000

fold() { printf '%s' "$1" | tr '[:upper:]' '[:lower:]'; }
base_folded=$(fold "$BASE")

deny() {
  [ -n "${GUARD_LOG:-}" ] && printf '%s\tdeny\t%s\n' "$HOOK" "$1" >> "$GUARD_LOG"
  echo "gitraptor(spike): denied: $1" >&2
  exit 1
}

# is_base <ref>: the ref, once HEAD is resolved and case is folded, is the base.
# Sets $resolved. A match that is not byte-identical is an alias (H-06): on a
# case-insensitive filesystem `refs/heads/Main` IS the file of `main`.
is_base() {
  r=$1
  case "$r" in
    HEAD|main-worktree/HEAD|worktrees/*/HEAD) r=$(git symbolic-ref -q "$r" 2>/dev/null || echo "$r") ;;
  esac
  resolved=$r
  [ "$(fold "$r")" = "$base_folded" ]
}

case "$HOOK" in
  pre-push)
    lines=''
    while read -r lref loid rref roid; do
      lines="$lines$lref $loid $rref $roid
"
      is_base "$rref" || continue
      [ "$resolved" = "$BASE" ] || deny "ambiguous alias of the base branch: $rref"
      case "$loid" in "$Z40"|"$Z64") deny "delete of base branch $rref" ;; esac
      case "$roid" in "$Z40"|"$Z64") continue ;; esac
      git cat-file -e "$roid^{commit}" 2>/dev/null || deny "remote tip $roid missing locally: treated as force-push on $rref"
      [ "$(git rev-parse --is-shallow-repository)" = true ] && deny "shallow history: treated as force-push on $rref"
      GIT_NO_REPLACE_OBJECTS=1 git -c core.commitGraph=false merge-base --is-ancestor "$roid" "$loid" \
        || deny "force-push (non fast-forward) on $rref"
    done
    [ -n "${GUARD_LOG:-}" ] && printf '%s\tallow\t\n' "$HOOK" >> "$GUARD_LOG"
    if [ -x "$PREV" ]; then printf '%s' "$lines" | "$PREV" "$@"; exit $?; fi
    exit 0 ;;
  reference-transaction)
    # Only `prepared` is evaluated; any other state (`committed`, `aborted`, and
    # `preparing` since Git 2.5x) only chains.
    if [ "$1" != prepared ]; then
      [ -x "$PREV" ] || exit 0
      exec "$PREV" "$@"
    fi
    lines=''
    while read -r old new ref; do
      lines="$lines$old $new $ref
"
      is_base "$ref" || continue
      [ "$resolved" = "$BASE" ] || deny "ambiguous alias of the base branch: $ref"
      case "$new" in "$Z40"|"$Z64")
        # files backend: pack-refs / gc prune the loose file of a ref already
        # written to packed-refs with the same value. Not a deletion.
        if [ "$PRUNE" = 1 ] && [ "$old" != "$Z40" ] && [ "$old" != "$Z64" ] \
           && [ -f "$COMMON/$ref" ] && [ "$(cat "$COMMON/$ref")" = "$old" ] \
           && grep -qx "$old $ref" "$COMMON/packed-refs" 2>/dev/null; then
          continue
        fi
        deny "delete of base branch $ref" ;;
      esac
    done
    if [ -x "$PREV" ]; then printf '%s' "$lines" | "$PREV" "$@"; exit $?; fi
    exit 0 ;;
  *)
    [ -x "$PREV" ] && exec "$PREV" "$@"
    exit 0 ;;
esac
