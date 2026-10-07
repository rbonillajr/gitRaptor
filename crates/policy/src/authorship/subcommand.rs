//! Which Git subcommand made a commit-shaped ref update (DS-US-GRD-018 § 5.3, the inverse list):
//! the second line evaluates every new commit except those of a subcommand that is **certainly**
//! `rebase`, `cherry-pick`, `revert` or `am` (D9). Pure: the daemon passes the command line of
//! the nearest `git` ancestor, read only for this and never stored or sent (ADR-GRD-003 § 1).

use std::ffi::OsStr;

/// Subcommands whose commits keep the original author and trailers (D9): not evaluated.
pub const KEEPS_ORIGINAL: [&str; 4] = ["rebase", "cherry-pick", "revert", "am"];

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

/// Whether the second line evaluates the commit made under a `git` with this command line
/// (`argv`, program name first). `None` (unreadable), an empty one, an unknown global option,
/// an alias or any other subcommand: evaluated. Only a subcommand of [`KEEPS_ORIGINAL`] read
/// with certainty is not.
pub fn second_line_evaluates(argv: Option<&[impl AsRef<OsStr>]>) -> bool {
    let Some(argv) = argv else {
        return true;
    };
    let mut args = argv.iter().skip(1).map(|a| a.as_ref().to_str());
    while let Some(arg) = args.next() {
        // A non-UTF-8 argument before the subcommand is not one Git knows: evaluate.
        let Some(arg) = arg else {
            return true;
        };
        if WITH_VALUE.contains(&arg) {
            if args.next().is_none() {
                return true;
            }
            continue;
        }
        if FLAGS.contains(&arg) || WITH_EQUALS.iter().any(|p| arg.starts_with(p)) {
            continue;
        }
        if arg.starts_with('-') {
            return true;
        }
        return !KEEPS_ORIGINAL.contains(&arg);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evaluates(line: &str) -> bool {
        let argv: Vec<&str> = line.split(' ').filter(|a| !a.is_empty()).collect();
        second_line_evaluates(Some(&argv))
    }

    #[test]
    fn only_a_certain_rebase_cherry_pick_revert_or_am_is_skipped() {
        for line in [
            "git rebase main",
            "git cherry-pick abc",
            "git revert HEAD",
            "git am x.patch",
            "/usr/bin/git -C /r -c user.name=x --no-pager rebase -i main",
            "git --git-dir=/r/.git --work-tree /r cherry-pick abc",
            "git --git-dir /r/.git -p revert HEAD",
        ] {
            assert!(!evaluates(line), "{line}");
        }
    }

    #[test]
    fn anything_else_is_evaluated() {
        for line in [
            "git commit --no-verify -m x",
            "git merge side",
            "git ci --no-verify",
            "git -c alias.x=rebase x",
            "git -c alias.x=commit x",
            "git commit-tree abc",
            "git update-ref refs/heads/main abc",
            "git --unknown-option rebase main",
            "git -C",
            "git",
            "git rebase-alias",
            "git pull --rebase",
        ] {
            assert!(evaluates(line), "{line}");
        }
    }

    #[test]
    fn an_unreadable_or_empty_command_line_is_evaluated() {
        assert!(second_line_evaluates(None::<&[&str]>));
        assert!(second_line_evaluates(Some::<&[&str]>(&[])));
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            let bad = OsStr::from_bytes(b"--\xff");
            assert!(second_line_evaluates(Some(&[
                OsStr::new("git"),
                bad,
                OsStr::new("rebase")
            ])));
        }
    }
}
