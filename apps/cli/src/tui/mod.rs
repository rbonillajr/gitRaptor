//! The Cockpit TUI (ADR-CKP-003): TEA loop, view, keys and terminal.

pub mod app;
pub mod input;
pub mod keymap;
pub mod metrics;
pub mod term;
pub mod update;
pub mod view;

pub use term::run;
