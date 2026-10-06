//! Sample data of every component and state. The gallery shows them and the snapshot tests
//! pin them, so what a reviewer sees is what CI checks (TS-CKP-005).
//!
//! Gallery data is English only: the gallery is a hidden developer tool, not a user surface
//! (decision validated by the PO, 2026-10-05).

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::model::SafeText;
use crate::tui::style::{Styles, modal_area};
use crate::tui::widgets::Component;
use crate::tui::widgets::agent_list::{
    AgentColumns, AgentListModel, AgentRowModel, AgentState, Branch, Operation, Sync,
};
use crate::tui::widgets::confirm::{Choice, ConfirmModel};
use crate::tui::widgets::conflict_alert::{AgentRef, ConflictAlertModel, Prediction};
use crate::tui::widgets::diff_view::{DiffBody, DiffLine, DiffLineKind, DiffModel};
use crate::tui::widgets::graph_lanes::{Base, GraphModel, LaneModel, LaneOwner};
use crate::tui::widgets::key_hints::{HelpGroup, HelpModel, KeyHint, KeyHintsModel};
use crate::tui::widgets::layout::{BodyPlan, Connection, StatusBarModel, TooSmallModel, layout};
use crate::tui::widgets::notification::{ToastKind, ToastModel, ToastStackModel};
use crate::tui::widgets::policy_banner::{PolicyBannerModel, PolicyKind};
use crate::tui::widgets::timeline::{TimelineEntry, TimelineModel, TimelineNotice};

/// One component in one state, at the size it is shown and pinned.
pub struct Story {
    pub component: &'static str,
    pub state: &'static str,
    pub width: u16,
    pub height: u16,
    pub paint: fn(Rect, &mut Buffer, &Styles),
}

impl Story {
    pub fn render(&self, styles: &Styles) -> Buffer {
        let area = Rect::new(0, 0, self.width, self.height);
        let mut buf = Buffer::empty(area);
        (self.paint)(area, &mut buf, styles);
        buf
    }
}

fn t(text: &str) -> SafeText {
    SafeText::text(text)
}

fn hint(keys: &'static str, label: &'static str) -> KeyHint {
    KeyHint::new(t(keys), t(label))
}

// --- StatusBar -----------------------------------------------------------------------------

fn status_bar(
    connection: Connection,
    label: &'static str,
    requester: &'static str,
) -> StatusBarModel {
    StatusBarModel {
        repo: t("gitraptor"),
        connection,
        connection_label: t(label),
        protection: Some(t("main protected")),
        requester: t(requester),
        conflicts: 2,
        blocked: 1,
    }
}

// --- AgentList -----------------------------------------------------------------------------

fn columns() -> AgentColumns {
    AgentColumns {
        agent: t("Agent"),
        branch: t("Branch"),
        changes: t("Files"),
        sync: t("Sync"),
        activity: t("Activity"),
    }
}

fn row(i: usize, name: &'static str, state: AgentState, branch: &'static str) -> AgentRowModel {
    AgentRowModel {
        color_index: i,
        name: t(name),
        state,
        state_label: t("unavailable"),
        branch: Branch::Named(t(branch)),
        changes: 3,
        sync: Sync::Known {
            ahead: 2,
            behind: 0,
        },
        activity: t("2 min ago"),
        conflict: false,
        blocked: false,
        operation: None,
    }
}

fn agent_list(rows: Vec<AgentRowModel>, selected: Option<usize>, focused: bool) -> AgentListModel {
    AgentListModel {
        title: t("Agents"),
        columns: columns(),
        rows,
        selected,
        offset: 0,
        focused,
        empty: t("No agents yet. Start one in a worktree of this repo."),
    }
}

fn mixed_rows() -> Vec<AgentRowModel> {
    let mut a = row(0, "claude-1", AgentState::Active, "feat/parser");
    a.conflict = true;
    let mut b = row(1, "claude-2", AgentState::Idle, "feat/lexer");
    b.changes = 0;
    b.sync = Sync::Known {
        ahead: 5,
        behind: 3,
    };
    b.activity = t("14 min ago");
    let mut c = row(2, "claude-3", AgentState::Done, "fix/flaky-test");
    c.blocked = true;
    c.activity = t("1 h ago");
    let mut d = row(
        3,
        "claude-4-with-a-very-long-session-name",
        AgentState::Active,
        "",
    );
    d.branch = Branch::Detached(t("HEAD at a1b2c3d"));
    d.sync = Sync::Unknown(t("unknown"));
    vec![a, b, c, d]
}

