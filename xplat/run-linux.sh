#!/usr/bin/env bash
# Linux validation of GitRaptor in a container, with one command (cross-platform validation
# stage). Builds xplat/linux/Dockerfile and runs, inside the container and on its own filesystem:
# cargo test --workspace, the repo_intact suites with GITRAPTOR_EXEC_AUDIT=strace and the Git
# version matrix. The checkout is sent as a Git bundle and cloned in the container: it is never
# bind-mounted from the host (inotify does not work well over host mounts).
#
# Usage: xplat/run-linux.sh [--dirty] [--stages "test repo-intact ..."] [--no-build]
#   --dirty     also send uncommitted and untracked (non-ignored) files
#   --stages    subset of: setup test repo-intact git-<version> (default: all)
#   --no-build  reuse the existing image
#
# Results land in xplat/linux/results/ (ignored by Git): summary.md and one log per stage.
# Cargo's registry and target dir live in named Docker volumes, so reruns are incremental.
# Exit code: 0 only if every stage passed.
set -euo pipefail

root=$(git -C "$(dirname "$0")" rev-parse --show-toplevel)
image=gitraptor-xplat-linux
dirty=0
build=1
stages=""
while [ $# -gt 0 ]; do
    case $1 in
    --dirty) dirty=1 ;;
    --no-build) build=0 ;;
    --stages) stages=$2; shift ;;
    -h | --help) sed -n '2,15p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
    shift
done

toolchain=$(sed -n 's/^channel *= *"\(.*\)"/\1/p' "$root/rust-toolchain.toml")
if [ "$build" = 1 ]; then
    docker build --build-arg "RUST_TOOLCHAIN=$toolchain" -t "$image" "$root/xplat/linux"
fi

stage_dir=$(mktemp -d)
container="$image-$$"
trap 'rm -rf "$stage_dir"; docker rm -f "$container" >/dev/null 2>&1 || true' EXIT
git -C "$root" bundle create "$stage_dir/repo.bundle" HEAD 2>/dev/null
if [ "$dirty" = 1 ]; then
    (cd "$root" && git ls-files -z --modified --others --exclude-standard |
        while IFS= read -r -d '' f; do [ -e "$f" ] && printf '%s\0' "$f"; done |
        tar --null -T - -cf "$stage_dir/overlay.tar")
    git -C "$root" ls-files -z --deleted >"$stage_dir/deleted.txt"
fi

results="$root/xplat/linux/results"
rm -rf "$results"
rc=0
tar -C "$stage_dir" -cf - . |
    docker run -i --name "$container" \
        -e "XPLAT_STAGES=$stages" \
        -v gitraptor-xplat-cargo-registry:/home/raptor/.cargo/registry \
        -v gitraptor-xplat-target:/cache/target \
        "$image" || rc=$?
if docker cp "$container:/results" "$results" >/dev/null; then
    echo "results: $results"
else
    echo "could not copy the results out of the container" >&2
    [ "$rc" = 0 ] && rc=1
fi
exit "$rc"
