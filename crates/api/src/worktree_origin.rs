//! Who created a linked worktree and how that is known (US-WTL-005, BR-WTL-011).
//!
//! The invariants (`unknown` <=> `none`, `gitraptor` <=> `registered`, no
//! `hands_off` for `gitraptor`) hold for every value, including the ones read
//! off the wire: a guessed origin with false confidence cannot be built.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The tool that created a worktree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum OriginToolView {
    Gitraptor,
    Orca,
    Worktrunk,
    Codex,
    Gitkraken,
    ClaudeCode,
    Unknown,
}

/// How the origin is known.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum OriginEvidenceView {
    /// GitRaptor's own record (reserved: it is never produced yet).
    Registered,
    /// A marker the tool leaves in `<common>/worktrees/<id>/`.
    AdminMarker,
    /// The root sits where the tool puts worktrees by default: likely.
    PathPattern,
    /// No evidence: the tool is `unknown`.
    None,
}

/// Why a foreign worktree is "hands off".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum HandsOffReasonView {
    Detached,
    Locked,
    Unreadable,
}

impl OriginToolView {
    /// Product name for the user, untranslated; `None` for `Unknown`.
    pub fn label(self) -> Option<&'static str> {
        match self {
            Self::Gitraptor => Some("GitRaptor"),
            Self::Orca => Some("Orca"),
            Self::Worktrunk => Some("worktrunk"),
            Self::Codex => Some("Codex"),
            Self::Gitkraken => Some("GitKraken"),
            Self::ClaudeCode => Some("Claude Code"),
            Self::Unknown => None,
        }
    }
}

/// Who created a linked worktree and how that is known.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, try_from = "OriginWire")]
pub struct WorktreeOriginView {
    pub tool: OriginToolView,
    pub evidence: OriginEvidenceView,
    /// Foreign and not proven safe to touch: GitRaptor offers no discard,
    /// cleanup or copy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hands_off: Option<HandsOffReasonView>,
}

/// The combinations [`WorktreeOriginView::new`] refuses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OriginInvariant {
    /// `unknown` with some evidence.
    UnknownWithEvidence,
    /// A tool with no evidence.
    EvidenceWithoutTool,
    /// `gitraptor` without `registered`, or `registered` for another tool.
    GitraptorNotRegistered,
    /// `gitraptor` with a `hands_off` reason.
    GitraptorHandsOff,
}

impl std::fmt::Display for OriginInvariant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::UnknownWithEvidence => "an unknown origin cannot carry evidence",
            Self::EvidenceWithoutTool => "a known tool needs evidence",
            Self::GitraptorNotRegistered => "gitraptor and `registered` go together",
            Self::GitraptorHandsOff => "a gitraptor worktree is never hands off",
        })
    }
}

impl std::error::Error for OriginInvariant {}

/// The wire form of [`WorktreeOriginView`], before its invariants are checked.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OriginWire {
    tool: OriginToolView,
    evidence: OriginEvidenceView,
    #[serde(default)]
    hands_off: Option<HandsOffReasonView>,
}

impl WorktreeOriginView {
    /// Builds an origin, refusing the combinations that would claim
    /// confidence without evidence.
    pub fn new(
        tool: OriginToolView,
        evidence: OriginEvidenceView,
        hands_off: Option<HandsOffReasonView>,
    ) -> Result<Self, OriginInvariant> {
        use OriginEvidenceView as E;
        use OriginToolView as T;
        match (tool, evidence) {
            (T::Unknown, E::None) => {}
            (T::Unknown, _) => return Err(OriginInvariant::UnknownWithEvidence),
            (_, E::None) => return Err(OriginInvariant::EvidenceWithoutTool),
            (T::Gitraptor, E::Registered) => {}
            (T::Gitraptor, _) | (_, E::Registered) => {
                return Err(OriginInvariant::GitraptorNotRegistered);
            }
            _ => {}
        }
        if tool == T::Gitraptor && hands_off.is_some() {
            return Err(OriginInvariant::GitraptorHandsOff);
        }
        Ok(Self {
            tool,
            evidence,
            hands_off,
        })
    }

    /// The worktree was not created by GitRaptor.
    pub fn is_foreign(&self) -> bool {
        self.tool != OriginToolView::Gitraptor
    }

    /// GitRaptor offers no discard, cleanup or copy for it.
    pub fn is_hands_off(&self) -> bool {
        self.hands_off.is_some()
    }

    /// The origin comes from the path only: likely, not certain.
    pub fn is_likely(&self) -> bool {
        self.evidence == OriginEvidenceView::PathPattern
    }
}

impl TryFrom<OriginWire> for WorktreeOriginView {
    type Error = OriginInvariant;

    fn try_from(w: OriginWire) -> Result<Self, Self::Error> {
        Self::new(w.tool, w.evidence, w.hands_off)
    }
}
