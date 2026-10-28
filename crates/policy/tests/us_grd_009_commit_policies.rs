//! US-GRD-009 in `gitraptor-policy` (DS-US-GRD-009 D1, D2, D3, D4, D8, D9, D11), through the
//! public API only: the Conventional Commits grammar (`grammar_*`), the combination and the
//! rules of the size and format policies (`rules_*`) and the shape of the two keys (`config_*`).
//! The three prefixes are disjoint on purpose: the verify filters are substrings.
//!
//! Shapes the plan names but does not spell (§ 6.1): everything below that is not a signature
//! of § 6.3 goes through the small adapter at the top, so a decision of the builder changes one
//! place. The assumed shapes are:
//! - `commit_format::Header { verdict: Verdict, kind: Option<String> }` with
//!   `Verdict::{Conforms, NotConventional, BodyNotSeparated}`; `kind` is the type in lowercase.
//! - `commit_policies::NewCommits { commits: Vec<CommitFacts>, unverifiable: bool }`,
//!   `CommitFacts { oid: String, header: Header, parents: usize, files: Option<Vec<FileLines>>,
//!   partial: bool }`, `FileLines { path: String, lines: u32 }`.
//! - `commit_policies::combine(&[guard::policies::Source]) -> CommitPolicies`, in the order floor,
//!   confirmed floor, worktree, profile, local, as `Layers::policies` does.
//! - `CommitPolicies::needs_commit_facts(actor: Option<AgentKind>) -> bool` (D15).
//! - `settings::Policies { diff_size: Option<DiffSize>, commit_format: Option<CommitFormat> }`,
//!   `DiffSize { max_lines, exclude: Option<Vec<String>>, applies_to: Option<AppliesTo>, effect:
//!   Option<PolicyEffect> }` and `CommitFormat { style, types: Option<Vec<String>>, applies_to,
//!   effect }`.

use std::time::{Duration, Instant};

use gitraptor_api::AgentKind;
use gitraptor_api::guard::{Cause, Effect, Level, ParamKind, Reason, Rule};
use gitraptor_policy::guard::Evaluation;
use gitraptor_policy::guard::commit_format::{Header, Verdict, parse};
use gitraptor_policy::guard::commit_policies::{
    CommitFacts, CommitPolicies, FileLines, NewCommits, apply, combine,
};
use gitraptor_policy::guard::glob::Budget;
use gitraptor_policy::guard::policies::Source;
use gitraptor_policy::settings::model::{AppliesTo, Level as SettingsLevel, PolicyEffect};
use gitraptor_policy::settings::{Code, Settings, SourceKind, SourceStatus, parse_document};

// ── adapter ─────────────────────────────────────────────────────────────────────────────────

fn conforms(h: &Header) -> bool {
    h.verdict == Verdict::Conforms
}

fn kind(h: &Header) -> Option<&str> {
    h.kind.as_deref()
}

fn ok(head: &[u8], parents: usize) -> bool {
    conforms(&parse(head, true, parents))
}

fn facts(
    oid: &str,
    head: &[u8],
    body_separated: bool,
    parents: usize,
    files: &[(&str, u32)],
) -> CommitFacts {
    CommitFacts {
        oid: oid.to_owned(),
        header: parse(head, body_separated, parents),
        parents,
        files: Some(
            files
                .iter()
                .map(|(path, lines)| FileLines {
                    path: (*path).to_owned(),
                    lines: *lines,
                })
                .collect(),
        ),
        partial: false,
    }
}

fn new_commits(commits: Vec<CommitFacts>) -> NewCommits {
    NewCommits {
        commits,
        unverifiable: false,
    }
}

fn settings(json: &str) -> Settings {
    let parsed = parse_document(json.as_bytes(), SettingsLevel::Team, SourceKind::Floor);
    assert!(
        matches!(
            parsed.status,
            SourceStatus::Readable | SourceStatus::Partial
        ),
        "{json}: {parsed:?}"
    );
    (*parsed.settings.expect("a readable source has settings")).clone()
}

/// Rules from `(level, json)` sources, in the order the daemon gives them.
fn rules_of(sources: &[(Level, &str)]) -> CommitPolicies {
    let parsed: Vec<(Level, Settings)> = sources.iter().map(|(l, j)| (*l, settings(j))).collect();
    let list: Vec<Source<'_>> = parsed
        .iter()
        .map(|(level, s)| Source {
            level: *level,
            settings: Some(s),
        })
        .collect();
    combine(&list)
}

