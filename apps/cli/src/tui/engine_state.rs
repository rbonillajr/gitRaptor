//! What the body and the header say about the engine and the connection: the one place that
//! decides the precedence, so `view` only asks. Pure functions of the [`Model`]; the engine's
//! own state is painted as published, never recomputed here.
//!
//! The body, in this order: a connection without an engine, Waiting for Git, the question of
//! an unobserved folder or the repo selector (both decided by `view`), no repos in place of the
//! fleet, and the fleet.

use gitraptor_api::messages::{DivergenceView, EngineStateView, RepoTier, ResyncReason};

use crate::model::{ConnState, Model, RepoView, SafeText, WorktreeState};
use crate::present::i18n::Text;
use crate::tui::style::Styles;
use crate::tui::view::Say;
use crate::tui::widgets::state_panel::{PanelTone, StatePanelModel};

/// Minimum Git the engine requires (BR-WF-002), pinned by a test to `gitraptor_git`.
pub const MIN_GIT: &str = "2.38";

/// What the header says instead of, or besides, the connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaderNotice {
    Resync(Option<ResyncReason>),
    Reconciling,
    Dormant,
}

fn say(model: &Model) -> Say {
    Say::new(model, &Styles::new(model.ui.theme))
}

fn panel(model: &Model, tone: PanelTone, title: Text<'_>, message: Text<'_>) -> StatePanelModel {
    let say = say(model);
    StatePanelModel {
        tone,
        title: say.safe(title),
        message: say.safe(message),
    }
}

/// A connection without an engine, or waiting for Git: replaces everything in the body (rows 1–2).
pub fn blocking_panel(model: &Model) -> Option<StatePanelModel> {
    let (title, message) = match model.conn {
        ConnState::EngineUnavailable => {
            (Text::EngineUnavailableTitle, Text::EngineUnavailableMessage)
        }
        ConnState::Incompatible => (Text::IncompatibleTitle, Text::IncompatibleMessage),
        ConnState::Rejected => (Text::RejectedTitle, Text::RejectedMessage),
        ConnState::Unsupported => (Text::UnsupportedTitle, Text::UnsupportedMessage),
        ConnState::Connecting
        | ConnState::Starting
        | ConnState::Syncing
        | ConnState::Live
        | ConnState::Resyncing
        | ConnState::Reconnecting { .. } => {
            return (global_engine(model) == Some(EngineStateView::WaitingForGit)).then(|| {
                panel(
                    model,
                    PanelTone::Warning,
                    Text::WaitingForGitTitle,
                    Text::WaitingForGitMessage { min: MIN_GIT },
                )
            });
        }
    };
    Some(panel(model, PanelTone::Error, title, message))
}

/// No repos: replaces the fleet widget only (row 5).
pub fn fleet_panel(model: &Model) -> Option<StatePanelModel> {
    (global_engine(model) == Some(EngineStateView::NoRepos) && model.engine.repo.is_none()).then(
        || {
            panel(
                model,
                PanelTone::Info,
                Text::NoReposTitle,
                Text::NoReposMessage,
            )
        },
    )
}

/// Resync while it lasts; otherwise, with `Live`, the selected repo's tier.
pub fn header_notice(model: &Model) -> Option<HeaderNotice> {
    let resync = model.engine.resync;
    match model.conn {
        ConnState::Resyncing => Some(HeaderNotice::Resync(resync)),
        ConnState::Connecting | ConnState::Reconnecting { .. } | ConnState::Syncing
            if resync.is_some() =>
        {
            Some(HeaderNotice::Resync(resync))
        }
        ConnState::Live => match selected_tier(model)? {
            RepoTier::Waking => Some(HeaderNotice::Reconciling),
            RepoTier::Dormant => Some(HeaderNotice::Dormant),
            RepoTier::Active => None,
        },
        _ => None,
    }
}

/// The published base when some row's divergence is `BaseMissing`.
pub fn base_not_found(repo: &RepoView) -> Option<&SafeText> {
    let missing = repo.worktrees.iter().any(|w| {
        matches!(
            w.state,
            WorktreeState::Ready {
                divergence: DivergenceView::BaseMissing,
                ..
            }
        )
    });
    repo.base.as_ref().filter(|_| missing)
}

fn global_engine(model: &Model) -> Option<EngineStateView> {
    model.engine.global.data.as_ref().map(|g| g.engine)
}