fn agents_mixed(area: Rect, buf: &mut Buffer, s: &Styles) {
    agent_list(mixed_rows(), Some(0), true).render(area, buf, s);
}

fn agents_ninth(area: Rect, buf: &mut Buffer, s: &Styles) {
    const NAMES: [&str; 9] = [
        "claude-1", "claude-2", "claude-3", "claude-4", "claude-5", "claude-6", "claude-7",
        "claude-8", "claude-9",
    ];
    let rows = NAMES
        .iter()
        .enumerate()
        .map(|(i, n)| row(i, n, AgentState::Active, "feat/work"))
        .collect();
    agent_list(rows, Some(8), true).render(area, buf, s);
}

fn agents_operation(area: Rect, buf: &mut Buffer, s: &Styles) {
    let mut a = row(0, "claude-1", AgentState::Active, "feat/parser");
    a.operation = Some(Operation {
        label: t("rebasing onto main"),
        elapsed: t("0:42"),
        cancel: hint("c", "cancel"),
    });
    let b = row(1, "claude-2", AgentState::Idle, "feat/lexer");
    agent_list(vec![a, b], Some(0), true).render(area, buf, s);
}

fn agents_unavailable(area: Rect, buf: &mut Buffer, s: &Styles) {
    let a = row(0, "claude-1", AgentState::Active, "feat/parser");
    let b = row(1, "claude-2", AgentState::Unavailable, "feat/lexer");
    agent_list(vec![a, b], Some(1), true).render(area, buf, s);
}

fn agents_empty(area: Rect, buf: &mut Buffer, s: &Styles) {
    agent_list(Vec::new(), None, true).render(area, buf, s);
}

fn agents_unfocused(area: Rect, buf: &mut Buffer, s: &Styles) {
    let mut m = agent_list(mixed_rows(), Some(3), false);
    m.offset = crate::tui::style::follow(0, 3, 3);
    m.render(area, buf, s);
}

// --- GraphLanes ----------------------------------------------------------------------------

fn lane(i: usize, name: &'static str, ahead: u32, behind: u32) -> LaneModel {
    LaneModel {
        owner: LaneOwner::Agent {
            color_index: i,
            name: t(name),
        },
        ahead,
        behind,
        conflict: false,
    }
}

fn graph(base: Base, lanes: Vec<LaneModel>) -> GraphModel {
    GraphModel {
        title: t("Branches"),
        base,
        lanes,
        more: t("more"),
        focused: false,
    }
}

fn graph_three(area: Rect, buf: &mut Buffer, s: &Styles) {
    let mut a = lane(0, "claude-1", 3, 1);
    a.conflict = true;
    let lanes = vec![a, lane(1, "claude-2", 2, 0), lane(2, "claude-3", 9, 4)];
    let mut m = graph(Base::Found(t("main")), lanes);
    m.focused = true;
    m.render(area, buf, s);
}

fn graph_collapsed(area: Rect, buf: &mut Buffer, s: &Styles) {
    let lanes = (0..6)
        .map(|i| {
            lane(
                i,
                [
                    "claude-1", "claude-2", "claude-3", "claude-4", "claude-5", "claude-6",
                ][i],
                2,
                0,
            )
        })
        .collect();
    graph(Base::Found(t("main")), lanes).render(area, buf, s);
}

fn graph_unattributed(area: Rect, buf: &mut Buffer, s: &Styles) {
    let lanes = vec![
        lane(0, "claude-1", 2, 0),
        LaneModel {
            owner: LaneOwner::Unattributed(t("unattributed (hotfix/login)")),
            ahead: 1,
            behind: 0,
            conflict: false,
        },
    ];
    graph(Base::Found(t("main")), lanes).render(area, buf, s);
}

fn graph_base_missing(area: Rect, buf: &mut Buffer, s: &Styles) {
    let lanes = vec![lane(0, "claude-1", 2, 0), lane(1, "claude-2", 1, 0)];
    graph(Base::Missing(t("Base branch main not found")), lanes).render(area, buf, s);
}

// --- DiffView ------------------------------------------------------------------------------

fn diff_line(
    kind: DiffLineKind,
    old: Option<u32>,
    new: Option<u32>,
    text: &'static str,
) -> DiffLine {
    DiffLine {
        kind,
        old,
        new,
        text: t(text),
    }
}