fn run(rules: &CommitPolicies, actor: Option<AgentKind>, commits: &NewCommits) -> Evaluation {
    let mut out = Evaluation::allow();
    apply(&mut out, commits, actor, rules, &mut Budget::default());
    out
}

const AGENT: Option<AgentKind> = Some(AgentKind::ClaudeCode);
const SIZE_400: &str = r#"{"policies":{"diffSize":{"maxLines":400}}}"#;
const FORMAT: &str = r#"{"policies":{"commitFormat":{}}}"#;

fn rule_set(e: &Evaluation) -> Vec<(Rule, Level)> {
    e.reasons.iter().map(|r| (r.rule, r.level)).collect()
}

fn param(r: &Reason, kind: ParamKind) -> Option<&str> {
    r.params
        .iter()
        .find(|p| p.kind == kind)
        .map(|p| p.value.raw())
}

// ── grammar ─────────────────────────────────────────────────────────────────────────────────

#[test]
fn grammar_accepts_the_forms_of_the_specification() {
    for head in [
        "feat: add parser",
        "fix: handle empty branch name",
        "feat(core): add parser",
        "feat(core)!: break the api",
        "feat!: break the api",
        "chore(deps-dev): bump",
        "feat:  two spaces then text",
        "feat: ünïcode description",
    ] {
        assert!(
            ok(head.as_bytes(), 1),
            "{head:?} follows Conventional Commits 1.0.0"
        );
    }
    // A scope of 64 bytes is the longest allowed.
    let scope = "s".repeat(64);
    assert!(
        ok(format!("feat({scope}): x").as_bytes(), 1),
        "64-byte scope"
    );
}

#[test]
fn grammar_type_is_read_in_lowercase_and_not_checked_here() {
    for (head, expected) in [
        ("feat: x", "feat"),
        ("FEAT: x", "feat"),
        ("Fix(Core)!: x", "fix"),
        ("wip: not in any list", "wip"),
    ] {
        let h = parse(head.as_bytes(), true, 1);
        assert!(conforms(&h), "{head:?} is well formed whatever its type");
        assert_eq!(
            kind(&h),
            Some(expected),
            "the type of {head:?}, lowercase, for the list check"
        );
    }
}

#[test]
fn grammar_rejects_what_is_not_a_header() {
    for head in [
        "",
        "arreglos varios",
        "feat",
        "feat:",
        "feat:x",
        "feat : x",
        "feat:\tx",
        "feat: ",
        "feat:    ",
        ": x",
        "1eat: x",
        "-feat: x",
        "fe at: x",
        "feat(): x",
        "feat(core: x",
        "feat(a(b)): x",
        "feat(a)b): x",
        "feat()!: x",
        "feat!(core): x",
        "feat!!: x",
        "feat(core) : x",
    ] {
        assert!(
            !ok(head.as_bytes(), 1),
            "{head:?} is not Conventional Commits"
        );
    }
    let scope = "s".repeat(65);
    assert!(
        !ok(format!("feat({scope}): x").as_bytes(), 1),
        "65-byte scope"
    );
    // A newline cannot be inside a scope: the header is one line.
    assert!(!ok(b"feat(a\nb): x", 1));
}

#[test]
fn grammar_a_second_line_must_be_blank() {
    let glued = parse(b"feat: x", false, 1);
    assert!(!conforms(&glued), "a body glued to the first line");
    assert_eq!(
        glued.verdict,
        Verdict::BodyNotSeparated,
        "its own cause (body-not-separated)"
    );
    assert!(
        conforms(&parse(b"feat: x", true, 1)),
        "no body, or a blank second line"
    );
    // The cause for a malformed header wins over the body: the header is what is wrong.
    assert_ne!(
        parse(b"nonsense", false, 1).verdict,
        Verdict::BodyNotSeparated
    );
}

#[test]
fn grammar_the_header_is_bounded_to_4096_bytes() {
    let at = |n: usize| {
        let mut head = b"feat: ".to_vec();
        head.resize(n, b'a');
        head
    };
    assert!(ok(&at(4096), 1), "4096 bytes is the last that fits");
    assert!(!ok(&at(4097), 1), "4097 bytes does not");
}

#[test]
fn grammar_bytes_that_are_not_utf8_are_judged_without_panicking() {
    assert!(
        ok(b"feat: \xff\xfe broken but described", 1),
        "the description is bytes"
    );
    assert!(!ok(b"fe\xffat: x", 1), "a type is ASCII");
    assert!(
        !ok(b"feat(\xff\xfe\xfd): x", 1) || ok(b"feat(\xff\xfe\xfd): x", 1),
        "never panics"
    );
    assert!(!ok(b"\xff\xfe", 1));
}

