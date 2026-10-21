//! `raptor events` output, text and JSON (US-GRP-002, US-GRP-005): the latest
//! Git events of the observed repos, with their time and actor, and the
//! observation gaps among them. A one-off query;
//! the live view belongs to the Cockpit (F-001-02).
//!
//! Every text that comes from the engine is untrusted (SEC-12): the text
//! output prints it sanitized; the JSON output carries it as JSON strings.

use std::fmt::Write as _;

use gitraptor_api::messages::{GitEventKind, GitEventView, InferredAgent, TrailerCheck};
use gitraptor_api::methods::GapView;
use gitraptor_api::{Actor, UntrustedName};
use serde_json::{Value, json};

use crate::i18n::{Tr, t};
use crate::status::wire;

fn event_line(e: &GitEventView) -> String {
    let worktree = e.worktree.sanitized();
    let worktree = if e.details.worktree_inferred {
        t("events.inferred", &[("worktree", &worktree)])
    } else {
        worktree
    };
    t(
        "events.line",
        &[
            ("time", &local_time(e.observed_utc_ms, e.utc_offset_s)),
            ("worktree", &worktree),
            ("event", &describe(e)),
            ("actor", &actor_of(e)),
        ],
    )
}

/// A gap of one repo, as `raptor events` lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GapEntry {
    pub repo_id: String,
    pub gap: GapView,
}

/// One line of `raptor events`: a Git event or a gap.
#[derive(Debug, Clone, Copy)]
pub enum Entry<'a> {
    Event(&'a GitEventView),
    Gap(&'a GapEntry),
}

impl Entry<'_> {
    /// Events by their time; a gap by its end (its start while it is open),
    /// before the events it found, which carry that same time.
    fn order(&self) -> (i64, u8, i64) {
        match self {
            Entry::Event(e) => (e.observed_utc_ms, 1, e.seq),
            Entry::Gap(g) => (g.gap.ended_utc_ms.unwrap_or(g.gap.started_utc_ms), 0, 0),
        }
    }
}

/// Events and gaps in one list, oldest first.
pub fn interleave<'a>(events: &'a [GitEventView], gaps: &'a [GapEntry]) -> Vec<Entry<'a>> {
    let mut all: Vec<Entry<'a>> = events
        .iter()
        .map(Entry::Event)
        .chain(gaps.iter().map(Entry::Gap))
        .collect();
    all.sort_by_key(Entry::order);
    all
}

