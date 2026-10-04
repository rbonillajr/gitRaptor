//! Settings document (ADR-GRP-007): model, schema, strict parsing and validation.

pub mod diagnostic;
pub mod document;
pub mod model;
pub mod schema;
pub mod strict;

pub use diagnostic::{Code, Diagnostic, Limit, Location, SourceKind};
pub use document::{Parsed, SourceStatus, parse_document};
pub use model::{Engine, Level, Operation, Permissions, Policies, Settings, TimeMachine, Watcher};