fn diff_text(area: Rect, buf: &mut Buffer, s: &Styles) {
    use DiffLineKind::*;
    DiffModel {
        title: t("src/parser.rs"),
        stats: Some((3, 1)),
        body: DiffBody::Lines(vec![
            diff_line(Hunk, None, None, "@@ -10,4 +10,6 @@ fn parse(input: &str)"),
            diff_line(
                Context,
                Some(10),
                Some(10),
                "    let mut tokens = lex(input);",
            ),
            diff_line(Removed, Some(11), None, "    let ast = build(tokens);"),
            diff_line(Added, None, Some(11), "    let ast = build(&mut tokens)?;"),
            diff_line(Added, None, Some(12), "    validate(&ast)?;"),
            diff_line(Added, None, Some(13), "    Ok(ast)"),
            diff_line(Context, Some(12), Some(14), "}"),
        ]),
        offset: 0,
        focused: true,
    }
    .render(area, buf, s);
}

fn diff_binary(area: Rect, buf: &mut Buffer, s: &Styles) {
    DiffModel {
        title: t("assets/logo.png"),
        stats: None,
        body: DiffBody::Binary(t("Binary file changed (12.4 KB to 13.1 KB)")),
        offset: 0,
        focused: false,
    }
    .render(area, buf, s);
}

fn diff_empty(area: Rect, buf: &mut Buffer, s: &Styles) {
    DiffModel {
        title: t("src/lexer.rs"),
        stats: None,
        body: DiffBody::Empty(t("No changes in this file")),
        offset: 0,
        focused: false,
    }
    .render(area, buf, s);
}

// --- TimelineList --------------------------------------------------------------------------

fn entry(
    when: &'static str,
    actor: &'static str,
    color: Option<usize>,
    summary: &'static str,
    undoable: bool,
) -> TimelineEntry {
    TimelineEntry {
        when: t(when),
        actor: t(actor),
        actor_color: color,
        summary: t(summary),
        undoable,
    }
}

fn timeline(
    entries: Vec<TimelineEntry>,
    notice: Option<TimelineNotice>,
    actions: Vec<KeyHint>,
) -> TimelineModel {
    TimelineModel {
        title: t("Timeline"),
        selected: (!entries.is_empty()).then_some(0),
        entries,
        offset: 0,
        focused: true,
        empty: t("No snapshots yet."),
        notice,
        actions,
    }
}

fn timeline_entries_list() -> Vec<TimelineEntry> {
    vec![
        entry(
            "14:02",
            "claude-1",
            Some(0),
            "rebase feat/parser onto main",
            true,
        ),
        entry(
            "13:58",
            "claude-2",
            Some(1),
            "commit \"lexer: handle tabs\"",
            true,
        ),
        entry("13:40", "you", None, "merge feat/lexer", true),
        entry(
            "13:12",
            "claude-3",
            Some(2),
            "reset --hard (blocked)",
            false,
        ),
    ]
}

fn timeline_entries(area: Rect, buf: &mut Buffer, s: &Styles) {
    timeline(
        timeline_entries_list(),
        None,
        vec![hint("u", "undo"), hint("r", "restore")],
    )
    .render(area, buf, s);
}

fn timeline_empty(area: Rect, buf: &mut Buffer, s: &Styles) {
    timeline(Vec::new(), None, Vec::new()).render(area, buf, s);
}

fn timeline_purge(area: Rect, buf: &mut Buffer, s: &Styles) {
    let notice = TimelineNotice::Purge(t("Snapshots older than 30 days are purged tonight"));
    timeline(
        timeline_entries_list(),
        Some(notice),
        vec![hint("u", "undo"), hint("r", "restore")],
    )
    .render(area, buf, s);
}

fn timeline_undo_disabled(area: Rect, buf: &mut Buffer, s: &Styles) {
    let notice = TimelineNotice::UndoDisabled(t("Undo is off while a rebase is in progress"));
    let actions = vec![
        hint("u", "undo").disabled(t("rebase in progress")),
        hint("r", "restore"),
    ];
    timeline(timeline_entries_list(), Some(notice), actions).render(area, buf, s);
}

// --- ConflictAlert -------------------------------------------------------------------------

fn alert(prediction: Prediction, status: &'static str) -> ConflictAlertModel {
    ConflictAlertModel {
        title: t("Conflict predicted"),
        left: AgentRef {
            color_index: 0,
            name: t("claude-1"),
        },
        right: AgentRef {
            color_index: 1,
            name: t("claude-2"),
        },
        files: vec![t("src/parser.rs"), t("src/lexer.rs")],
        more: t("more files"),
        prediction,
        status: t(status),
        new: None,
        focused: false,
    }
}

