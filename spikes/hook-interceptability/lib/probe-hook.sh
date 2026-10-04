#!/bin/sh
# Probe dispatcher (SPIKE-GRD-001). Copied once per hook name. Logs one TSV
# line per invocation: seq, hook, arg1, args, stdin, cwd, GIT_DIR, refs, idx, wt, state.
# PROBE_DENY="<hook>[:<arg1>]" makes it exit 1 (optionally only when stdin
# contains PROBE_DENY_MATCH).
[ -z "${PROBE_LOG:-}" ] && exit 0
. '@LIB@/fp.sh'
name=${0##*/}
input=''
case "$name" in
  pre-push|reference-transaction|post-rewrite|pre-receive|post-receive)
    input=$(cat | tr '\n' '|') ;;
esac
seq=$(wc -l < "$PROBE_LOG" | tr -d ' ')
printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$seq" "$name" "${1:-}" "$*" "$input" "$(pwd -P)" "${GIT_DIR:-}" "$(fp_all)" >> "$PROBE_LOG"
case "${PROBE_DENY:-}" in
  "$name"|"$name:${1:-}")
    if [ -z "${PROBE_DENY_MATCH:-}" ]; then exit 1; fi
    case "$input" in *"$PROBE_DENY_MATCH"*) exit 1 ;; esac ;;
esac
exit 0
