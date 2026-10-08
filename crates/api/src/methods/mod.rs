//! The method registry of the contract (ADR-GRP-016 § 2).
//!
//! Each method says whether it is a reserved command (ADR-GRP-005 § 6:
//! authorized only by the daemon, never by a client claim), whether
//! `raptor-mcp` may use it (SEC-14, NFR-02) and, for methods declared ahead
//! of their story, which story implements them.
//!
//! Each contract module declares its methods, notifications, capabilities
//! and error codes in its own file here, as one [`Group`]. This file only
//! holds the types and one line per module: a story adds a method to its
//! module's file and edits nothing shared. A new module is a new file and
//! its two lines below (`mod` + `pub use`, and its entry in [`GROUPS`]).

use std::sync::LazyLock;

use crate::capability::Capability;
use crate::rpc::ErrorSpec;

mod attribution;
mod audit;
mod connection;
mod daemon;
mod discovery;
mod engine;
mod events;
mod guard;
mod mcp;
mod observation;
mod operation;
mod registration;
mod repo;
mod requester;
mod scope;
mod sessions;
mod timemachine;

pub use attribution::*;
pub use audit::*;
pub use connection::*;
pub use daemon::*;
pub use discovery::*;
pub use engine::*;
pub use events::*;
pub use guard::*;
pub use mcp::*;
pub use observation::*;
pub use operation::*;
pub use registration::*;
pub use repo::*;
pub use requester::*;
pub use scope::*;
pub use sessions::*;
pub use timemachine::*;

/// Every module of the contract, in the order the handshake lists its
/// methods.
pub const GROUPS: &[&Group] = &[
    &connection::GROUP,
    &engine::GROUP,
    &events::GROUP,
    &sessions::GROUP,
    &audit::GROUP,
    &daemon::GROUP,
    &repo::GROUP,
    &attribution::GROUP,
    &registration::GROUP,
    &operation::GROUP,
    &requester::GROUP,
    &timemachine::GROUP,
    &scope::GROUP,
    &guard::GROUP,
    &mcp::GROUP,
    &observation::GROUP,
    &discovery::GROUP,
];

/// What one contract module declares, in its own file.
#[derive(Debug)]
pub struct Group {
    /// The module, and the prefix of its names: `guard` for `guard.plan`.
    pub module: &'static str,
    pub methods: &'static [MethodSpec],
    /// Notifications the daemon pushes for this module.
    pub notifications: &'static [&'static str],
    pub capabilities: &'static [Capability],
    /// First code of the module's block of [`ERROR_BLOCK_LEN`] codes, going
    /// down (`-33000` owns `-33000..=-33019`). `None` until the module has
    /// an error of its own.
    pub error_block: Option<i64>,
    /// Codes the module added after the shared list froze; each one is in
    /// its block.
    pub errors: &'static [ErrorSpec],
}

/// Codes in one module's block.
pub const ERROR_BLOCK_LEN: i64 = 20;

/// Where the blocks of the modules start: below the range JSON-RPC reserves
/// (`-32768..=-32000`), so they never meet a code of the shared list.
pub const FIRST_ERROR_BLOCK: i64 = -33000;

impl Group {
    /// An empty group of `module`, to fill with struct update syntax.
    pub const fn new(module: &'static str) -> Self {
        Self {
            module,
            methods: &[],
            notifications: &[],
            capabilities: &[],
            error_block: None,
            errors: &[],
        }
    }

    /// Whether `code` is in the module's block.
    pub fn owns(&self, code: i64) -> bool {
        self.error_block
            .is_some_and(|start| code <= start && code > start - ERROR_BLOCK_LEN)
    }
}

/// Whether a method may modify a repository (ADR-TMC-004 § 1). Only the
/// protected operation and the Time Machine's own protected operations do:
/// every write takes a prior snapshot first (BR-TMC-CONS-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepoWrite {
    None,
    /// A catalog operation run through the protected operation.
    Protected,
    /// Undo, redo or restore: protected operations of the Time Machine.
    TimeMachine,
    /// The install of the Guardrails hook layer: only `core.hooksPath` and
    /// `<common>/gitraptor/`, never the user's content; recovered by its own
    /// journal (ADR-GRD-001 § 4), not by a prior snapshot.
    Guardrails,
}