/// The tier of the selected repo, as the global scope published it.
fn selected_tier(model: &Model) -> Option<RepoTier> {
    let repo_id = &model.engine.repo.as_ref()?.data.as_ref()?.repo_id;
    model
        .engine
        .global
        .data
        .as_ref()?
        .repos
        .iter()
        .find(|r| &r.repo_id == repo_id)?
        .tier
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ConnEvent, Msg, RepoChoice};
    use crate::present::i18n::Lang;
    use crate::tui::gallery::buffer_text;
    use crate::tui::update::update;
    use crate::tui::view::{asking, outside, shop_model, view};
    use gitraptor_api::messages::UnavailableReason;
    use gitraptor_theme::{Background, ColorMode, Contrast, SymbolSet, Theme};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    /// The symbol set is pinned: the platform default may differ (Windows), and a snapshot
    /// compares text.
    fn pinned(mut model: Model) -> Model {
        let theme = Theme::new(ColorMode::NoColor, Contrast::Normal, SymbolSet::Unicode)
            .with_background(Background::Dark);
        model.ui = crate::model::Ui { theme, ..model.ui };
        model
    }

    fn screen(model: &Model, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| view(model, f)).unwrap();
        buffer_text(terminal.backend().buffer()).join("\n")
    }

    fn with_engine(mut model: Model, state: EngineStateView) -> Model {
        model.engine.global.data.as_mut().unwrap().engine = state;
        model
    }

    fn no_repos(lang: Lang) -> Model {
        let mut model = with_engine(outside(lang, &[]), EngineStateView::NoRepos);
        model.engine.global.data.as_mut().unwrap().repo_count = 0;
        model
    }

    fn waiting_for_git(lang: Lang) -> Model {
        with_engine(no_repos(lang), EngineStateView::WaitingForGit)
    }

    fn chosen(lang: Lang) -> Model {
        let mut model = outside(lang, &["shop", "blog"]);
        update(&mut model, Msg::Conn(ConnEvent::Unlocated));
        model
    }

    #[test]
    fn the_body_follows_the_precedence() {
        // 1. A connection without an engine beats everything, a question included.
        let mut m = pinned(asking(Lang::En));
        m.conn = ConnState::EngineUnavailable;
        let painted = screen(&m, 100, 30);
        assert!(painted.contains("Engine unavailable"), "{painted}");
        assert!(!painted.contains("Observe this repo?"), "{painted}");
        // 2. Waiting for Git beats the question of the folder.
        let m = pinned(with_engine(
            asking(Lang::En),
            EngineStateView::WaitingForGit,
        ));
        let painted = screen(&m, 100, 30);
        assert!(painted.contains("Waiting for Git"), "{painted}");
        assert!(!painted.contains("Observe this repo?"), "{painted}");
        // Waiting for Git with no repos does not show the first-repo guide.
        let painted = screen(&pinned(waiting_for_git(Lang::En)), 100, 30);
        assert!(painted.contains("Waiting for Git"), "{painted}");
        assert!(painted.contains("2.38"), "{painted}");
        assert!(!painted.contains("raptor repo add"), "{painted}");
        let painted = screen(&pinned(waiting_for_git(Lang::Es)), 100, 30);
        assert!(painted.contains("Esperando Git"), "{painted}");
        assert!(!painted.contains("raptor repo add"), "{painted}");
        // 3. The question of the folder.
        let painted = screen(&pinned(asking(Lang::En)), 100, 30);
        assert!(painted.contains("Observe this repo? [y/N]"), "{painted}");
        // 4. The repo selector.
        let painted = screen(&pinned(chosen(Lang::En)), 100, 30);
        assert!(painted.contains("Observed repos"), "{painted}");
        // 5. No repos: the panel takes the fleet's place.
        let painted = screen(&pinned(no_repos(Lang::En)), 100, 30);
        assert!(painted.contains("No repos observed"), "{painted}");
        assert!(painted.contains("raptor repo add <path>"), "{painted}");
        let painted = screen(&pinned(no_repos(Lang::Es)), 100, 30);
        assert!(painted.contains("raptor repo add <ruta>"), "{painted}");
        // 6. Anything else is the fleet, also with the engine saying "no repos" while a repo
        // of the folder is open.
        let painted = screen(&pinned(shop_model(Lang::En)), 100, 30);
        assert!(painted.contains("feat-pagos"), "{painted}");
        assert!(!painted.contains("No repos observed"), "{painted}");
        let m = with_engine(shop_model(Lang::En), EngineStateView::NoRepos);
        assert!(fleet_panel(&m).is_none());
    }

    /// Every connection state, with no wildcard: a new one does not compile until it has a row.
    #[test]
    fn blocking_panel_covers_every_connection_state() {
        let table = [
            ConnState::Connecting,
            ConnState::Starting,
            ConnState::Syncing,
            ConnState::Live,
            ConnState::Resyncing,
            ConnState::Reconnecting { attempt: 2 },
            ConnState::EngineUnavailable,
            ConnState::Incompatible,
            ConnState::Rejected,
            ConnState::Unsupported,
        ];
        for conn in table {
            let expected = match conn {
                ConnState::Connecting
                | ConnState::Starting
                | ConnState::Syncing
                | ConnState::Live
                | ConnState::Resyncing
                | ConnState::Reconnecting { .. } => None,
                ConnState::EngineUnavailable => Some("Engine unavailable"),
                ConnState::Incompatible => Some("Incompatible engine"),
                ConnState::Rejected => Some("Engine channel refused"),
                ConnState::Unsupported => Some("No engine channel"),
            };
            let mut m = shop_model(Lang::En);
            m.conn = conn;
            let panel = blocking_panel(&m);
            assert_eq!(
                panel.as_ref().map(|p| p.title.as_str()),
                expected,
                "{conn:?}"
            );
            if let Some(panel) = panel {
                assert_eq!(panel.tone, PanelTone::Error, "{conn:?}");
                // Old data does not stay on screen as if it were current.
                assert!(screen(&m, 100, 30).contains(expected.unwrap()), "{conn:?}");
            }
        }
        // Waiting for Git blocks while the connection is up, with the warning tone.
        let panel = blocking_panel(&waiting_for_git(Lang::En)).unwrap();
        assert_eq!(panel.tone, PanelTone::Warning);
        assert!(panel.message.as_str().contains(MIN_GIT));
    }

    #[test]
    fn the_header_says_reconciling_dormant_or_why_it_resyncs() {
        let header = |m: &Model| {
            let styles = Styles::new(m.ui.theme);
            crate::tui::view::status_bar(m, &styles)
                .connection_label
                .as_str()
                .to_owned()
        };
        let with_tier = |lang: Lang, tier: Option<RepoTier>| {
            let mut m = pinned(shop_model(lang));
            m.engine.global.data.as_mut().unwrap().repos = vec![RepoChoice {
                repo_id: "r1".into(),
                name: SafeText::name("shop"),
                path: SafeText::text("/w/shop/.git"),
                tier,
            }];
            m
        };
        // The tier: reconciling and dormant take the place of "live · engine …".
        let m = with_tier(Lang::En, Some(RepoTier::Waking));
        assert_eq!(header_notice(&m), Some(HeaderNotice::Reconciling));
        assert_eq!(header(&m), "reconciling: provisional data");
        let m = with_tier(Lang::Es, Some(RepoTier::Dormant));
        assert_eq!(header_notice(&m), Some(HeaderNotice::Dormant));
        assert_eq!(header(&m), "dormido: datos con retraso");
        for tier in [None, Some(RepoTier::Active)] {
            let m = with_tier(Lang::En, tier);
            assert_eq!(header_notice(&m), None);
            assert!(header(&m).starts_with("live"), "{}", header(&m));
        }
        // A tier is only said while live.
        let mut m = with_tier(Lang::En, Some(RepoTier::Waking));
        m.conn = ConnState::Resyncing;
        assert_eq!(header_notice(&m), Some(HeaderNotice::Resync(None)));
        // The reason of a resync, while it lasts and through the reconnection after it.
        for conn in [
            ConnState::Resyncing,
            ConnState::Connecting,
            ConnState::Reconnecting { attempt: 1 },
            ConnState::Syncing,
        ] {
            let mut m = pinned(shop_model(Lang::En));
            m.conn = conn;
            m.engine.resync = Some(ResyncReason::SlowConsumer);
            assert_eq!(
                header_notice(&m),
                Some(HeaderNotice::Resync(Some(ResyncReason::SlowConsumer)))
            );
            assert!(
                header(&m).ends_with("the view fell behind the engine"),
                "{conn:?}: {}",
                header(&m)
            );
            m.ui.lang = Lang::Es;
            assert!(header(&m).contains("la vista se quedó atrás del motor"));
        }
        // A gap the client found has no reason, and says "resyncing" alone.
        let mut m = pinned(shop_model(Lang::En));
        m.conn = ConnState::Resyncing;
        assert!(header(&m).starts_with("resyncing"), "{}", header(&m));
        assert!(!header(&m).contains("fell behind"), "{}", header(&m));
        // Back to live, nothing is left of it.
        let mut m = pinned(shop_model(Lang::En));
        m.engine.resync = Some(ResyncReason::SlowConsumer);
        assert_eq!(header_notice(&m), None);
    }

    #[test]
    fn the_minimum_git_is_the_one_the_engine_requires() {
        let min = gitraptor_git::resolve::MIN_VERSION;
        assert_eq!(min.patch, 0);
        assert_eq!(MIN_GIT, format!("{}.{}", min.major, min.minor));
    }

    fn with_missing_base(lang: Lang) -> Model {
        let mut m = pinned(shop_model(lang));
        let data = m.engine.repo.as_mut().unwrap().data.as_mut().unwrap();
        data.base = Some(SafeText::name("develop"));
        for w in &mut data.worktrees {
            if let WorktreeState::Ready { divergence, .. } = &mut w.state {
                *divergence = DivergenceView::BaseMissing;
            }
        }
        m
    }

    #[test]
    fn a_missing_base_names_the_published_base_and_never_counts() {
        for (lang, title, cell) in [
            (
                Lang::En,
                "↑↓ not computable: develop not found",
                "not computable",
            ),
            (
                Lang::Es,
                "↑↓ no calculable: develop no encontrada",
                "no calculable",
            ),
        ] {
            let m = with_missing_base(lang);
            let repo = m.engine.repo.as_ref().unwrap().data.as_ref().unwrap();
            assert_eq!(base_not_found(repo).map(SafeText::as_str), Some("develop"));
            let painted = screen(&m, 120, 40);
            assert!(painted.contains(title), "{painted}");
            assert!(painted.contains(cell), "{painted}");
            for line in painted.lines().filter(|l| l.contains("feat-pagos")) {
                assert!(!line.contains('↑'), "{line}");
                assert!(!line.contains('–'), "{line}");
            }
        }
        // Without a published base there is no name to give, and with every row counted no
        // title about it.
        let mut m = with_missing_base(Lang::En);
        m.engine.repo.as_mut().unwrap().data.as_mut().unwrap().base = None;
        let repo = m.engine.repo.as_ref().unwrap().data.as_ref().unwrap();
        assert!(base_not_found(repo).is_none());
        let m = pinned(shop_model(Lang::En));
        let repo = m.engine.repo.as_ref().unwrap().data.as_ref().unwrap();
        assert!(base_not_found(repo).is_none());
    }

    #[test]
    fn an_unavailable_worktree_says_not_available() {
        for (lang, reason, text) in [
            (
                Lang::En,
                UnavailableReason::Missing,
                "unavailable: folder missing",
            ),
            (
                Lang::En,
                UnavailableReason::Unreadable,
                "unavailable: unreadable now",
            ),
            (
                Lang::Es,
                UnavailableReason::Missing,
                "no disponible: falta la carpeta",
            ),
            (
                Lang::Es,
                UnavailableReason::Unreadable,
                "no disponible: ilegible ahora",
            ),
        ] {
            let mut m = pinned(shop_model(lang));
            let data = m.engine.repo.as_mut().unwrap().data.as_mut().unwrap();
            let gone = data.worktrees.iter_mut().find(|w| !w.main).unwrap();
            gone.state = WorktreeState::Unavailable(reason);
            let painted = screen(&m, 120, 40);
            let row = painted.lines().find(|l| l.contains(text)).unwrap_or("");
            assert!(row.contains('⚠'), "{text}: {painted}");
        }
    }

    #[test]
    fn the_screens_of_each_state() {
        let mut settings = insta::Settings::clone_current();
        settings.set_prepend_module_to_snapshot(false);
        settings.set_snapshot_path("snapshots");
        settings.bind(|| {
            for (code, lang) in [("en", Lang::En), ("es", Lang::Es)] {
                let mut unavailable = pinned(shop_model(lang));
                unavailable.conn = ConnState::EngineUnavailable;
                let mut reconciling = pinned(shop_model(lang));
                reconciling.engine.global.data.as_mut().unwrap().repos = vec![RepoChoice {
                    repo_id: "r1".into(),
                    name: SafeText::name("shop"),
                    path: SafeText::text("/w/shop/.git"),
                    tier: Some(RepoTier::Waking),
                }];
                let mut resync = pinned(shop_model(lang));
                resync.conn = ConnState::Resyncing;
                resync.engine.resync = Some(ResyncReason::SlowConsumer);
                let at_80x24 = [
                    ("engine_unavailable", unavailable),
                    ("waiting_for_git", pinned(waiting_for_git(lang))),
                    ("no_repos", pinned(no_repos(lang))),
                    ("reconciling", reconciling),
                    ("resync", resync),
                ];
                for (name, model) in at_80x24 {
                    insta::assert_snapshot!(
                        format!("engine_state_{name}_80x24_{code}"),
                        screen(&model, 80, 24)
                    );
                }
                insta::assert_snapshot!(
                    format!("engine_state_base_not_found_120x40_{code}"),
                    screen(&with_missing_base(lang), 120, 40)
                );
            }
        });
    }
}