#[test]
fn grammar_the_messages_git_writes_do_not_break_the_format() {
    // D9 a: a merge commit, and only a merge commit.
    for head in [
        "Merge branch 'x'",
        "Merge remote-tracking branch 'origin/x'",
        "Merge tag 'v1'",
        "Merge pull request #7 from a/b",
        "Merge stuff",
    ] {
        assert!(
            ok(head.as_bytes(), 2),
            "{head:?} written by git merge (two parents)"
        );
        assert!(
            !ok(head.as_bytes(), 1),
            "{head:?} on a normal commit does not pass"
        );
    }
    assert!(
        !ok(b"merge branch 'x'", 2) || ok(b"merge branch 'x'", 2),
        "never panics"
    );
    // D9 c: `fixup!`, `squash!`, `amend!` before a header that follows the format.
    assert!(ok(b"fixup! feat: x", 1));
    assert!(ok(b"squash! feat(core): x", 1));
    assert!(ok(b"amend! fix: x", 1));
    assert!(ok(b"fixup! squash! amend! feat: x", 1), "nested prefixes");
    assert!(
        !ok(b"fixup! wip", 1),
        "the rest of the line must follow the format"
    );
    assert!(!ok(b"fixup! Merge stuff", 1));
    assert!(!ok(b"fixup! ", 1));
    let eight = "fixup! ".repeat(8);
    let nine = "fixup! ".repeat(9);
    assert!(
        ok(format!("{eight}feat: x").as_bytes(), 1),
        "up to 8 prefixes are removed"
    );
    assert!(
        !ok(format!("{nine}feat: x").as_bytes(), 1),
        "the ninth is not"
    );
}

#[test]
fn grammar_never_panics_and_does_not_grow_with_what_it_does_not_read() {
    // A deterministic pseudo-random walk over bytes that matter to the grammar.
    let alphabet: &[u8] = b"featfixMerge ()!:\n\r\t-_ !fixup!\xff\x00xZ09";
    let mut state = 0x9E37_79B9_7F4A_7C15_u64;
    let mut next = move || {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (state >> 33) as usize
    };
    for _ in 0..20_000 {
        let len = next() % 300;
        let head: Vec<u8> = (0..len)
            .map(|_| alphabet[next() % alphabet.len()])
            .collect();
        for (sep, parents) in [(true, 0), (false, 1), (true, 2), (false, 3)] {
            let _ = parse(&head, sep, parents);
        }
    }
    // One pass with a bound: a huge input is rejected quickly, not scanned many times.
    let huge = vec![b'('; 1 << 20];
    let start = Instant::now();
    assert!(!conforms(&parse(&huge, true, 1)));
    let mut nested = b"fixup! ".repeat(100_000);
    nested.extend_from_slice(b"feat: x");
    assert!(!conforms(&parse(&nested, true, 1)));
    assert!(
        start.elapsed() < Duration::from_secs(2),
        "linear time with a size bound (ADR-GRD-003 § 1)"
    );
}

// ── rules ───────────────────────────────────────────────────────────────────────────────────

#[test]
fn rules_without_a_key_nothing_applies_and_nothing_is_read() {
    let rules = rules_of(&[(Level::Floor, "{}")]);
    assert!(!rules.needs_commit_facts(AGENT));
    assert!(!rules.needs_commit_facts(None));
    let c = new_commits(vec![facts(
        "a".repeat(40).as_str(),
        b"wip",
        true,
        1,
        &[("x", 5000)],
    )]);
    assert_eq!(run(&rules, AGENT, &c).effect, Effect::Allow, "scenario 4");
}

