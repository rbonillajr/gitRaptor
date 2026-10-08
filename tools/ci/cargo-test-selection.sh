#!/usr/bin/env bash
# Package selection for the general tests of `lint and test (<os>)` (ADR-GRP-002, Enmienda
# 2026-10-07).
#
#   SCOPE=affected PROJECTS="gitraptor-cli gitraptor-mcp" tools/ci/cargo-test-selection.sh
#
# Writes `args` to $GITHUB_OUTPUT (or prints it): `--workspace`, or `--workspace --exclude …` for
# the packages Nx did not mark as affected. One cargo invocation, never one per package.
#
# Cargo unifies features over the selected packages, so a smaller selection can build serde,
# memchr and everything above them (gix, the workspace crates) with other features and recompile
# ~100 units the workspace build already has. The selection is used only if every package and
# feature set it resolves is already in the workspace's (`cargo tree`, about a second, per OS
# because of the `cfg` dependencies); otherwise the whole workspace runs. Read-only.
set -euo pipefail

emit() {
  echo "args=$1"
  if [ -n "${GITHUB_OUTPUT:-}" ]; then echo "args=$1" >> "$GITHUB_OUTPUT"; fi
}

if [ "${SCOPE:-}" != affected ] || [ -z "${PROJECTS:-}" ]; then
  echo "Whole workspace (scope: ${SCOPE:-none})"
  emit --workspace
  exit 0
fi

selection=(--workspace)
for member in $(cargo metadata --no-deps --format-version 1 | jq -r '.packages[].name' | tr -d '\r'); do
  case " $PROJECTS " in
    *" $member "*) ;;
    *) selection+=(--exclude "$member") ;;
  esac
done
if [ "${#selection[@]}" -eq 1 ]; then
  echo "Every package is affected"
  emit --workspace
  exit 0
fi

tree() { cargo tree -e normal,build,dev -f '{p} {f}' --prefix none "$@" | sed 's/ (\*)$//' | sort -u; }
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
tree --workspace > "$tmp/workspace"
tree "${selection[@]}" > "$tmp/selection"
diverging=$(comm -23 "$tmp/selection" "$tmp/workspace")
if [ -n "$diverging" ]; then
  echo "::notice::Affected packages ($PROJECTS) resolve other features than the workspace build; running the whole workspace instead of recompiling"
  echo "$diverging"
  emit --workspace
  exit 0
fi
echo "Affected packages: $PROJECTS"
emit "${selection[*]}"
