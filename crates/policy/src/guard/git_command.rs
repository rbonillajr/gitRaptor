//! The subcommand of a `git` command line, read only when it is **certain** (D6): the program is
//! `git` itself, every global option before the subcommand is one Git documents, and the
//! subcommand is a command Git ships (an alias can never shadow one of those). Anything else is
//! [`GitCommand::Unknown`]. Pure: the caller reads the command line of a process, uses it only to
//! classify and never stores or sends it (ADR-GRD-003 § 1).

use std::ffi::OsStr;

/// Global options of `git` (before the subcommand) that take the next argument.
const WITH_VALUE: [&str; 7] = [
    "-C",
    "-c",
    "--git-dir",
    "--work-tree",
    "--namespace",
    "--config-env",
    "--attr-source",
];

/// Global options of `git` without a value.
const FLAGS: [&str; 14] = [
    "-p",
    "--paginate",
    "-P",
    "--no-pager",
    "--no-replace-objects",
    "--no-lazy-fetch",
    "--bare",
    "--literal-pathspecs",
    "--no-literal-pathspecs",
    "--glob-pathspecs",
    "--noglob-pathspecs",
    "--icase-pathspecs",
    "--no-optional-locks",
    "--no-advice",
];

/// Global options of `git` written `--name=value`.
const WITH_EQUALS: [&str; 7] = [
    "--git-dir=",
    "--work-tree=",
    "--namespace=",
    "--exec-path=",
    "--config-env=",
    "--attr-source=",
    "--super-prefix=",
];

/// Commands Git ships. A name outside this list may be an alias or a `git-<name>` program on the
/// `PATH`, so it is never read as certain.
const SUBCOMMANDS: [&str; 98] = [
    "add",
    "am",
    "annotate",
    "apply",
    "archive",
    "bisect",
    "blame",
    "branch",
    "bundle",
    "cat-file",
    "check-attr",
    "check-ignore",
    "check-mailmap",
    "check-ref-format",
    "checkout",
    "checkout-index",
    "cherry",
    "cherry-pick",
    "clean",
    "clone",
    "column",
    "commit",
    "commit-graph",
    "commit-tree",
    "config",
    "count-objects",
    "describe",
    "diff",
    "diff-files",
    "diff-index",
    "diff-tree",
    "difftool",
    "fast-export",
    "fast-import",
    "fetch",
    "fetch-pack",
    "for-each-ref",
    "format-patch",
    "fsck",
    "gc",
    "grep",
    "hash-object",
    "help",
    "index-pack",
    "init",
    "log",
    "ls-files",
    "ls-remote",
    "ls-tree",
    "maintenance",
    "merge",
    "merge-base",
    "merge-file",
    "merge-tree",
    "mktag",
    "mktree",
    "mv",
    "notes",
    "pack-objects",
    "pack-refs",
    "prune",
    "pull",
    "push",
    "range-diff",
    "read-tree",
    "rebase",
    "receive-pack",
    "reflog",
    "remote",
    "repack",
    "replace",
    "reset",
    "restore",
    "rev-list",
    "rev-parse",
    "revert",
    "rm",
    "send-pack",
    "shortlog",
    "show",
    "show-branch",
    "show-ref",
    "sparse-checkout",
    "stash",
    "status",
    "submodule",
    "switch",
    "symbolic-ref",
    "tag",
    "update-index",
    "update-ref",
    "upload-pack",
    "var",
    "verify-commit",
    "verify-pack",
    "verify-tag",
    "worktree",
    "write-tree",
];

/// Actions of `git worktree`: the only subcommand whose action the classifier reads.
const WORKTREE_ACTIONS: [&str; 8] = [
    "add", "list", "lock", "move", "prune", "remove", "repair", "unlock",
];

/// What a `git` command line certainly is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitCommand {
    /// The subcommand and, for `worktree`, its action.
    Certain(&'static str, Option<&'static str>),
    /// Unreadable, not `git`, an unknown global option, an alias or an unknown subcommand.
    Unknown,
}

impl GitCommand {
    /// The subcommand, when certain.
    pub fn subcommand(&self) -> Option<&'static str> {
        match self {
            Self::Certain(sub, _) => Some(sub),
            Self::Unknown => None,
        }
    }

    /// Whether this is certainly `git <sub>` (with `action`, when given).
    pub fn is(&self, sub: &str, action: Option<&str>) -> bool {
        match self {
            Self::Certain(s, a) => *s == sub && (action.is_none() || *a == action),
            Self::Unknown => false,
        }
    }
}

/// Reads `argv` (program name first). `None` is a command line that could not be read.
pub fn parse(argv: Option<&[impl AsRef<OsStr>]>) -> GitCommand {
    let Some(argv) = argv else {
        return GitCommand::Unknown;
    };
    // The program must be `git` itself: an empty or odd `argv[0]` could shift what follows.
    let program = argv.first().map(|a| std::path::Path::new(a.as_ref()));
    if !program
        .and_then(std::path::Path::file_name)
        .is_some_and(|n| n == "git" || n == "git.exe")
    {
        return GitCommand::Unknown;
    }
    let mut args = argv.iter().skip(1).map(|a| a.as_ref().to_str());
    while let Some(arg) = args.next() {
        // A non-UTF-8 argument before the subcommand is not one Git knows.
        let Some(arg) = arg else {
            return GitCommand::Unknown;
        };
        if WITH_VALUE.contains(&arg) {
            // The value itself may be anything, even non-UTF-8: it is skipped, not read.
            if args.next().is_none() {
                return GitCommand::Unknown;
            }
            continue;
        }
        if FLAGS.contains(&arg) || WITH_EQUALS.iter().any(|p| arg.starts_with(p)) {
            continue;
        }
        let Some(sub) = SUBCOMMANDS.iter().find(|s| **s == arg) else {
            return GitCommand::Unknown;
        };
        if *sub != "worktree" {
            return GitCommand::Certain(sub, None);
        }
        return match args.next().flatten() {
            Some(action) => match WORKTREE_ACTIONS.iter().find(|a| **a == action) {
                Some(action) => GitCommand::Certain(sub, Some(action)),
                None => GitCommand::Unknown,
            },
            None => GitCommand::Unknown,
        };
    }
    GitCommand::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_subcommand_list_is_sorted_and_unique() {
        assert!(SUBCOMMANDS.windows(2).all(|w| w[0] < w[1]));
    }
}