#[test]
fn rules_the_lowest_limit_of_the_levels_holds() {
    let floor = (Level::Floor, SIZE_400);
    let local200 = (
        Level::Local,
        r#"{"policies":{"diffSize":{"maxLines":200}}}"#,
    );
    let local1000 = (
        Level::Local,
        r#"{"policies":{"diffSize":{"maxLines":1000}}}"#,
    );
    let commit = |lines| {
        new_commits(vec![facts(
            &"b".repeat(40),
            b"feat: x",
            true,
            1,
            &[("a.rs", lines)],
        )])
    };

    // team 400 + local 200: 300 lines break only the local rule; 450 break both.
    let rules = rules_of(&[floor, local200]);
    assert_eq!(
        rule_set(&run(&rules, AGENT, &commit(300))),
        [(Rule::DiffSize, Level::Local)]
    );
    let mut both = rule_set(&run(&rules, AGENT, &commit(450)));
    both.sort_by_key(|(_, l)| *l as u8);
    assert_eq!(
        both,
        [
            (Rule::DiffSize, Level::Floor),
            (Rule::DiffSize, Level::Local)
        ],
        "one reason per broken rule"
    );
    assert_eq!(run(&rules, AGENT, &commit(150)).effect, Effect::Allow);

    // team 400 + local 1000: the personal level cannot raise it.
    let rules = rules_of(&[floor, local1000]);
    assert_eq!(
        rule_set(&run(&rules, AGENT, &commit(500))),
        [(Rule::DiffSize, Level::Floor)]
    );
    assert_eq!(
        run(&rules, AGENT, &commit(400)).effect,
        Effect::Allow,
        "400 is within a limit of 400"
    );
    assert_eq!(run(&rules, AGENT, &commit(401)).effect, Effect::Deny);
}

#[test]
fn rules_exclusions_only_free_the_rule_that_declares_them() {
    let commit = new_commits(vec![facts(
        &"c".repeat(40),
        b"feat: x",
        true,
        1,
        &[
            ("Cargo.lock", 5000),
            ("src/a.rs", 100),
            ("deep/Cargo.lock", 1),
        ],
    )]);
    // The rule that excludes the lockfile counts the rest: 100 + 1 (a nested one is a different
    // path under `Cargo.lock`, matched at any depth) -> within 400.
    let excluding = rules_of(&[(
        Level::Floor,
        r#"{"policies":{"diffSize":{"maxLines":400,"exclude":["Cargo.lock"]}}}"#,
    )]);
    assert_eq!(run(&excluding, AGENT, &commit).effect, Effect::Allow);
    // Without the exclusion the lockfile counts.
    let plain = rules_of(&[(Level::Floor, SIZE_400)]);
    assert_eq!(run(&plain, AGENT, &commit).effect, Effect::Deny);
    // A personal level that excludes it cannot free the floor's rule (D2).
    let both = rules_of(&[
        (Level::Floor, SIZE_400),
        (
            Level::Profile,
            r#"{"policies":{"diffSize":{"maxLines":1000,"exclude":["Cargo.lock"]}}}"#,
        ),
    ]);
    assert_eq!(
        rule_set(&run(&both, AGENT, &commit)),
        [(Rule::DiffSize, Level::Floor)]
    );
    // …and the worktree neither.
    let worktree = rules_of(&[
        (Level::Floor, SIZE_400),
        (
            Level::Worktree,
            r#"{"policies":{"diffSize":{"maxLines":1000,"exclude":["Cargo.lock"]}}}"#,
        ),
    ]);
    assert_eq!(
        rule_set(&run(&worktree, AGENT, &commit)),
        [(Rule::DiffSize, Level::Floor)]
    );
}

#[test]
fn rules_applies_to_decides_by_the_actor() {
    let big = new_commits(vec![facts(
        &"d".repeat(40),
        b"wip",
        true,
        1,
        &[("a", 5000)],
    )]);
    let default = rules_of(&[(Level::Floor, SIZE_400), (Level::Floor, FORMAT)]);
    assert!(default.needs_commit_facts(AGENT));
    assert!(
        !default.needs_commit_facts(None),
        "nothing to read for the person (D15)"
    );
    assert_eq!(run(&default, AGENT, &big).effect, Effect::Deny);
    assert_eq!(
        run(&default, None, &big).effect,
        Effect::Allow,
        "the person passes by default"
    );

    let everyone = rules_of(&[
        (
            Level::Floor,
            r#"{"policies":{"diffSize":{"maxLines":400,"appliesTo":"everyone"}}}"#,
        ),
        (
            Level::Floor,
            r#"{"policies":{"commitFormat":{"appliesTo":"everyone"}}}"#,
        ),
    ]);
    assert!(everyone.needs_commit_facts(None));
    let e = run(&everyone, None, &big);
    assert_eq!(e.effect, Effect::Deny);
    let found = rule_set(&e);
    assert!(
        found.contains(&(Rule::DiffSize, Level::Floor))
            && found.contains(&(Rule::CommitFormat, Level::Floor)),
        "{found:?}"
    );
}

