#!/usr/bin/env bash
# Entry point of the Linux validation container (see xplat/run-linux.sh).
#
# stdin: a tar stream with `repo.bundle` (Git bundle of the host checkout) and, optionally,
# `overlay.tar` (uncommitted files, with --dirty). The repository is cloned onto the container's
# own filesystem; nothing is mounted from the host.
#
# Stages (each logged to /results/<stage>.log, summarised in /results/summary.md):
#   setup         pnpm install --frozen-lockfile
#   test          cargo test --workspace, distro Git
#   repo-intact   cargo test --workspace -- repo_intact, GITRAPTOR_EXEC_AUDIT=strace
#   git-<version> cargo test --workspace with /opt/git/<version>/bin first on PATH
#
# Environment: XPLAT_STAGES (space-separated subset, default: all).
set -uo pipefail

RESULTS=/results
SRC=/work/gitRaptor
mkdir -p "$RESULTS" /tmp/in

tar -xf - -C /tmp/in
git clone --quiet /tmp/in/repo.bundle "$SRC"
if [ -f /tmp/in/overlay.tar ]; then
    tar -xf /tmp/in/overlay.tar -C "$SRC"
fi
cd "$SRC"
git config --global user.name "xplat"
git config --global user.email "xplat@localhost"

{
    echo "commit: $(git rev-parse HEAD)$( [ -f /tmp/in/overlay.tar ] && echo ' + uncommitted overlay')"
    echo "kernel: $(uname -srm)"
    echo "rustc: $(rustc --version)"
    echo "distro git: $(/usr/bin/git version)"
    for g in /opt/git/*/bin/git; do echo "matrix git: $("$g" version)"; done
    echo "strace: $(strace -V | head -1)"
    echo "node: $(node --version), pnpm: $(pnpm --version)"
    echo "fs.inotify.max_user_watches: $(cat /proc/sys/fs/inotify/max_user_watches)"
    echo "kernel.yama.ptrace_scope: $(cat /proc/sys/kernel/yama/ptrace_scope 2>/dev/null || echo n/a)"
} >"$RESULTS/environment.txt"
cat "$RESULTS/environment.txt"

matrix=()
for d in /opt/git/*/; do matrix+=("git-$(basename "$d")"); done
stages=${XPLAT_STAGES:-"setup test repo-intact ${matrix[*]}"}

run_stage() {
    local stage=$1
    shift
    echo "=== $stage: $*"
    local start=$SECONDS
    "$@" >"$RESULTS/$stage.log" 2>&1
    local rc=$?
    echo "=== $stage: exit $rc ($((SECONDS - start)) s)"
    echo "$stage $rc $((SECONDS - start))" >>"$RESULTS/stages.txt"
    return 0
}

: >"$RESULTS/stages.txt"
for stage in $stages; do
    case $stage in
    setup) run_stage setup pnpm install --frozen-lockfile ;;
    test) run_stage test cargo test --workspace --no-fail-fast ;;
    repo-intact)
        run_stage repo-intact env GITRAPTOR_EXEC_AUDIT=strace \
            cargo test --workspace --no-fail-fast -- repo_intact
        ;;
    git-*)
        v=${stage#git-}
        run_stage "$stage" env PATH="/opt/git/$v/bin:$PATH" \
            cargo test --workspace --no-fail-fast
        ;;
    *) echo "unknown stage: $stage" >&2 ;;
    esac
done

# Summary: per stage, the totals of every `test result:` line and the failing tests.
{
    echo "# Linux validation summary"
    echo
    echo '```'
    cat "$RESULTS/environment.txt"
    echo '```'
    echo
    echo "| Stage | Exit | Seconds | Passed | Failed | Ignored |"
    echo "|---|---|---|---|---|---|"
    while read -r stage rc secs; do
        read -r p f i < <(grep -h '^test result:' "$RESULTS/$stage.log" |
            awk '{p+=$4; f+=$6; i+=$8} END {print p+0, f+0, i+0}')
        echo "| $stage | $rc | $secs | $p | $f | $i |"
    done <"$RESULTS/stages.txt"
    echo
    while read -r stage rc _; do
        [ "$rc" = 0 ] && continue
        echo "## Failures in $stage"
        echo
        echo '```'
        grep -E '^test .* \.\.\. FAILED$|^error(\[|:)|^    [A-Za-z_:0-9]+$' "$RESULTS/$stage.log" | sort -u | head -80
        echo '```'
        echo
    done <"$RESULTS/stages.txt"
} >"$RESULTS/summary.md"
cat "$RESULTS/summary.md"

! awk '$2 != 0 {bad=1} END {exit !bad}' "$RESULTS/stages.txt"