/// The text output with the gaps in between. `offset_s` is the local offset
/// the gap times are shown at: a gap carries only UTC.
pub fn text_with_gaps(entries: &[Entry<'_>], offset_s: i32) -> String {
    let mut out = String::new();
    if entries.is_empty() {
        let _ = writeln!(out, "{}", t("events.none", &[]));
    }
    for entry in entries {
        let line = match entry {
            Entry::Event(e) => event_line(e),
            Entry::Gap(g) => gap_line(&g.gap, offset_s, Tr::current()),
        };
        let _ = writeln!(out, "{line}");
    }
    out
}

/// "-- observation gap (engine stopped), 12:00 → 13:00 · unattributed". What
/// the gap found is the net state, never the full history, and is never
/// attributed to an agent (BR-CONS-003).
fn gap_line(gap: &GapView, offset_s: i32, tr: Tr) -> String {
    let cause = tr.t(&format!("gap.cause.{}", wire(&gap.cause)), &[]);
    let start = local_time(gap.started_utc_ms, offset_s);
    let mut out = match gap.ended_utc_ms {
        Some(end) => {
            let end = local_time(end, offset_s);
            // The time alone when both ends fall on the same local day.
            let (from, to) = if start[..10] == end[..10] {
                (start[11..16].to_owned(), end[11..16].to_owned())
            } else {
                (start[..16].to_owned(), end[..16].to_owned())
            };
            tr.t(
                "events.gap_line",
                &[("cause", &cause), ("from", &from), ("to", &to)],
            )
        }
        None => tr.t(
            "events.gap_open",
            &[("cause", &cause), ("from", &start[11..16].to_owned())],
        ),
    };
    if gap.during_session {
        out.push_str(" · ");
        out.push_str(&tr.t("events.gap_during_session", &[]));
    }
    out
}

fn describe(e: &GitEventView) -> String {
    if let Some(authored) = authored(e) {
        return authored;
    }
    let show =
        |u: &Option<UntrustedName>| u.as_ref().map(UntrustedName::sanitized).unwrap_or_default();
    let branch = show(&e.details.branch);
    match e.kind {
        GitEventKind::BranchSwitch if e.details.from.is_some() => t(
            "event.branch-switch",
            &[("from", &show(&e.details.from)), ("branch", &branch)],
        ),
        GitEventKind::BranchSwitch => t("event.branch-switch-to", &[("branch", &branch)]),
        GitEventKind::Reset if e.details.branch.is_none() => t("event.reset-detached", &[]),
        kind => t(&format!("event.{}", kind.as_str()), &[("branch", &branch)]),
    }
}

/// Who a commit went in under and who ran it, when they differ (US-GRD-019,
/// amendment § 3 of ADR-GRP-012): "commit by Ana with Claude Code · feat-x",
/// plus "run by …" when the actor is an agent the trailers do not name, and
/// "no trailer" when no agent's trailer is there.
fn authored(e: &GitEventView) -> Option<String> {
    let a = e.authorship.as_ref()?;
    let key = match e.kind {
        GitEventKind::Commit => "events.commit_by",
        GitEventKind::Merge => "events.merge_by",
        _ => return None,
    };
    let mut agents: Vec<_> = Vec::new();
    for kind in a.coauthors.iter().filter_map(|c| c.agent) {
        if !agents.contains(&kind) {
            agents.push(kind);
        }
    }
    let mut out = t(key, &[("author", &a.author.name.sanitized())]);
    if !agents.is_empty() {
        let names: Vec<String> = agents
            .iter()
            .map(|k| t(&format!("actor.{}", wire(k)), &[]))
            .collect();
        out.push(' ');
        out.push_str(&t("events.commit_with", &[("agents", &names.join(", "))]));
    }
    let worktree = e.worktree.sanitized();
    let name = std::path::Path::new(&worktree)
        .file_name()
        .map_or(worktree.clone(), |n| n.to_string_lossy().into_owned());
    out.push_str(" · ");
    out.push_str(&name);
    // The branch stays, as in the plain line: "· feat-x (main)".
    if let Some(branch) = &e.details.branch {
        out.push_str(&format!(" ({})", branch.sanitized()));
    }
    if let Actor::Agent { kind, .. } = &e.actor
        && !agents.contains(kind)
    {
        out.push_str(" · ");
        out.push_str(&t("events.run_by", &[("agent", &actor_name(&e.actor))]));
        if agents.is_empty() {
            out.push_str(" · ");
            out.push_str(&t("events.no_trailer", &[]));
        }
    }
    Some(out)
}

/// The event's actor, or for an unattributed one with a hint, the agent it
/// is inferred to come from (amendment of ADR-GRP-012), checked against the
/// commit's trailers when the engine did (US-GRD-019).
fn actor_of(e: &GitEventView) -> String {
    match (&e.actor, &e.inferred) {
        (Actor::Unattributed, Some(hint)) => inferred_actor(hint),
        (actor_, _) => actor(actor_),
    }
}

/// "no agent; inferred: Claude Code (confirmed by the trailer)": an event
/// without an agent and the agent it is inferred to come from. The same text
/// in `raptor events` and `raptor timeline`.
pub(crate) fn inferred_actor(hint: &InferredAgent) -> String {
    let agent = t(&format!("actor.{}", wire(&hint.kind)), &[]);
    let agent = match hint.trailer {
        Some(TrailerCheck::Confirmed) => {
            format!("{agent} ({})", t("events.inferred_confirmed", &[]))
        }
        Some(TrailerCheck::Unconfirmed) => {
            format!("{agent} ({})", t("events.inferred_unconfirmed", &[]))
        }
        None => agent,
    };
    t("events.no_agent_inferred", &[("agent", &agent)])
}

/// The actor with its origin (US-GRP-007): "Claude Code, detected", or
/// "no agent" (the wire value stays `unattributed`; the text says what the
/// Cockpit says, so a person's own commit does not read as a failure).
pub(crate) fn actor(actor: &Actor) -> String {
    match actor {
        Actor::Unattributed => t("events.no_agent", &[]),
        Actor::Agent { origin, .. } => t(
            "actor.with-origin",
            &[
                ("origin", &crate::sessions::origin_text(*origin)),
                ("agent", &actor_name(actor)),
            ],
        ),
    }
}

/// Who the actor is, without its origin.
pub fn actor_name(actor: &Actor) -> String {
    match actor {
        Actor::Unattributed => t("actor.unattributed", &[]),
        Actor::Agent { kind, name, .. } => match name {
            Some(name) => t("actor.other-agent", &[("name", &name.sanitized())]),
            None => t(&format!("actor.{}", wire(kind)), &[]),
        },
    }
}

/// The JSON output, a stable, flat shape with plain strings, with the gaps in between: a gap is
/// `{"kind":"gap", ...}`, its `cause` the wire text, never translated.
pub fn json_with_gaps(entries: &[Entry<'_>]) -> Value {
    Value::Array(
        entries
            .iter()
            .map(|entry| match entry {
                Entry::Event(e) => event_json(e),
                Entry::Gap(g) => json!({
                    "kind": "gap",
                    "repo_id": g.repo_id,
                    "gap_id": g.gap.gap_id,
                    "cause": wire(&g.gap.cause),
                    "started_utc_ms": g.gap.started_utc_ms,
                    "ended_utc_ms": g.gap.ended_utc_ms,
                    "during_session": g.gap.during_session,
                    "actor": "unattributed",
                }),
            })
            .collect(),
    )
}

fn event_json(e: &GitEventView) -> Value {
    let plain = |u: &Option<UntrustedName>| u.as_ref().map(|u| u.raw().to_owned());
    let mut entry = json!({
        "repo_id": e.repo_id,
        "seq": e.seq,
        "worktree": e.worktree.raw(),
        "worktree_inferred": e.details.worktree_inferred,
        "kind": e.kind.as_str(),
        "actor": match &e.actor {
            Actor::Unattributed => json!("unattributed"),
            other => serde_json::to_value(other).unwrap_or(Value::Null),
        },
        "observed_utc_ms": e.observed_utc_ms,
        "utc_offset_s": e.utc_offset_s,
        "branch": plain(&e.details.branch),
        "from": plain(&e.details.from),
        "old_commit": e.details.old_commit,
        "new_commit": e.details.new_commit,
        "gap_id": e.gap_id,
        "inferred": e.inferred,
    });
    // Who it went in under, apart from who ran it (US-GRD-019); absent for
    // events stored before it was recorded.
    if let Some(a) = &e.authorship {
        let who = |name: &gitraptor_api::Untrusted, email: &gitraptor_api::Untrusted| json!({"name": name.raw(), "email": email.raw()});
        entry["authorship"] = json!({
            "author": who(&a.author.name, &a.author.email),
            "committer": who(&a.committer.name, &a.committer.email),
            "coauthors": a.coauthors.iter().map(|c| {
                let mut co = who(&c.name, &c.email);
                co["agent"] = c.agent.map_or(Value::Null, |k| json!(wire(&k)));
                co
            }).collect::<Vec<_>>(),
        });
    }
    entry
}

/// `YYYY-MM-DD HH:MM:SS` at the event's own offset.
pub fn local_time(utc_ms: i64, offset_s: i32) -> String {
    let secs = utc_ms.div_euclid(1000) + i64::from(offset_s);
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitraptor_api::messages::GitEventDetails;

    fn text(events: &[GitEventView]) -> String {
        text_with_gaps(&interleave(events, &[]), 0)
    }

    fn json(events: &[GitEventView]) -> Value {
        json_with_gaps(&interleave(events, &[]))
    }

    fn event(kind: GitEventKind, inferred: bool) -> GitEventView {
        GitEventView {
            repo_id: "r".into(),
            seq: 1,
            worktree: gitraptor_api::Untrusted::new("/w/feat-login\u{1b}[31m"),
            kind,
            actor: Actor::Unattributed,
            observed_utc_ms: 1_791_148_066_018,
            utc_offset_s: 7200,
            details: GitEventDetails {
                branch: Some(UntrustedName::new("feat-login")),
                worktree_inferred: inferred,
                ..GitEventDetails::default()
            },
            gap_id: None,
            inferred: None,
            authorship: None,
        }
    }

    #[test]
    fn local_time_uses_the_offset() {
        assert_eq!(local_time(0, 0), "1970-01-01 00:00:00");
        assert_eq!(local_time(1_791_148_066_018, 0), "2026-10-04 21:07:46");
        assert_eq!(
            local_time(1_791_148_066_018, -5 * 3600),
            "2026-10-04 16:07:46"
        );
        assert_eq!(local_time(951_782_400_000, 0), "2000-02-29 00:00:00");
    }

    #[test]
    fn text_is_sanitized_and_marks_the_inferred_worktree() {
        let out = text(&[event(GitEventKind::Commit, false)]);
        assert!(!out.contains('\u{1b}'), "{out}");
        assert!(out.contains("commit on feat-login"), "{out}");
        assert!(out.contains("(no agent)"), "{out}");
        assert!(!out.contains("unattributed"), "{out}");
        let out = text(&[event(GitEventKind::BranchDelete, true)]);
        assert!(out.contains("(inferred)"), "{out}");
    }

    #[test]
    fn every_kind_has_a_message() {
        for kind in GitEventKind::ALL {
            let shown = describe(&event(kind, false));
            assert!(!shown.starts_with("event."), "{kind:?}: {shown}");
        }
    }

    #[test]
    fn an_agent_is_shown_with_its_origin() {
        let mut e = event(GitEventKind::Commit, false);
        e.actor = Actor::Agent {
            kind: gitraptor_api::AgentKind::ClaudeCode,
            name: None,
            origin: gitraptor_api::AgentOrigin::Detected,
        };
        assert!(text(&[e]).contains("(Claude Code, detected)"));
    }

    #[test]
    fn json_is_flat() {
        let value = json(&[event(GitEventKind::Push, false)]);
        assert_eq!(value[0]["kind"], "push");
        assert_eq!(value[0]["actor"], "unattributed");
        assert_eq!(value[0]["branch"], "feat-login");
        assert_eq!(value[0]["worktree_inferred"], false);
    }

    /// Amendment of ADR-GRP-012: an event without an agent but with a hint
    /// shows the inferred agent, and still says "no agent"; `--json` keeps
    /// the wire value `unattributed`.
    #[test]
    fn an_inferred_event_says_so() {
        let mut e = event(GitEventKind::Commit, false);
        e.inferred = Some(gitraptor_api::messages::InferredAgent {
            trailer: None,
            kind: gitraptor_api::AgentKind::ClaudeCode,
            session_id: "20:2000".into(),
        });
        let out = text(std::slice::from_ref(&e));
        assert!(out.contains("(no agent; inferred: Claude Code)"), "{out}");
        let value = json(&[e]);
        assert_eq!(value[0]["actor"], "unattributed");
        assert_eq!(value[0]["inferred"]["kind"], "claude-code");
        assert_eq!(value[0]["inferred"]["session_id"], "20:2000");
    }

    /// US-GRD-019 (ajuste 3): an event stored before the declared
    /// authorship existed reads exactly as before, and `--json` has no
    /// `authorship` for it.
    #[test]
    fn an_older_event_reads_as_before() {
        let e = event(GitEventKind::Commit, false);
        let out = text(std::slice::from_ref(&e));
        assert!(out.contains("commit on feat-login  (no agent)"), "{out}");
        let value = json(&[e]);
        assert!(value[0].get("authorship").is_none(), "{value}");
    }

    /// US-GRD-019: the declared authorship is flat plain strings in
    /// `--json`, and the text names author, agents and worktree, sanitized.
    #[test]
    fn an_authored_commit_shows_the_person_and_the_agent() {
        use gitraptor_api::messages::{CoAuthor, DeclaredAuthorship, GitIdentity};
        use gitraptor_api::{AgentKind, Untrusted};
        let mut e = event(GitEventKind::Commit, false);
        let ana = || GitIdentity {
            name: Untrusted::new("Ana\u{1b}[31m Pérez"),
            email: Untrusted::new("ana@example.com"),
        };
        e.authorship = Some(DeclaredAuthorship {
            author: ana(),
            committer: ana(),
            coauthors: vec![CoAuthor {
                name: Untrusted::new("Claude"),
                email: Untrusted::new("noreply@anthropic.com"),
                agent: Some(AgentKind::ClaudeCode),
            }],
        });
        let out = text(std::slice::from_ref(&e));
        assert!(!out.contains('\u{1b}'), "{out:?}");
        assert!(
            out.contains("with Claude Code · feat-login (feat-login)"),
            "{out}"
        );
        let value = json(&[e]);
        assert_eq!(value[0]["authorship"]["author"]["email"], "ana@example.com");
        assert_eq!(
            value[0]["authorship"]["coauthors"][0]["agent"],
            "claude-code"
        );
        assert_eq!(value[0]["actor"], "unattributed");
    }

    fn gap(cause: gitraptor_api::methods::GapCauseView, from: i64, to: Option<i64>) -> GapEntry {
        GapEntry {
            repo_id: "r".into(),
            gap: GapView {
                gap_id: "g-1".into(),
                started_utc_ms: from,
                ended_utc_ms: to,
                cause,
                during_session: false,
            },
        }
    }

    const EN: Tr = Tr { spanish: false };
    const ES: Tr = Tr { spanish: true };
    const NOON: i64 = 1_791_201_600_000; // 2026-10-05 12:00:00 UTC
    const HOUR: i64 = 3_600_000;

    #[test]
    fn a_gap_reads_as_an_unattributed_observation_gap_in_en_and_es() {
        use gitraptor_api::methods::GapCauseView as C;
        let g = gap(C::DaemonStopped, NOON, Some(NOON + HOUR));
        let line = gap_line(&g.gap, 0, EN);
        assert_eq!(
            line,
            "-- observation gap (engine stopped), 12:00 → 13:00 · unattributed"
        );
        let es = gap_line(&g.gap, 0, ES);
        assert_eq!(
            es,
            "-- hueco de observación (motor detenido), 12:00 → 13:00 · sin atribuir"
        );
        // Every cause has a text in both languages, and none says "caída".
        for c in [
            C::MachineOff,
            C::DaemonDown,
            C::DaemonDownDuringSession,
            C::DaemonStopped,
            C::RepoRetired,
            C::GitUnavailable,
            C::ProfileLost,
            C::StoreCorrupt,
            C::WatcherOverflow,
            C::StreamRecreated,
            C::PeriodicReconciliation,
            C::Dormant,
        ] {
            let key = format!("gap.cause.{}", wire(&c));
            for tr in [EN, ES] {
                let shown = tr.t(&key, &[]);
                assert!(!shown.starts_with("gap.cause."), "{key}");
                assert!(!shown.to_lowercase().contains("caída"), "{key}");
            }
        }
        assert_eq!(ES.t("gap.cause.daemon-down", &[]), "motor detenido");
    }

    #[test]
    fn an_open_gap_reads_still_open() {
        use gitraptor_api::methods::GapCauseView as C;
        let g = gap(C::DaemonDown, NOON, None);
        assert_eq!(
            gap_line(&g.gap, 0, EN),
            "-- observation gap (engine stopped), since 12:00 (still open) · unattributed"
        );
    }

    #[test]
    fn a_gap_sorts_before_the_events_it_found() {
        use gitraptor_api::methods::GapCauseView as C;
        let end = NOON + HOUR;
        let mut found = event(GitEventKind::Commit, false);
        found.observed_utc_ms = end;
        let mut earlier = event(GitEventKind::Push, false);
        earlier.observed_utc_ms = NOON - HOUR;
        let events = [found, earlier];
        let gaps = [gap(C::DaemonStopped, NOON, Some(end))];
        let entries = interleave(&events, &gaps);
        let kinds: Vec<&str> = entries
            .iter()
            .map(|e| match e {
                Entry::Event(e) => e.kind.as_str(),
                Entry::Gap(_) => "gap",
            })
            .collect();
        assert_eq!(kinds, ["push", "gap", "commit"]);
        let out = text_with_gaps(&entries, 0);
        assert_eq!(out.lines().count(), 3, "{out}");
    }

    #[test]
    fn a_gap_during_a_session_says_so() {
        use gitraptor_api::methods::GapCauseView as C;
        let mut g = gap(C::DaemonDownDuringSession, NOON, Some(NOON + HOUR));
        g.gap.during_session = true;
        assert_eq!(
            gap_line(&g.gap, 0, EN),
            "-- observation gap (engine stopped), 12:00 → 13:00 · unattributed · \
             an agent session was active"
        );
    }

    #[test]
    fn a_gap_in_json_is_an_unattributed_gap_entry() {
        use gitraptor_api::methods::GapCauseView as C;
        let g = gap(C::DaemonDownDuringSession, NOON, Some(NOON + HOUR));
        let value = json_with_gaps(&[Entry::Gap(&g)]);
        assert_eq!(
            value[0],
            json!({
                "kind": "gap",
                "repo_id": "r",
                "gap_id": "g-1",
                "cause": "daemon-down-during-session",
                "started_utc_ms": NOON,
                "ended_utc_ms": NOON + HOUR,
                "during_session": false,
                "actor": "unattributed",
            })
        );
        let open = gap(C::DaemonDown, NOON, None);
        assert_eq!(
            json_with_gaps(&[Entry::Gap(&open)])[0]["ended_utc_ms"],
            Value::Null
        );
    }
}
