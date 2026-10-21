#!/usr/bin/env bash
# Builds the Git versions of the spike (2.38.5, the NFR-07 minimum, and 2.56.0, the latest
# stable) into GIT_TOOLS (default ./.tools, git-ignored), from the tags of a clone of git/git
# (GIT_SRC, the same clone R-REAL uses). Nothing is installed globally. Apple Git 2.50.1 is
# /usr/bin/git.
set -euo pipefail
GIT_TOOLS="${GIT_TOOLS:-$(cd "$(dirname "$0")" && pwd)/.tools}"
GIT_SRC="${GIT_SRC:?clone of git/git (bare is fine)}"
# Sources outside this directory: Git >= 2.52 builds a Rust crate, and Cargo would take this
# prototype's [workspace] as its own.
GIT_BUILD="${GIT_BUILD:-${TMPDIR:-/tmp}/ckp-git-build}"
mkdir -p "$GIT_BUILD"
for v in ${GIT_VERSIONS:-2.38.5 2.56.0}; do
  [ -x "$GIT_TOOLS/git-$v/bin/git" ] && { echo "git $v: present"; continue; }
  if [ ! -d "$GIT_BUILD/git-$v" ]; then
    mkdir -p "$GIT_BUILD/git-$v"
    git --git-dir="$GIT_SRC" archive "v$v" | tar x -C "$GIT_BUILD/git-$v"
  fi
  make -C "$GIT_BUILD/git-$v" -j"$(sysctl -n hw.ncpu)" prefix="$GIT_TOOLS/git-$v" \
    NO_GETTEXT=1 NO_TCLTK=1 NO_PERL=1 NO_PYTHON=1 NO_OPENSSL=1 NO_CURL=1 NO_EXPAT=1 \
    NO_INSTALL_HARDLINKS=1 install >/dev/null 2>&1
  "$GIT_TOOLS/git-$v/bin/git" --version
done