/// Static description of one method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MethodSpec {
    pub name: &'static str,
    /// Reserved to the developer (ADR-GRP-005 § 6).
    pub reserved: bool,
    /// Offered to `raptor-mcp` connections.
    pub mcp: bool,
    /// Story that implements it, for methods declared ahead of it.
    pub implemented_by: Option<&'static str>,
    pub writes: RepoWrite,
    /// First protocol version that has it: a connection that negotiated an
    /// older one does not see it (DS-TS-GRP-004 E-D1). Frozen at
    /// [`crate::capability::CAPABILITIES_PROTOCOL`] for every method added
    /// from then on: a newer method is found in the handshake's `methods`.
    pub since: u32,
}

impl MethodSpec {
    /// Whether a connection of `protocol` has this method.
    pub const fn exists_in(&self, protocol: u32) -> bool {
        protocol >= self.since
    }

    /// A method that is new from protocol `since`.
    pub(crate) const fn since(self, since: u32) -> Self {
        Self { since, ..self }
    }

    /// A method that may modify a repo.
    pub(crate) const fn writes(self, writes: RepoWrite) -> Self {
        Self { writes, ..self }
    }
}

/// Methods of protocol 5 and before: every client in the window has them.
pub(crate) const BASE: u32 = crate::MIN_COMPATIBLE_PROTOCOL;

/// A method that is not reserved and not implemented ahead of its story.
pub(crate) const fn method(name: &'static str, reserved: bool, mcp: bool) -> MethodSpec {
    MethodSpec {
        name,
        reserved,
        mcp,
        implemented_by: None,
        writes: RepoWrite::None,
        since: BASE,
    }
}

/// A reserved method declared ahead of the story that implements it.
pub(crate) const fn pending(name: &'static str, story: &'static str) -> MethodSpec {
    MethodSpec {
        name,
        reserved: true,
        mcp: false,
        implemented_by: Some(story),
        writes: RepoWrite::None,
        since: BASE,
    }
}

/// Every method of the contract, from every module.
pub static METHODS: LazyLock<Vec<MethodSpec>> = LazyLock::new(|| {
    GROUPS
        .iter()
        .flat_map(|g| g.methods.iter().copied())
        .collect()
});

pub fn spec(name: &str) -> Option<&'static MethodSpec> {
    METHODS.iter().find(|m| m.name == name)
}