#[test]
fn rules_effect_ask_asks_and_deny_wins() {
    let big = new_commits(vec![facts(
        &"e".repeat(40),
        b"feat: x",
        true,
        1,
        &[("a", 5000)],
    )]);
    let ask = rules_of(&[(
        Level::Floor,
        r#"{"policies":{"diffSize":{"maxLines":400,"effect":"ask"}}}"#,
    )]);
    let e = run(&ask, AGENT, &big);
    assert_eq!(e.effect, Effect::Ask);
    assert_eq!(rule_set(&e), [(Rule::DiffSize, Level::Floor)]);
    // Two rules broken, one asks and one denies: the maximum, with only its reasons.
    let mixed = rules_of(&[
        (
            Level::Floor,
            r#"{"policies":{"diffSize":{"maxLines":400,"effect":"ask"}}}"#,
        ),
        (Level::Floor, FORMAT),
    ]);
    let bad = new_commits(vec![facts(
        &"e".repeat(40),
        b"wip",
        true,
        1,
        &[("a", 5000)],
    )]);
    let e = run(&mixed, AGENT, &bad);
    assert_eq!(e.effect, Effect::Deny);
    assert_eq!(rule_set(&e), [(Rule::CommitFormat, Level::Floor)]);
}

#[test]
fn rules_every_broken_rule_gives_one_reason_and_none_carries_the_message() {
    let rules = rules_of(&[
        (Level::Floor, SIZE_400),
        (
            Level::Floor,
            r#"{"policies":{"commitFormat":{"types":["feat","fix"]}}}"#,
        ),
    ]);
    let oid = "0123456789abcdef0123456789abcdef01234567";
    let secret = "wip: leaking the password hunter2";
    let c = new_commits(vec![facts(oid, secret.as_bytes(), true, 1, &[("a", 1200)])]);
    let e = run(&rules, AGENT, &c);
    assert_eq!(e.effect, Effect::Deny);
    let found = rule_set(&e);
    assert_eq!(found.len(), 2, "size and format, one each: {found:?}");
    assert!(found.contains(&(Rule::DiffSize, Level::Floor)));
    assert!(found.contains(&(Rule::CommitFormat, Level::Floor)));
    let size = e.reasons.iter().find(|r| r.rule == Rule::DiffSize).unwrap();
    assert_eq!(param(size, ParamKind::Lines), Some("1200"));
    assert_eq!(param(size, ParamKind::Limit), Some("400"));
    assert_eq!(
        param(size, ParamKind::Oid),
        Some(&oid[..12]),
        "the oid is 12 characters"
    );
    let format = e
        .reasons
        .iter()
        .find(|r| r.rule == Rule::CommitFormat)
        .unwrap();
    assert_eq!(
        format.cause,
        Some(Cause::TypeNotAllowed),
        "`wip` is well formed but not listed"
    );
    let types = param(format, ParamKind::Types).unwrap();
    assert!(types.contains("feat") && types.contains("fix"), "{types}");
    // D11: nothing of the message, not even the type that was used.
    for reason in &e.reasons {
        for p in &reason.params {
            let v = p.value.raw();
            assert!(
                !v.contains("hunter2") && !v.contains("password") && !v.contains("leaking"),
                "{reason:?}"
            );
        }
    }
    let types_list: Vec<&str> = types.split([',', ' ']).filter(|t| !t.is_empty()).collect();
    assert!(
        !types_list.contains(&"wip"),
        "the type that was used is not repeated: {types}"
    );
}

#[test]
fn rules_the_first_offending_commit_is_named_with_how_many() {
    let rules = rules_of(&[(Level::Floor, SIZE_400)]);
    let (a, b, c) = ("a".repeat(40), "b".repeat(40), "c".repeat(40));
    let commits = new_commits(vec![
        facts(&a, b"feat: small", true, 1, &[("x", 10)]),
        facts(&b, b"feat: big", true, 1, &[("x", 500)]),
        facts(&c, b"feat: bigger", true, 1, &[("x", 900)]),
    ]);
    let e = run(&rules, AGENT, &commits);
    assert_eq!(e.reasons.len(), 1, "{e:?}");
    let r = &e.reasons[0];
    assert_eq!(
        param(r, ParamKind::Oid),
        Some(&b[..12]),
        "the oldest that breaks it"
    );
    assert_eq!(param(r, ParamKind::Lines), Some("500"));
    assert_eq!(param(r, ParamKind::Commits), Some("2"), "how many break it");
}

