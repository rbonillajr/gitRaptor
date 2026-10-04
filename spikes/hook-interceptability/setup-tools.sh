#!/usr/bin/env bash
# Installs the hook managers used by suite 03 into SPIKE_TOOLS (default
# ./.tools, git-ignored): husky and lefthook from npm, pre-commit in a Python
# venv. Nothing is installed globally and no repo is touched.
set -euo pipefail
SPIKE_TOOLS="${SPIKE_TOOLS:-$(cd "$(dirname "$0")" && pwd)/.tools}"
mkdir -p "$SPIKE_TOOLS"
cd "$SPIKE_TOOLS"
[ -f package.json ] || npm init -y >/dev/null
npm install --no-audit --no-fund --ignore-scripts husky@9 lefthook@2 >/dev/null
[ -x venv/bin/pre-commit ] || { python3 -m venv venv && venv/bin/pip install -q 'pre-commit>=4,<5'; }
echo "husky $(node -p "require('./node_modules/husky/package.json').version")"
echo "lefthook $(node_modules/.bin/lefthook version)"
venv/bin/pre-commit --version
