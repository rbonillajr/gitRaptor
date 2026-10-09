//! The Guardrails configuration directory is out of an agent's reach (BR-AUTH-004).
//!
//! A product rule, not a setting: no key turns it off, `disableSafeMinimum` included. It reuses
//! the path matcher of the forbidden paths, so the folding of case, Unicode and NTFS names is the
//! same one.

use gitraptor_api::AgentKind;
use gitraptor_api::guard::{Cause, Effect, Level, Param, ParamKind, Reason, Rule};

use super::Evaluation;
use super::glob::{Budget, Kind, Pattern};
use super::policies::Touched;

/// The Guardrails configuration directory, anchored at the root.
pub const CONFIG_PATTERN: &str = "/.gitraptor/";

/// What is matched. `/.gitraptor` also covers the exact name `.gitraptor` when it is a file, a
/// symlink or a submodule (a directory-only pattern would let those through), and everything
/// below it when it is a directory.
const MATCHED: &str = "/.gitraptor";

fn denied(cause: Option<Cause>, params: Vec<Param>) -> Reason {
    Reason {
        rule: Rule::ConfigProtected,
        level: Level::Minimum,
        cause,
        params,
    }
}

/// Decides which names of a root tree are the configuration directory, with the same matcher
/// (and the same folding of case, Unicode and NTFS names) `protect_config` uses.
pub struct RootMatcher(Pattern);

impl RootMatcher {
    /// `None` only when the pattern cannot be built: a bug, never "nothing matches".
    pub fn new() -> Option<Self> {
        Pattern::new(MATCHED, Kind::Path).ok().map(Self)
    }

    /// Whether the root entry `name` is the configuration (a file, a link, a submodule or the
    /// directory). `None` when the budget ran out or the name is too long to compare: the caller
    /// treats it as unverifiable.
    pub fn is_config(&self, name: &str, budget: &mut Budget) -> Option<bool> {
        self.0.matches_path(name, budget).ok()
    }
}

/// An agent's movement whose new commits touch `/.gitraptor/` is denied with
/// `policy.config-protected` (level `minimum`, params `path` = first such path, `pattern`);
/// `touched.unverifiable` with an agent denies with `Cause::Unverifiable`, no params. The
/// person (`actor = None`) is never governed.
pub fn protect_config(
    out: &mut Evaluation,
    touched: &Touched,
    actor: Option<AgentKind>,
    budget: &mut Budget,
) {
    if actor.is_none() {
        return;
    }
    let unverifiable = || denied(Some(Cause::Unverifiable), Vec::new());
    if touched.unverifiable {
        out.add(Effect::Deny, unverifiable());
        return;
    }
    // A pattern that cannot be built is a bug, and never "nothing matches".
    let Ok(pattern) = Pattern::new(MATCHED, Kind::Path) else {
        out.add(Effect::Deny, unverifiable());
        return;
    };
    for path in &touched.paths {
        match pattern.matches_path(path, budget) {
            Ok(true) => {
                out.add(
                    Effect::Deny,
                    denied(
                        None,
                        vec![
                            Param::new(ParamKind::Path, path.as_str()),
                            Param::new(ParamKind::Pattern, CONFIG_PATTERN),
                        ],
                    ),
                );
                return;
            }
            Ok(false) => {}
            // Out of work or a path too long to compare: never "no match".
            Err(_) => {
                out.add(Effect::Deny, unverifiable());
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AGENT: Option<AgentKind> = Some(AgentKind::ClaudeCode);

    fn run(paths: &[&str], unverifiable: bool, actor: Option<AgentKind>) -> Evaluation {
        let touched = Touched {
            paths: paths.iter().map(|s| (*s).to_owned()).collect(),
            unverifiable,
        };
        let mut out = Evaluation::allow();
        protect_config(&mut out, &touched, actor, &mut Budget::default());
        out
    }

    fn denies(paths: &[&str]) -> bool {
        run(paths, false, AGENT).effect == Effect::Deny
    }

    #[test]
    fn it_denies_the_directory_and_names_the_first_path() {
        let out = run(
            &["src/a.rs", ".gitraptor/settings.json", ".gitraptor/x"],
            false,
            AGENT,
        );
        assert_eq!(out.effect, Effect::Deny);
        assert_eq!(out.reasons.len(), 1);
        let r = &out.reasons[0];
        assert_eq!(
            (r.rule, r.level, r.cause),
            (Rule::ConfigProtected, Level::Minimum, None)
        );
        let named: Vec<(ParamKind, &str)> =
            r.params.iter().map(|p| (p.kind, p.value.raw())).collect();
        assert_eq!(
            named,
            [
                (ParamKind::Path, ".gitraptor/settings.json"),
                (ParamKind::Pattern, "/.gitraptor/")
            ]
        );
    }

    #[test]
    fn a_deletion_is_a_touch_too() {
        // The I/O side lists deleted paths like any other change.
        assert!(denies(&[".gitraptor/settings.json"]));
    }

    #[test]
    fn the_exact_name_is_covered_as_a_file_a_link_or_a_submodule() {
        // A file or a symlink named `.gitraptor` at the root, and a submodule (trailing slash).
        assert!(denies(&[".gitraptor"]));
        assert!(denies(&[".gitraptor/"]));
    }

    #[test]
    fn it_is_anchored_at_the_root() {
        assert!(!denies(&["a/.gitraptor/x"]));
        assert!(!denies(&["a/.gitraptor"]));
        assert!(!denies(&[
            ".gitraptorx/y",
            "gitraptor/y",
            "src/.gitraptor.rs"
        ]));
        assert!(!denies(&["src/main.rs"]));
        assert!(!denies(&[]));
    }

    #[test]
    fn case_unicode_and_ntfs_variants_are_the_same_directory() {
        assert!(denies(&[".GitRaptor/x"]));
        assert!(denies(&[".GITRAPTOR"]));
        assert!(denies(&[".gitraptor./x"]));
        assert!(denies(&[".gitraptor /x"]));
        assert!(denies(&[".git\u{200c}raptor/x"]));
    }

    #[test]
    fn the_person_is_never_governed() {
        assert_eq!(
            run(&[".gitraptor/settings.json"], false, None).effect,
            Effect::Allow
        );
        assert_eq!(run(&[], true, None).effect, Effect::Allow);
    }

    #[test]
    fn what_cannot_be_read_denies_an_agent_without_params() {
        let out = run(&[], true, AGENT);
        assert_eq!(out.effect, Effect::Deny);
        assert_eq!(out.reasons[0].rule, Rule::ConfigProtected);
        assert_eq!(out.reasons[0].cause, Some(Cause::Unverifiable));
        assert!(out.reasons[0].params.is_empty());
    }

    #[test]
    fn running_out_of_work_never_reads_as_no_match() {
        let touched = Touched {
            paths: vec!["a/b/c/d.txt".into()],
            unverifiable: false,
        };
        let mut out = Evaluation::allow();
        protect_config(&mut out, &touched, AGENT, &mut Budget::new(1));
        assert_eq!(out.effect, Effect::Deny);
        assert_eq!(out.reasons[0].cause, Some(Cause::Unverifiable));
        // A path longer than the matcher compares.
        let long = "x/".repeat(3000);
        assert!(denies(&[&long]));
    }
}
