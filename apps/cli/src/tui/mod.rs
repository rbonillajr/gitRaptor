//! The Cockpit TUI (ADR-CKP-003): TEA loop, view, keys and terminal.

pub mod app;
pub mod engine_state;
pub mod fleet_order;
pub mod gallery;
pub mod help;
pub mod input;
pub mod keymap;
pub mod metrics;
pub mod prediction;
pub mod style;
pub mod term;
pub mod update;
pub mod view;
pub mod widgets;

pub use term::run;

#[cfg(test)]
mod prediction_stale_tests;
#[cfg(test)]
mod prediction_tests;
#[cfg(test)]
mod view_order_tests;
