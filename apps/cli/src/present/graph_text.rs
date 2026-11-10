//! The typed en/es catalog of the graph's texts (panel and full-screen view).
//!
//! Every string is whole: no sentence is built by joining pieces, so a language is free to word
//! it its own way. The view folds the result to ASCII when the symbol set asks for it.

use super::i18n::Lang;

/// One text of the graph panel or view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphText<'a> {
    /// Title of the panel of the fleet.
    PanelTitle,
    /// What the panel calls the lanes that do not fit ("+2 lanes").
    PanelMore,
    /// Title of the view: the repo, the base and the UTC clock time of the answer.
    Title {
        repo: &'a str,
        base: &'a str,
        clock: &'a str,
    },
    /// Title of the view while there is no answer to name a base from.
    TitlePending {
        repo: &'a str,
    },
    /// Appended to the title while a request is in flight or the answer is out of date.
    Updating,
    Loading,
    Busy,
    Lost,
    Refused,
    /// The base is resolved but not confirmed: the message of the view, then the action.
    BaseUnconfirmed {
        name: &'a str,
    },
    ConfirmAction,
    /// The same, as one line for the panel.
    BaseUnconfirmedLine {
        name: &'a str,
    },
    NoBase,
    BaseMissing {
        name: &'a str,
    },
    RepoUnavailable,
    /// "feat-pagos · claude-1": the lane's branch and its agent; without one, "no agent".
    LaneTitle {
        branch: &'a str,
        agent: Option<&'a str>,
    },
    /// "72 commits since a1b2c3d"; `exact: false` counts "at least".
    Since {
        count: u64,
        exact: bool,
        merge_base: &'a str,
    },
    /// A commit's author is not known to the engine yet.
    Unattributed,
    Merge,
    /// Commits of the lane that the window does not list.
    More {
        count: u64,
        exact: bool,
    },
    WorktreeUnavailable,
    NoCommits,
    NoMergeBase {
        base: &'a str,
    },
    TooSlow,
    Unreadable,
    CutLanes,
    CutBytes,
}