#[test]
fn rules_a_merge_is_judged_by_its_message_only() {
    let both = rules_of(&[(Level::Floor, SIZE_400), (Level::Floor, FORMAT)]);
    let merge = new_commits(vec![facts(
        &"f".repeat(40),
        b"Merge branch 'x'",
        true,
        2,
        &[("big", 9000)],
    )]);
    assert_eq!(
        run(&both, AGENT, &merge).effect,
        Effect::Allow,
        "no size, and a merge message"
    );
    let odd = new_commits(vec![facts(
        &"f".repeat(40),
        b"whatever",
        true,
        2,
        &[("big", 9000)],
    )]);
    assert_eq!(
        rule_set(&run(&both, AGENT, &odd)),
        [(Rule::CommitFormat, Level::Floor)],
        "format yes, size no"
    );
}

#[test]
fn rules_a_body_glued_to_the_first_line_has_its_own_cause() {
    let rules = rules_of(&[(Level::Floor, FORMAT)]);
    let c = new_commits(vec![facts(&"9".repeat(40), b"feat: x", false, 1, &[])]);
    let e = run(&rules, AGENT, &c);
    assert_eq!(e.reasons.len(), 1, "{e:?}");
    assert_eq!(e.reasons[0].cause, Some(Cause::BodyNotSeparated));
}

#[test]
fn rules_the_team_layout_of_the_message_is_not_changed_by_a_personal_level() {
    let wip = new_commits(vec![facts(&"1".repeat(40), b"wip: x", true, 1, &[])]);
    let docs = new_commits(vec![facts(&"2".repeat(40), b"docs: x", true, 1, &[])]);
    let team = (
        Level::Floor,
        r#"{"policies":{"commitFormat":{"types":["feat","fix"]}}}"#,
    );

    // The floor fixes it: a personal format with more types does not add them…
    let more = rules_of(&[
        team,
        (
            Level::Local,
            r#"{"policies":{"commitFormat":{"types":["feat","fix","wip"]}}}"#,
        ),
    ]);
    assert_eq!(
        run(&more, AGENT, &wip).effect,
        Effect::Deny,
        "`wip` is not the team's"
    );
    // …nor does one with fewer take any away.
    let fewer = rules_of(&[
        team,
        (
            Level::Profile,
            r#"{"policies":{"commitFormat":{"types":["feat"]}}}"#,
        ),
    ]);
    let fix = new_commits(vec![facts(&"3".repeat(40), b"fix: x", true, 1, &[])]);
    assert_eq!(
        run(&fewer, AGENT, &fix).effect,
        Effect::Allow,
        "`fix` is the team's"
    );

    // No floor format, a worktree one: the worktree's, and the personal does not count.
    let worktree = rules_of(&[
        (Level::Floor, "{}"),
        (
            Level::Worktree,
            r#"{"policies":{"commitFormat":{"types":["docs"]}}}"#,
        ),
        (
            Level::Local,
            r#"{"policies":{"commitFormat":{"types":["feat"]}}}"#,
        ),
    ]);
    assert_eq!(run(&worktree, AGENT, &docs).effect, Effect::Allow);
    assert_eq!(
        run(
            &worktree,
            AGENT,
            &new_commits(vec![facts(&"4".repeat(40), b"feat: x", true, 1, &[])])
        )
        .effect,
        Effect::Deny
    );

    // No team level: the local one (before the profile) requires its own.
    let personal = rules_of(&[
        (
            Level::Profile,
            r#"{"policies":{"commitFormat":{"types":["docs"]}}}"#,
        ),
        (
            Level::Local,
            r#"{"policies":{"commitFormat":{"types":["feat"]}}}"#,
        ),
    ]);
    assert_eq!(run(&personal, AGENT, &wip).effect, Effect::Deny);
    assert_eq!(
        run(&personal, AGENT, &docs).effect,
        Effect::Deny,
        "local before profile"
    );
    assert_eq!(
        run(
            &personal,
            AGENT,
            &new_commits(vec![facts(&"5".repeat(40), b"feat: x", true, 1, &[])])
        )
        .effect,
        Effect::Allow
    );
}

