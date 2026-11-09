//! The typed en/es catalog of the freshness texts of the conflict panel: how old a result is
//! and the lines that group the pairs that have none.
//!
//! Every string is whole per language arm. The view folds the `·` to ASCII when the symbol set
//! asks for it.

use gitraptor_api::prediction::PairReason;

use super::i18n::Lang;

/// The unit an age is told in: the largest whole one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgeUnit {
    Seconds,
    Minutes,
    Hours,
    Days,
}

impl AgeUnit {
    fn short(self) -> &'static str {
        match self {
            Self::Seconds => "s",
            Self::Minutes => "min",
            Self::Hours => "h",
            Self::Days => "d",
        }
    }
}

/// One text of the freshness of the prediction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FreshnessText {
    /// "3 min ago".
    Age { n: i64, unit: AgeUnit },
    /// A pair being recalculated, with the age of the result it still shows.
    Stale { n: i64, unit: AgeUnit },
    /// Every pair is current and none has a conflict or an overlap.
    Clear { n: i64, unit: AgeUnit },
    /// `n` of the `m` pairs have no result yet.
    Calculating { n: usize, m: usize },
    /// `n` pairs wait for `reason`.
    Pending { reason: PairReason, n: usize },
    /// `n` pairs cannot be computed because of `reason`.
    NotComputable { reason: PairReason, n: usize },
    /// Summary lines that do not fit.
    MoreLines(usize),
}

impl FreshnessText {
    pub fn render(self, lang: Lang) -> String {
        match lang {
            Lang::En => self.en(),
            Lang::Es => self.es(),
        }
    }

    /// Every shape of text, for the test of the two languages.
    pub fn every() -> Vec<Self> {
        const REASONS: [PairReason; 9] = [
            PairReason::BaseUnconfirmed,
            PairReason::OperationInProgress,
            PairReason::PowerSaving,
            PairReason::Dormant,
            PairReason::BaseMissing,
            PairReason::MissingObject,
            PairReason::Shallow,
            PairReason::LimitExceeded,
            PairReason::Error,
        ];
        let mut all = Vec::new();
        for unit in [
            AgeUnit::Seconds,
            AgeUnit::Minutes,
            AgeUnit::Hours,
            AgeUnit::Days,
        ] {
            all.push(Self::Age { n: 3, unit });
            all.push(Self::Stale { n: 40, unit });
            all.push(Self::Clear { n: 1, unit });
        }
        all.extend([
            Self::Calculating { n: 1, m: 1 },
            Self::Calculating { n: 1, m: 10 },
            Self::Calculating { n: 3, m: 10 },
            Self::MoreLines(2),
        ]);
        for reason in REASONS {
            for n in [1, 4] {
                all.push(Self::Pending { reason, n });
                all.push(Self::NotComputable { reason, n });
            }
        }
        all
    }

    fn en(self) -> String {
        match self {
            Self::Age { n, unit } => format!("{n} {} ago", unit.short()),
            Self::Stale { n, unit } => {
                format!("outdated · {n} {} ago · recalculating", unit.short())
            }
            Self::Clear { n, unit } => {
                format!("No conflicts predicted · {n} {} ago", unit.short())
            }
            Self::Calculating { n, m: 1 } => format!("Calculating {n} of 1 pair"),
            Self::Calculating { n, m } => format!("Calculating {n} of {m} pairs"),
            Self::Pending { reason, n } => {
                format!("Pending: {} ({})", reason_en(reason), pairs_en(n))
            }
            Self::NotComputable { reason, n } => {
                format!("Not computable: {} ({})", reason_en(reason), pairs_en(n))
            }
            Self::MoreLines(n) => format!("+{n} more"),
        }
    }

    fn es(self) -> String {
        match self {
            Self::Age { n, unit } => format!("hace {n} {}", unit.short()),
            Self::Stale { n, unit } => {
                format!("desactualizado · hace {n} {} · recalculando", unit.short())
            }
            Self::Clear { n, unit } => {
                format!("Sin conflictos previstos · hace {n} {}", unit.short())
            }
            Self::Calculating { n, m: 1 } => format!("Calculando {n} de 1 par"),
            Self::Calculating { n, m } => format!("Calculando {n} de {m} pares"),
            Self::Pending { reason, n } => {
                format!("Pendiente: {} ({})", reason_es(reason), pairs_es(n))
            }
            Self::NotComputable { reason, n } => {
                format!("No calculable: {} ({})", reason_es(reason), pairs_es(n))
            }
            Self::MoreLines(n) => format!("+{n} más"),
        }
    }
}

fn pairs_en(n: usize) -> String {
    if n == 1 {
        "1 pair".to_owned()
    } else {
        format!("{n} pairs")
    }
}

fn pairs_es(n: usize) -> String {
    if n == 1 {
        "1 par".to_owned()
    } else {
        format!("{n} pares")
    }
}

fn reason_en(reason: PairReason) -> &'static str {
    match reason {
        PairReason::BaseUnconfirmed => "confirm the base branch",
        PairReason::OperationInProgress => "an operation is in progress",
        PairReason::PowerSaving => "power saving is on",
        PairReason::Dormant => "the repo is dormant",
        PairReason::BaseMissing => "the base branch is missing",
        PairReason::MissingObject => "a Git object is missing",
        PairReason::Shallow => "the clone is shallow",
        PairReason::LimitExceeded => "a limit was exceeded",
        PairReason::Error => "the calculation failed",
    }
}

fn reason_es(reason: PairReason) -> &'static str {
    match reason {
        PairReason::BaseUnconfirmed => "confirma la rama base",
        PairReason::OperationInProgress => "operación en curso",
        PairReason::PowerSaving => "ahorro de energía activo",
        PairReason::Dormant => "el repo está en reposo",
        PairReason::BaseMissing => "falta la rama base",
        PairReason::MissingObject => "falta un objeto de Git",
        PairReason::Shallow => "el clon es superficial",
        PairReason::LimitExceeded => "se superó un límite",
        PairReason::Error => "el cálculo falló",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_freshness_text_has_both_languages() {
        let every = FreshnessText::every();
        assert!(every.len() > 30, "{}", every.len());
        for text in &every {
            for lang in [Lang::En, Lang::Es] {
                assert!(
                    !text.render(lang).trim().is_empty(),
                    "{text:?} has no text in {lang:?}"
                );
            }
        }
        let es = |t: FreshnessText| t.render(Lang::Es);
        assert_eq!(
            es(FreshnessText::Pending {
                reason: PairReason::BaseUnconfirmed,
                n: 4
            }),
            "Pendiente: confirma la rama base (4 pares)"
        );
        assert_eq!(
            es(FreshnessText::NotComputable {
                reason: PairReason::BaseMissing,
                n: 1
            }),
            "No calculable: falta la rama base (1 par)"
        );
        assert_eq!(
            es(FreshnessText::Calculating { n: 1, m: 1 }),
            "Calculando 1 de 1 par"
        );
    }
}
