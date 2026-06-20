//! Small reusable gpui element builders (chrome chips, sidebar rows, buttons,
//! status colors) shared across the app's render methods.

use crate::format::short_method;
use crate::{icons, theme};
use gpui::prelude::*;
use gpui::{div, px, Div};
/// A subtle toolbar button (ghost until hovered) â€” Bruno's chrome controls.
pub fn chip(label: &str) -> Div {
    div()
        .px_3()
        .py_1()
        .rounded_md()
        .text_color(theme::subtext())
        .text_size(px(13.))
        .hover(|s| s.bg(theme::surface0()).text_color(theme::text()))
        .child(label.to_string())
}

pub fn icon_chip(label: &str) -> Div {
    div()
        .px_2()
        .py_1()
        .rounded_md()
        .text_color(theme::subtext())
        .text_size(px(12.))
        .child(label.to_string())
}

/// An icon-only chrome button (ghost until hovered), tinted `subtext`.
pub fn svg_chip(name: &str) -> Div {
    div()
        .px_2()
        .py_1()
        .rounded_md()
        .hover(|s| s.bg(theme::surface0()))
        .child(icons::icon(name).size(px(15.)).text_color(theme::subtext()))
}

/// A sidebar request row: colored method badge + name, indented by depth.
pub fn req_row(method: &str, name: &str, active: bool, depth: usize) -> Div {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .pr_2()
        .py_1()
        .pl(px(8. + depth as f32 * 14.))
        .rounded_md()
        .when(active, |d| d.bg(theme::surface0()))
        .when(!active, |d| d.hover(|s| s.bg(theme::mantle())))
        .child(
            div()
                .w(px(36.))
                .text_size(px(10.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .font_family("monospace")
                .text_color(theme::method_color(method))
                .child(short_method(method)),
        )
        .child(
            div()
                .text_size(px(13.))
                .text_color(theme::text())
                .child(name.to_string()),
        )
}

/// A sidebar folder row (chevron reflects collapsed state).
pub fn folder_row(name: &str, depth: usize, collapsed: bool) -> Div {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .pr_2()
        .py_1()
        .pl(px(8. + depth as f32 * 14.))
        .rounded_md()
        .hover(|s| s.bg(theme::mantle()))
        .child(
            icons::icon(if collapsed {
                "chevron-right"
            } else {
                "chevron-down"
            })
            .size(px(14.))
            .text_color(theme::muted()),
        )
        .child(
            icons::icon("folder")
                .size(px(14.))
                .text_color(theme::muted()),
        )
        .child(
            div()
                .ml_1()
                .text_size(px(13.))
                .text_color(theme::text())
                .child(name.to_string()),
        )
}
pub fn status_color(s: u16) -> gpui::Hsla {
    match s {
        200..=299 => theme::green(),
        300..=399 => theme::blue(),
        400..=499 => theme::orange(),
        _ => theme::red(),
    }
}
/// A tab label (request / response sub-tabs).
/// A clickable checkbox box (gpui has no checkbox primitive).
pub fn check_box(on: bool) -> Div {
    div()
        .w(px(14.))
        .h(px(14.))
        .rounded_sm()
        .border_1()
        .border_color(theme::border2())
        .flex()
        .items_center()
        .justify_center()
        .when(on, |d| {
            d.bg(theme::accent()).child(
                div()
                    .text_size(px(9.))
                    .text_color(theme::bg())
                    .child("\u{2713}"),
            )
        })
}

pub fn ghost_btn(label: &str) -> Div {
    div()
        .px_3()
        .py_1()
        .rounded_md()
        .text_size(px(13.))
        .text_color(theme::text())
        .bg(theme::surface0())
        .border_1()
        .border_color(theme::border1())
        .hover(|s| s.border_color(theme::border2()))
        .child(label.to_string())
}

pub fn solid_btn(label: &str) -> Div {
    div()
        .px_4()
        .py_1()
        .rounded_md()
        .text_size(px(13.))
        .bg(theme::accent())
        .text_color(theme::bg())
        .hover(|s| s.opacity(0.92))
        .child(label.to_string())
}

pub fn tab_chip(label: &str, active: bool) -> Div {
    div()
        .px_3()
        .py(px(6.))
        .text_size(px(12.))
        .text_color(if active {
            theme::text()
        } else {
            theme::muted()
        })
        .when(active, |d| {
            d.border_b_1().border_color(theme::tab_underline())
        })
        .when(!active, |d| d.hover(|s| s.text_color(theme::text())))
        .child(label.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_color_buckets() {
        assert_eq!(status_color(204), theme::green());
        assert_eq!(status_color(301), theme::blue());
        assert_eq!(status_color(404), theme::orange());
        assert_eq!(status_color(500), theme::red());
        // Anything outside 2xx-4xx falls through to red.
        assert_eq!(status_color(100), theme::red());
        assert_eq!(status_color(0), theme::red());
    }
}
