# Shared helpers of the SPIKE-CKP-001 suites. Sourced, never run.
# Every repo lives under CKP_ROOT (default /private/tmp/ckp-spike), a directory outside any
# repository and marked as a testkit root, so the INF-GRP-001 fingerprint guard accepts it.

CKP_ROOT="${CKP_ROOT:-/private/tmp/ckp-spike}"
SPIKE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CKP="$SPIKE_DIR/target/release/ckp-spike"
TOOLS="$SPIKE_DIR/.tools"
mkdir -p "$CKP_ROOT"
touch "$CKP_ROOT/.gitraptor-testkit-root"

# Isolated Git for building fixtures: no system or global config, fixed identity and dates.
export GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null
export GIT_AUTHOR_NAME=spike GIT_AUTHOR_EMAIL=spike@example.com
export GIT_COMMITTER_NAME=spike GIT_COMMITTER_EMAIL=spike@example.com
export GIT_AUTHOR_DATE="1700000000 +0000" GIT_COMMITTER_DATE="1700000000 +0000"

# The Git versions of the matrix: name and binary.
git_versions() {
  for v in 2.38.5 2.56.0; do
    [ -x "$TOOLS/git-$v/bin/git" ] && echo "$v $TOOLS/git-$v/bin/git"
  done
  echo "2.50.1-apple /usr/bin/git"
}

results_dir() { # <git-version>
  local d="$SPIKE_DIR/results/macos-git$1"
  mkdir -p "$d"
  echo "$d"
}

load() { uptime | sed 's/.*load averages*: //'; }