#[test]
fn rules_what_cannot_be_counted_is_denied_never_allowed() {
    let rules = rules_of(&[(Level::Floor, SIZE_400)]);
    let mut c = new_commits(vec![facts(
        &"7".repeat(40),
        b"feat: x",
        true,
        1,
        &[("a", 10)],
    )]);
    c.unverifiable = true;
    let e = run(&rules, AGENT, &c);
    assert_eq!(e.effect, Effect::Deny);
    assert_eq!(e.reasons[0].cause, Some(Cause::Unverifiable));
    // Only when a rule applies: the person with a rule for agents has nothing to verify.
    assert_eq!(run(&rules, None, &c).effect, Effect::Allow);
    // A commit whose lines were cut by a bound but already exceed the limit: counted "at least".
    let mut cut = facts(&"8".repeat(40), b"feat: x", true, 1, &[("a", 900)]);
    cut.partial = true;
    let e = run(&rules, AGENT, &new_commits(vec![cut]));
    assert_eq!(e.effect, Effect::Deny);
    assert_ne!(
        e.reasons[0].cause,
        Some(Cause::Unverifiable),
        "over the limit already: a count"
    );
    assert_eq!(param(&e.reasons[0], ParamKind::Lines), Some("≥900"));
    // Cut below the limit: nothing proves it fits.
    let mut small = facts(&"8".repeat(40), b"feat: x", true, 1, &[("a", 10)]);
    small.partial = true;
    let e = run(&rules, AGENT, &new_commits(vec![small]));
    assert_eq!(e.effect, Effect::Deny);
    assert_eq!(e.reasons[0].cause, Some(Cause::Unverifiable));
}

// ── configuration ───────────────────────────────────────────────────────────────────────────

fn codes(json: &str, level: SettingsLevel) -> (SourceStatus, Vec<Code>, Option<Settings>) {
    let p = parse_document(json.as_bytes(), level, SourceKind::Floor);
    (
        p.status,
        p.diagnostics.iter().map(|d| d.code).collect(),
        p.settings.map(|s| (*s).clone()),
    )
}

#[test]
fn config_the_two_keys_are_read_at_every_level_and_are_no_longer_unsupported() {
    let json = r#"{"policies":{
        "diffSize":{"maxLines":400,"exclude":["Cargo.lock","pnpm-lock.yaml"],"appliesTo":"everyone","effect":"ask"},
        "commitFormat":{"style":"conventional","types":["feat","fix"],"appliesTo":"agents","effect":"deny"}}}"#;
    for level in [
        SettingsLevel::Team,
        SettingsLevel::Profile,
        SettingsLevel::Local,
    ] {
        let (status, found, s) = codes(json, level);
        assert_eq!(status, SourceStatus::Readable, "{level:?}: {found:?}");
        assert!(found.is_empty(), "{level:?}: {found:?}");
        assert!(!found.contains(&Code::PolicyNotSupported));
        let p = s.unwrap().policies.unwrap();
        let d = p.diff_size.expect("diffSize is read");
        assert_eq!(d.max_lines as u64, 400);
        assert_eq!(
            d.exclude.as_deref(),
            Some(&["Cargo.lock".to_owned(), "pnpm-lock.yaml".to_owned()][..])
        );
        assert_eq!(d.applies_to, Some(AppliesTo::Everyone));
        assert_eq!(d.effect, Some(PolicyEffect::Ask));
        let f = p.commit_format.expect("commitFormat is read");
        assert_eq!(
            f.types.as_deref(),
            Some(&["feat".to_owned(), "fix".to_owned()][..])
        );
    }
    // `{}` already asks for Conventional Commits.
    let (status, found, s) = codes(r#"{"policies":{"commitFormat":{}}}"#, SettingsLevel::Team);
    assert_eq!((status, found.is_empty()), (SourceStatus::Readable, true));
    assert!(s.unwrap().policies.unwrap().commit_format.is_some());
}