fn alert_new(area: Rect, buf: &mut Buffer, s: &Styles) {
    let mut m = alert(Prediction::Current, "just now");
    m.new = Some(t("NEW"));
    m.focused = true;
    m.render(area, buf, s);
}

fn alert_current(area: Rect, buf: &mut Buffer, s: &Styles) {
    alert(Prediction::Current, "updated 3 min ago").render(area, buf, s);
}

fn alert_stale(area: Rect, buf: &mut Buffer, s: &Styles) {
    alert(Prediction::Stale, "outdated, recalculating").render(area, buf, s);
}

fn alert_calculating(area: Rect, buf: &mut Buffer, s: &Styles) {
    alert(Prediction::Calculating, "calculating").render(area, buf, s);
}

fn alert_pending(area: Rect, buf: &mut Buffer, s: &Styles) {
    alert(
        Prediction::PendingBase,
        "pending: the base branch is not confirmed",
    )
    .render(area, buf, s);
}

fn alert_not_computable(area: Rect, buf: &mut Buffer, s: &Styles) {
    alert(
        Prediction::NotComputable,
        "not computable: the merge base is missing",
    )
    .render(area, buf, s);
}

fn alert_many_files(area: Rect, buf: &mut Buffer, s: &Styles) {
    let mut m = alert(Prediction::Current, "updated 1 min ago");
    m.files = vec![
        t("src/parser.rs"),
        t("src/lexer.rs"),
        t("src/ast.rs"),
        t("src/eval.rs"),
        t("tests/parse.rs"),
    ];
    m.render(area, buf, s);
}

// --- PolicyBanner --------------------------------------------------------------------------

fn banner(kind: PolicyKind) -> PolicyBannerModel {
    PolicyBannerModel {
        kind,
        title: t("Push blocked"),
        rule_label: t("Rule"),
        rule: t("protect-main"),
        reason: t("claude-1 tried to push to main, which only accepts merges through a PR."),
        alternative: Some(t("Create a branch: raptor branch new <name>")),
        countdown: None,
        actions: vec![hint("Esc", "close")],
    }
}

fn modal(m: &dyn Component, area: Rect, buf: &mut Buffer, s: &Styles) {
    let h = m.height(64);
    m.render(modal_area(area, 64, h), buf, s);
}

fn banner_blocked(area: Rect, buf: &mut Buffer, s: &Styles) {
    modal(&banner(PolicyKind::Blocked), area, buf, s);
}

fn banner_pending(area: Rect, buf: &mut Buffer, s: &Styles) {
    let mut m = banner(PolicyKind::Pending);
    m.title = t("Approval needed");
    m.rule = t("no-force-push");
    m.reason = t("claude-2 asks to force-push feat/lexer.");
    m.alternative = None;
    m.countdown = Some(t("Expires in 4:32"));
    m.actions = vec![
        hint("a", "approve"),
        hint("d", "deny"),
        hint("Esc", "later"),
    ];
    modal(&m, area, buf, s);
}

fn banner_expired(area: Rect, buf: &mut Buffer, s: &Styles) {
    let mut m = banner(PolicyKind::Expired);
    m.title = t("Request expired");
    m.rule = t("no-force-push");
    m.reason = t("The request of claude-2 expired without an answer. Nothing was done.");
    m.alternative = None;
    m.actions = Vec::new();
    modal(&m, area, buf, s);
}

// --- ConfirmPrompt -------------------------------------------------------------------------

fn confirm() -> ConfirmModel {
    let mut m = ConfirmModel::new(
        t("Discard worktree"),
        t("Discard the worktree of claude-3? This cannot be undone."),
        t("Discard worktree"),
        t("No"),
    );
    m.losses = vec![
        t("2 files with uncommitted changes"),
        t("1 commit not in any other branch"),
    ];
    m
}

fn confirm_default(area: Rect, buf: &mut Buffer, s: &Styles) {
    modal(&confirm(), area, buf, s);
}

fn confirm_yes(area: Rect, buf: &mut Buffer, s: &Styles) {
    let mut m = confirm();
    m.choice = Choice::Yes;
    modal(&m, area, buf, s);
}

fn confirm_recovery(area: Rect, buf: &mut Buffer, s: &Styles) {
    let mut m = ConfirmModel::new(
        t("Rebase"),
        t("Rebase feat/parser of claude-1 onto main?"),
        t("Rebase"),
        t("No"),
    );
    m.recovery = Some(t("A snapshot is taken first: u undoes it"));
    modal(&m, area, buf, s);
}

