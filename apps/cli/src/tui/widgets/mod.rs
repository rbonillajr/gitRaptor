//! TUI component library v0 (DSYS-GRP-001 § 3, TS-CKP-005).
//!
//! # Convention (shared with INF-CKP-001)
//!
//! - Each component is a **view model** of its own (plain data: no channel, engine or
//!   `present` types) that implements [`Component`]. `tui::view` builds these models from the
//!   app `Model`; `present::ingest` does not know them.
//! - Every visible text is a [`SafeText`](crate::model::SafeText) field of the model, labels
//!   included: widgets hold no user-facing literal, so the language is the caller's catalog.
//! - Widgets are **pure**: no clock, no timer, no state of their own. Selection, focus and the
//!   scroll offset live in the model (the offset follows the selection with
//!   [`follow`](super::style::follow)); times and countdowns come as text.
//! - Colors and symbols come only from [`Styles`] (semantic tokens of `crates/theme`), and
//!   structural glyphs from [`Glyphs`](super::style::Glyphs). Nothing is shown by color alone:
//!   every state has a symbol or a text, and focus has a shape (heavier border, `›` marker).
//! - To draw: `frame.render_widget(themed(&model, &styles), area)`. Modals and toasts expose
//!   [`Component::height`] so the caller sizes them with
//!   [`modal_area`](super::style::modal_area).

pub mod agent_list;
pub mod confirm;
pub mod conflict_alert;
pub mod diff_view;
pub mod graph_lanes;
pub mod key_hints;
pub mod layout;
pub mod notification;
pub mod policy_banner;
pub mod repo_picker;
pub mod timeline;

#[cfg(test)]
pub(crate) mod tests;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;

use super::style::Styles;

/// A view model that knows how to paint itself with a theme.
pub trait Component {
    fn render(&self, area: Rect, buf: &mut Buffer, styles: &Styles);

    /// Rows the component wants at a given width (modals, toasts, alerts). Lists and panels
    /// take whatever the layout gives them.
    fn height(&self, _width: u16) -> u16 {
        0
    }
}

/// A component bound to a theme: the ratatui [`Widget`] the frame renders.
pub struct Themed<'a, M: ?Sized> {
    model: &'a M,
    styles: &'a Styles,
}

pub fn themed<'a, M: Component + ?Sized>(model: &'a M, styles: &'a Styles) -> Themed<'a, M> {
    Themed { model, styles }
}

impl<M: Component + ?Sized> Widget for Themed<'_, M> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        self.model.render(area, buf, self.styles);
    }
}
