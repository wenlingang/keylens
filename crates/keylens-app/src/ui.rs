//! Building blocks shared by Keylens' windows (settings, shortcut editor).

use gpui_kit::component::{ActiveTheme, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

/// Every control in a row has this width, so their right edges and left edges line up.
pub const CONTROL_WIDTH: f32 = 176.;

/// A Keylens window other than the panel lost keyboard focus. The controller then checks whether
/// focus left Keylens altogether (see `Keylens::on_window_deactivated`).
pub struct ChildDeactivated;

/// A titled card of rows separated by hairlines, with an optional note under it.
pub fn section(
    title: impl Into<SharedString>,
    rows: Vec<AnyElement>,
    note: Option<SharedString>,
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme();
    v_flex()
        .w_full()
        .gap_1()
        .child(
            div()
                .px_3()
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.muted_foreground)
                .child(title.into()),
        )
        .child(
            v_flex()
                .w_full()
                .rounded(theme.radius_lg)
                .border_1()
                .border_color(theme.border)
                // A 5% foreground tint: one step off the window background in both appearances
                // (the theme's group_box colour equals the background in dark mode).
                .bg(theme.foreground.opacity(0.05))
                .children(rows.into_iter().enumerate().map(|(i, row)| {
                    div()
                        .when(i > 0, |this| this.border_t_1().border_color(theme.border))
                        .child(row)
                })),
        )
        .when_some(note, |this, note| {
            this.child(
                div()
                    .px_3()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(note),
            )
        })
}