// --- Notification --------------------------------------------------------------------------

fn toasts(list: Vec<ToastModel>, area: Rect, buf: &mut Buffer, s: &Styles) {
    let m = ToastStackModel { toasts: list };
    let at = m.area(area, s);
    m.render(at, buf, s);
}

fn toast(kind: ToastKind, message: &'static str, action: Option<KeyHint>) -> ToastModel {
    ToastModel {
        kind,
        message: t(message),
        action,
    }
}

fn toast_success(area: Rect, buf: &mut Buffer, s: &Styles) {
    toasts(
        vec![toast(
            ToastKind::Success,
            "Rebased claude-1 onto main",
            Some(hint("u", "undo")),
        )],
        area,
        buf,
        s,
    );
}

fn toast_error(area: Rect, buf: &mut Buffer, s: &Styles) {
    toasts(
        vec![toast(
            ToastKind::Error,
            "Rebase stopped: conflict in src/parser.rs",
            None,
        )],
        area,
        buf,
        s,
    );
}

fn toast_stack(area: Rect, buf: &mut Buffer, s: &Styles) {
    toasts(
        vec![
            toast(ToastKind::Info, "Prediction updated", None),
            toast(
                ToastKind::Warning,
                "claude-2 is 3 commits behind main",
                None,
            ),
            toast(
                ToastKind::Success,
                "Merged feat/lexer",
                Some(hint("u", "undo")),
            ),
        ],
        area,
        buf,
        s,
    );
}

// --- KeyHints / Help -----------------------------------------------------------------------

fn hints(list: Vec<KeyHint>) -> KeyHintsModel {
    KeyHintsModel {
        hints: list,
        help: hint("?", "help"),
    }
}

fn hints_bar(area: Rect, buf: &mut Buffer, s: &Styles) {
    hints(vec![
        hint("j/k", "move"),
        hint("Enter", "open"),
        hint("d", "diff"),
        hint("q", "quit"),
    ])
    .render(area, buf, s);
}

fn hints_narrow(area: Rect, buf: &mut Buffer, s: &Styles) {
    hints(vec![
        hint("j/k", "move"),
        hint("Enter", "open"),
        hint("d", "diff"),
        hint("m", "merge"),
        hint("r", "rebase"),
        hint("x", "discard"),
        hint("e", "editor"),
        hint("q", "quit"),
    ])
    .render(area, buf, s);
}

fn hints_disabled(area: Rect, buf: &mut Buffer, s: &Styles) {
    hints(vec![
        hint("d", "diff"),
        hint("m", "merge").disabled(t("rebase in progress")),
        hint("q", "quit"),
    ])
    .render(area, buf, s);
}

fn help(area: Rect, buf: &mut Buffer, s: &Styles) {
    let m = HelpModel {
        title: t("Keys"),
        groups: vec![
            HelpGroup {
                title: t("Navigate"),
                hints: vec![
                    hint("j k", "move"),
                    hint("Tab", "next panel"),
                    hint("Enter", "open"),
                ],
            },
            HelpGroup {
                title: t("Act"),
                hints: vec![
                    hint("m", "merge into base"),
                    hint("r", "rebase onto base"),
                    hint("x", "discard worktree").disabled(t("not in --plain")),
                    hint("u", "undo"),
                ],
            },
        ],
        note: Some(t("--plain is read-only: actions need the full TUI.")),
        close: hint("Esc", "close"),
    };
    modal(&m, area, buf, s);
}

// --- Layout --------------------------------------------------------------------------------

fn shell(area: Rect, buf: &mut Buffer, s: &Styles) {
    let alert = alert(Prediction::Current, "updated 3 min ago");
    let plan = BodyPlan {
        alerts: alert.height(area.width),
        graph: true,
        detail: true,
    };
    let Some(r) = layout(area, plan) else {
        TooSmallModel {
            message: t("The Cockpit needs at least 80x24. Make the terminal larger."),
            quit: hint("q", "quit"),
        }
        .render(area, buf, s);
        return;
    };
    status_bar(Connection::Live, "live", "you").render(r.header, buf, s);
    agent_list(mixed_rows(), Some(0), true).render(r.list, buf, s);
    if let Some(a) = r.alerts {
        alert.render(a, buf, s);
    }
    if let Some(g) = r.graph {
        graph_three(g, buf, s);
    }
    if let Some(d) = r.detail {
        diff_text(d, buf, s);
    }
    hints(vec![
        hint("j/k", "move"),
        hint("Enter", "open"),
        hint("d", "diff"),
        hint("q", "quit"),
    ])
    .render(r.hints, buf, s);
}