/// The module that declares a method or a notification, by the prefix of
/// its name. `hello` and `ping` predate the prefixes: they are
/// `connection`'s.
pub fn module_of(name: &str) -> &str {
    name.split_once('.')
        .map_or("connection", |(module, _)| module)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SEC-14: no reserved command is offered to `raptor-mcp`.
    #[test]
    fn mcp_never_gets_a_reserved_command() {
        for m in METHODS.iter() {
            assert!(
                !(m.reserved && m.mcp),
                "{} is reserved and offered to MCP",
                m.name
            );
        }
        assert!(!spec(AUDIT_LIST).unwrap().mcp);
        let resources = spec(ENGINE_RESOURCES).unwrap();
        assert!(!resources.mcp && !resources.reserved);
        assert_eq!(resources.writes, RepoWrite::None);
    }

    #[test]
    fn pending_methods_name_their_story() {
        for m in METHODS.iter().filter(|m| m.implemented_by.is_some()) {
            let story = m.implemented_by.unwrap();
            if m.name.starts_with("timemachine.") {
                assert!(!m.reserved, "{}", m.name);
                assert!(story.starts_with("US-TMC-"), "{}", m.name);
            } else {
                assert!(m.reserved);
                assert!(story.starts_with("US-GRP-"));
            }
        }
    }

    /// ADR-TMC-004 § 1 and TS-TMC-004: only the protected operation and
    /// the Time Machine's undo, redo and restore may modify a repo.
    #[test]
    fn only_protected_paths_write() {
        let mut writers: Vec<_> = METHODS
            .iter()
            .filter(|m| m.writes != RepoWrite::None)
            .map(|m| (m.name, m.writes))
            .collect();
        writers.sort_by_key(|(name, _)| *name);
        assert_eq!(
            writers,
            [
                (GUARD_INSTALL, RepoWrite::Guardrails),
                (GUARD_UNINSTALL, RepoWrite::Guardrails),
                (OPERATION_RUN, RepoWrite::Protected),
                (TM_REDO, RepoWrite::TimeMachine),
                (TM_RESTORE, RepoWrite::TimeMachine),
                (TM_UNDO, RepoWrite::TimeMachine),
            ]
        );
        // A writer is never a reserved command: reserved commands do not go
        // through the prior snapshot. The one exception is the Guardrails
        // install and uninstall, recovered by their own journal (ADR-GRD-001
        // § 4); only they may declare that kind of write.
        assert!(METHODS.iter().all(|m| !(m.reserved
            && m.writes != RepoWrite::None
            && m.writes != RepoWrite::Guardrails)));
        assert!(
            METHODS
                .iter()
                .filter(|m| m.writes == RepoWrite::Guardrails)
                .all(|m| (m.name == GUARD_INSTALL || m.name == GUARD_UNINSTALL) && m.reserved)
        );
    }

    /// E-D1: protocol 5 connections keep exactly the protocol 5 methods;
    /// the Cockpit's are of protocol 6 and stay out of MCP.
    #[test]
    fn protocol_5_methods_are_new_and_not_for_mcp() {
        for name in [SCOPE_SNAPSHOT, SCOPE_SUBSCRIBE, REPO_LOCATE] {
            let m = spec(name).unwrap();
            assert!(!m.exists_in(5) && m.exists_in(6), "{name}");
            assert!(!m.mcp && !m.reserved, "{name}");
        }
        assert!(
            spec(HELLO)
                .unwrap()
                .exists_in(crate::MIN_COMPATIBLE_PROTOCOL)
        );
        assert!(METHODS.iter().all(|m| m.since <= crate::PROTOCOL_VERSION));
    }

    /// Two modules never declare one name: a method, a notification, or a
    /// method and a notification.
    #[test]
    fn method_names_are_unique() {
        let mut names: Vec<_> = METHODS.iter().map(|m| m.name).collect();
        names.extend(GROUPS.iter().flat_map(|g| g.notifications.iter().copied()));
        let total = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), total);
    }

    /// Each name lives in the file of the module its prefix names, so two
    /// stories of different modules never edit one file.
    #[test]
    fn every_name_is_in_its_module() {
        for group in GROUPS {
            let names = group
                .methods
                .iter()
                .map(|m| m.name)
                .chain(group.notifications.iter().copied())
                .chain(group.capabilities.iter().map(|c| c.name));
            for name in names {
                assert_eq!(module_of(name), group.module, "{name}");
            }
        }
        let mut modules: Vec<_> = GROUPS.iter().map(|g| g.module).collect();
        let total = modules.len();
        modules.sort_unstable();
        modules.dedup();
        assert_eq!(modules.len(), total, "a module is registered twice");
    }

    /// ADR-GRP-016 § 3: every module code is in its own block, blocks do
    /// not overlap and stay out of the range JSON-RPC reserves, and no code
    /// or name is declared twice, by the modules or in the frozen list.
    #[test]
    fn error_codes_are_in_their_blocks_and_unique() {
        use crate::rpc::{ErrorCode, module_errors};
        let mut blocks: Vec<(i64, &str)> = Vec::new();
        for group in GROUPS {
            if let Some(start) = group.error_block {
                assert!(start <= FIRST_ERROR_BLOCK, "{}", group.module);
                assert_eq!(
                    (FIRST_ERROR_BLOCK - start) % ERROR_BLOCK_LEN,
                    0,
                    "{}: a block starts a multiple of {ERROR_BLOCK_LEN} below {FIRST_ERROR_BLOCK}",
                    group.module
                );
                blocks.push((start, group.module));
            }
            for error in group.errors {
                assert!(
                    group.owns(error.code),
                    "{} is outside the block of {}",
                    error.name,
                    group.module
                );
            }
        }
        blocks.sort_unstable();
        for pair in blocks.windows(2) {
            assert_ne!(
                pair[0].0, pair[1].0,
                "{} and {} share a block",
                pair[0].1, pair[1].1
            );
        }
        let mut codes: Vec<i64> = ErrorCode::ALL.iter().map(|c| c.code()).collect();
        codes.extend(module_errors().map(|e| e.code));
        let mut names: Vec<&str> = ErrorCode::ALL.iter().map(|c| c.as_str()).collect();
        names.extend(module_errors().map(|e| e.name));
        let (code_count, name_count) = (codes.len(), names.len());
        codes.sort_unstable();
        codes.dedup();
        names.sort_unstable();
        names.dedup();
        assert_eq!(codes.len(), code_count, "an error code is declared twice");
        assert_eq!(names.len(), name_count, "an error name is declared twice");
    }

    #[test]
    fn a_block_owns_twenty_codes() {
        let group = Group {
            error_block: Some(FIRST_ERROR_BLOCK - ERROR_BLOCK_LEN),
            ..Group::new("example")
        };
        assert!(group.owns(-33020) && group.owns(-33039));
        assert!(!group.owns(-33019) && !group.owns(-33040));
        assert!(!Group::new("none").owns(FIRST_ERROR_BLOCK));
    }
}
