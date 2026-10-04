#!/usr/bin/env bash
# Runs every suite against one Git (GIT_BIN_DIR, default: first git in PATH).
# Results: results/<os>-<arch>-git<version>[-reftable]/*.tsv
#   SKIP_COST=1       skip suite 06 (timing; run it alone on an idle machine)
#   REF_FORMAT=reftable  repos under test use reftable (Git >= 2.45)
set -u
here="$(cd "$(dirname "$0")" && pwd)"
for s in "$here"/suites/0*.sh; do
  case "$s" in *06-cost.sh) [ -n "${SKIP_COST:-}" ] && continue ;; esac
  echo "== $(basename "$s")" >&2
  bash "$s"
done