macro_rules! story {
    ($component:literal, $state:literal, $w:expr, $h:expr, $paint:expr) => {
        Story {
            component: $component,
            state: $state,
            width: $w,
            height: $h,
            paint: $paint,
        }
    };
}

/// Every story, grouped by component in the order of DSYS-GRP-001 § 3.
pub fn stories() -> Vec<Story> {
    vec![
        story!("Layout", "80x24", 80, 24, shell),
        story!("Layout", "100x30", 100, 30, shell),
        story!("Layout", "120x40", 120, 40, shell),
        story!("Layout", "79x24 too small", 79, 24, shell),
        story!("StatusBar", "live", 80, 1, |a, b, s| status_bar(
            Connection::Live,
            "live",
            "you"
        )
        .render(a, b, s)),
        story!("StatusBar", "reconnecting", 80, 1, |a, b, s| status_bar(
            Connection::Reconnecting,
            "reconnecting",
            "you"
        )
        .render(a, b, s)),
        story!("StatusBar", "engine unavailable", 80, 1, |a, b, s| {
            status_bar(Connection::Unavailable, "engine unavailable", "you").render(a, b, s)
        }),
        story!("StatusBar", "acting as agent", 80, 1, |a, b, s| status_bar(
            Connection::Live,
            "live",
            "acting as claude-1"
        )
        .render(a, b, s)),
        story!("AgentList", "mixed states", 80, 7, agents_mixed),
        story!(
            "AgentList",
            "ninth agent reuses color",
            80,
            12,
            agents_ninth
        ),
        story!(
            "AgentList",
            "operation in progress",
            80,
            5,
            agents_operation
        ),
        story!("AgentList", "unavailable", 80, 5, agents_unavailable),
        story!("AgentList", "empty", 80, 4, agents_empty),
        story!("AgentList", "unfocused, scrolled", 80, 6, agents_unfocused),
        story!("GraphLanes", "three agents", 60, 6, graph_three),
        story!("GraphLanes", "collapsed lanes", 60, 6, graph_collapsed),
        story!(
            "GraphLanes",
            "unattributed commits",
            60,
            5,
            graph_unattributed
        ),
        story!("GraphLanes", "base missing", 60, 5, graph_base_missing),
        story!("DiffView", "text", 80, 11, diff_text),
        story!("DiffView", "binary", 80, 4, diff_binary),
        story!("DiffView", "no changes", 80, 4, diff_empty),
        story!("TimelineList", "entries", 80, 8, timeline_entries),
        story!("TimelineList", "empty", 80, 4, timeline_empty),
        story!("TimelineList", "purge warning", 80, 9, timeline_purge),
        story!(
            "TimelineList",
            "undo disabled",
            80,
            9,
            timeline_undo_disabled
        ),
        story!("ConflictAlert", "new", 60, 6, alert_new),
        story!("ConflictAlert", "current", 60, 6, alert_current),
        story!("ConflictAlert", "stale", 60, 6, alert_stale),
        story!("ConflictAlert", "calculating", 60, 6, alert_calculating),
        story!("ConflictAlert", "pending base", 60, 6, alert_pending),
        story!(
            "ConflictAlert",
            "not computable",
            60,
            6,
            alert_not_computable
        ),
        story!(
            "ConflictAlert",
            "more files than rows",
            60,
            6,
            alert_many_files
        ),
        story!("PolicyBanner", "blocked", 70, 10, banner_blocked),
        story!("PolicyBanner", "pending approval", 70, 9, banner_pending),
        story!("PolicyBanner", "expired", 70, 7, banner_expired),
        story!("ConfirmPrompt", "default No", 70, 9, confirm_default),
        story!("ConfirmPrompt", "Yes selected", 70, 9, confirm_yes),
        story!("ConfirmPrompt", "with recovery", 70, 8, confirm_recovery),
        story!("Notification", "success with undo", 60, 3, toast_success),
        story!("Notification", "error", 60, 3, toast_error),
        story!("Notification", "stack", 60, 4, toast_stack),
        story!("KeyHints", "bar", 80, 1, hints_bar),
        story!("KeyHints", "narrow", 80, 1, hints_narrow),
        story!("KeyHints", "disabled action", 80, 1, hints_disabled),
        story!("KeyHints", "help overlay", 70, 16, help),
    ]
}