impl GraphText<'_> {
    pub fn render(self, lang: Lang) -> String {
        match lang {
            Lang::En => self.en(),
            Lang::Es => self.es(),
        }
    }

    /// Every text, for the test of the two languages.
    pub fn every() -> Vec<GraphText<'static>> {
        vec![
            GraphText::PanelTitle,
            GraphText::PanelMore,
            GraphText::Title {
                repo: "shop",
                base: "main",
                clock: "14:03:12",
            },
            GraphText::TitlePending { repo: "shop" },
            GraphText::Updating,
            GraphText::Loading,
            GraphText::Busy,
            GraphText::Lost,
            GraphText::Refused,
            GraphText::BaseUnconfirmed { name: "main" },
            GraphText::ConfirmAction,
            GraphText::BaseUnconfirmedLine { name: "main" },
            GraphText::NoBase,
            GraphText::BaseMissing { name: "develop" },
            GraphText::RepoUnavailable,
            GraphText::LaneTitle {
                branch: "feat-pagos",
                agent: Some("claude-1"),
            },
            GraphText::LaneTitle {
                branch: "feat-pagos",
                agent: None,
            },
            GraphText::Since {
                count: 1,
                exact: true,
                merge_base: "a1b2c3d",
            },
            GraphText::Since {
                count: 72,
                exact: true,
                merge_base: "a1b2c3d",
            },
            GraphText::Since {
                count: 10_000,
                exact: false,
                merge_base: "a1b2c3d",
            },
            GraphText::Unattributed,
            GraphText::Merge,
            GraphText::More {
                count: 22,
                exact: true,
            },
            GraphText::More {
                count: 9_950,
                exact: false,
            },
            GraphText::WorktreeUnavailable,
            GraphText::NoCommits,
            GraphText::NoMergeBase { base: "main" },
            GraphText::TooSlow,
            GraphText::Unreadable,
            GraphText::CutLanes,
            GraphText::CutBytes,
        ]
    }

    fn en(self) -> String {
        match self {
            Self::PanelTitle => "Graph".into(),
            Self::PanelMore => "lanes".into(),
            Self::Title { repo, base, clock } => {
                format!("graph of {repo} over {base} · at {clock} UTC")
            }
            Self::TitlePending { repo } => format!("graph of {repo}"),
            Self::Updating => "updating…".into(),
            Self::Loading => "loading the graph…".into(),
            Self::Busy => "the engine is busy with another query; try again".into(),
            Self::Lost => "connection lost: the graph will come back when it does".into(),
            Self::Refused => "the engine refused the graph".into(),
            Self::BaseUnconfirmed { name } => {
                format!("Base {name} is not confirmed: confirm the base branch to see the lanes")
            }
            Self::ConfirmAction => "confirm it with `raptor guard confirm`".into(),
            Self::BaseUnconfirmedLine { name } => format!(
                "Base {name} is not confirmed: confirm the base branch to see the lanes (raptor guard confirm)"
            ),
            Self::NoBase => "No base branch: not computable".into(),
            Self::BaseMissing { name } => format!("Base branch {name} not found: not computable"),
            Self::RepoUnavailable => "The repo is not available: not computable".into(),
            Self::LaneTitle {
                branch,
                agent: Some(agent),
            } => format!("{branch} · {agent}"),
            Self::LaneTitle {
                branch,
                agent: None,
            } => format!("{branch} · no agent"),
            Self::Since {
                count,
                exact,
                merge_base,
            } => {
                let n = number(count, ',');
                let n = if exact { n } else { format!("at least {n}") };
                let word = if exact && count == 1 {
                    "commit"
                } else {
                    "commits"
                };
                format!("{n} {word} since {merge_base}")
            }
            Self::Unattributed => "unattributed".into(),
            Self::Merge => "(merge)".into(),
            Self::More { count, exact } => {
                let n = number(count, ',');
                if exact {
                    format!("{n} more")
                } else {
                    format!("at least {n} more")
                }
            }
            Self::WorktreeUnavailable => "not available".into(),
            Self::NoCommits => "no commits".into(),
            Self::NoMergeBase { base } => format!("no merge-base with {base}"),
            Self::TooSlow => "took too long".into(),
            Self::Unreadable => "could not be read".into(),
            Self::CutLanes => "truncated: more than 32 lanes".into(),
            Self::CutBytes => "truncated: response too large".into(),
        }
    }

    fn es(self) -> String {
        match self {
            Self::PanelTitle => "Grafo".into(),
            Self::PanelMore => "carriles".into(),
            Self::Title { repo, base, clock } => {
                format!("grafo de {repo} sobre {base} · a las {clock} UTC")
            }
            Self::TitlePending { repo } => format!("grafo de {repo}"),
            Self::Updating => "actualizando…".into(),
            Self::Loading => "cargando el grafo…".into(),
            Self::Busy => "el motor está ocupado con otra consulta; reintenta".into(),
            Self::Lost => "conexión perdida: el grafo volverá cuando vuelva".into(),
            Self::Refused => "el motor rechazó el grafo".into(),
            Self::BaseUnconfirmed { name } => {
                format!("Base {name} sin confirmar: confirma la rama base para ver los carriles")
            }
            Self::ConfirmAction => "confírmala con `raptor guard confirm`".into(),
            Self::BaseUnconfirmedLine { name } => format!(
                "Base {name} sin confirmar: confirma la rama base para ver los carriles (raptor guard confirm)"
            ),
            Self::NoBase => "Sin rama base: no calculable".into(),
            Self::BaseMissing { name } => format!("Rama base {name} no encontrada: no calculable"),
            Self::RepoUnavailable => "El repo no está disponible: no calculable".into(),
            Self::LaneTitle {
                branch,
                agent: Some(agent),
            } => format!("{branch} · {agent}"),
            Self::LaneTitle {
                branch,
                agent: None,
            } => format!("{branch} · sin agente"),
            Self::Since {
                count,
                exact,
                merge_base,
            } => {
                let n = number(count, '.');
                let n = if exact { n } else { format!("al menos {n}") };
                let word = if exact && count == 1 {
                    "commit"
                } else {
                    "commits"
                };
                format!("{n} {word} desde {merge_base}")
            }
            Self::Unattributed => "sin atribuir".into(),
            Self::Merge => "(merge)".into(),
            Self::More { count, exact } => {
                let n = number(count, '.');
                if exact {
                    format!("{n} más")
                } else {
                    format!("al menos {n} más")
                }
            }
            Self::WorktreeUnavailable => "no disponible".into(),
            Self::NoCommits => "sin commits".into(),
            Self::NoMergeBase { base } => format!("sin merge-base con {base}"),
            Self::TooSlow => "tardó demasiado".into(),
            Self::Unreadable => "no se pudo leer".into(),
            Self::CutLanes => "truncado: más de 32 carriles".into(),
            Self::CutBytes => "truncado: respuesta demasiado grande".into(),
        }
    }
}

/// `n` with `sep` between groups of three digits.
fn number(n: u64, sep: char) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(sep);
        }
        out.push(c);
    }
    out
}