#[test]
fn config_the_limit_is_required_and_bounded() {
    for good in ["1", "400", "100000"] {
        let json = format!(r#"{{"policies":{{"diffSize":{{"maxLines":{good}}}}}}}"#);
        let (status, found, _) = codes(&json, SettingsLevel::Team);
        assert_eq!(
            (status, found.is_empty()),
            (SourceStatus::Readable, true),
            "{good}"
        );
    }
    for bad in ["0", "100001", "-1"] {
        let json = format!(r#"{{"policies":{{"diffSize":{{"maxLines":{bad}}}}}}}"#);
        let (status, found, _) = codes(&json, SettingsLevel::Team);
        assert_eq!(
            status,
            SourceStatus::Ignored,
            "{bad}: the whole source is ignored"
        );
        assert!(found.contains(&Code::OutOfRange), "{bad}: {found:?}");
    }
    for wrong in [r#""400""#, "4.5", "null", "true"] {
        let json = format!(r#"{{"policies":{{"diffSize":{{"maxLines":{wrong}}}}}}}"#);
        let (status, found, _) = codes(&json, SettingsLevel::Team);
        assert_eq!(status, SourceStatus::Ignored, "{wrong}");
        assert!(
            found.contains(&Code::WrongType) || found.contains(&Code::OutOfRange),
            "{wrong}: {found:?}"
        );
    }
    // Without `maxLines` the rule does not exist and the source is partial.
    let (status, found, s) = codes(
        r#"{"policies":{"diffSize":{"exclude":["a"]}}}"#,
        SettingsLevel::Team,
    );
    assert_eq!(status, SourceStatus::Partial);
    assert!(found.contains(&Code::PolicyInvalid), "{found:?}");
    assert!(s.is_none_or(|s| s.policies.is_none_or(|p| p.diff_size.is_none())));
}

#[test]
fn config_lists_are_bounded_and_a_bad_entry_is_dropped_alone() {
    // 65 patterns: more than allowed.
    let many: Vec<String> = (0..65).map(|i| format!("\"p{i}\"")).collect();
    let json = format!(
        r#"{{"policies":{{"diffSize":{{"maxLines":10,"exclude":[{}]}}}}}}"#,
        many.join(",")
    );
    let (status, found, _) = codes(&json, SettingsLevel::Team);
    assert!(
        status == SourceStatus::Ignored || !found.is_empty(),
        "65 patterns exceed the limit: {status:?}"
    );
    // One invalid pattern (a negation) is dropped and the source turns partial.
    let (status, found, s) = codes(
        r#"{"policies":{"diffSize":{"maxLines":10,"exclude":["!nope","Cargo.lock"]}}}"#,
        SettingsLevel::Team,
    );
    assert_eq!(status, SourceStatus::Partial, "{found:?}");
    assert!(found.contains(&Code::PolicyInvalid));
    let d = s.unwrap().policies.unwrap().diff_size.unwrap();
    assert_eq!(d.exclude.as_deref(), Some(&["Cargo.lock".to_owned()][..]));
    // An invalid type is dropped alone.
    let (status, found, s) = codes(
        r#"{"policies":{"commitFormat":{"types":["feat","Bad Type","fix"]}}}"#,
        SettingsLevel::Team,
    );
    assert_eq!(status, SourceStatus::Partial, "{found:?}");
    assert!(found.contains(&Code::PolicyInvalid));
    let f = s.unwrap().policies.unwrap().commit_format.unwrap();
    assert_eq!(
        f.types.as_deref(),
        Some(&["feat".to_owned(), "fix".to_owned()][..])
    );
    // `types: []` is out of bounds: the whole source is ignored.
    let (status, found, _) = codes(
        r#"{"policies":{"commitFormat":{"types":[]}}}"#,
        SettingsLevel::Team,
    );
    assert_eq!(status, SourceStatus::Ignored);
    assert!(found.contains(&Code::OutOfRange), "{found:?}");
    // Every type dropped: the rule accepts no type (an error never relaxes).
    let rules = rules_of(&[(
        Level::Floor,
        r#"{"policies":{"commitFormat":{"types":["Bad Type"]}}}"#,
    )]);
    let c = new_commits(vec![facts(&"6".repeat(40), b"feat: x", true, 1, &[])]);
    assert_eq!(run(&rules, AGENT, &c).effect, Effect::Deny);
}

#[test]
fn config_an_unknown_value_is_read_as_the_strictest() {
    let (status, found, s) = codes(
        r#"{"policies":{"diffSize":{"maxLines":10,"appliesTo":"nobody","effect":"shrug"},
                        "commitFormat":{"style":"pattern","appliesTo":"?","effect":"warn"}}}"#,
        SettingsLevel::Team,
    );
    assert_eq!(status, SourceStatus::Partial, "{found:?}");
    assert!(found.contains(&Code::PolicyInvalid));
    let p = s.unwrap().policies.unwrap();
    let d = p.diff_size.unwrap();
    assert_eq!(d.applies_to, Some(AppliesTo::Everyone));
    assert_eq!(d.effect, Some(PolicyEffect::Deny));
    let f = p.commit_format.unwrap();
    assert_eq!(f.applies_to, Some(AppliesTo::Everyone));
    assert_eq!(f.effect, Some(PolicyEffect::Deny));
    // A wrong JSON type is out of bounds, like any other key.
    let (status, found, _) = codes(
        r#"{"policies":{"commitFormat":"conventional"}}"#,
        SettingsLevel::Team,
    );
    assert_eq!(status, SourceStatus::Ignored);
    assert!(found.contains(&Code::WrongType), "{found:?}");
}
