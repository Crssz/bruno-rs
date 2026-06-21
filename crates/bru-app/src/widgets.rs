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

#[cfg(test)]
mod cov_tests {
    use super::*;

    // Each widget builder below is a pure `Div`-builder: calling it runs the
    // whole construction body (and the `theme::*` / `icons::icon` / `short_method`
    // / `method_color` calls nested inside). `Div` has no useful equality or
    // Debug, so we exercise the builders for coverage and discard the element.
    // The existing `mod tests` already covers the `status_color` buckets; here we
    // add the remaining builders plus both branches of each `when(..)` toggle.

    #[test]
    fn chip_builds() {
        let _ = chip("Save");
        let _ = chip("");
    }

    #[test]
    fn icon_chip_builds() {
        let _ = icon_chip("GET");
        let _ = icon_chip("");
    }

    #[test]
    fn svg_chip_builds() {
        // Goes through `icons::icon(name)` for a known and an unknown name.
        let _ = svg_chip("folder");
        let _ = svg_chip("chevron-down");
        let _ = svg_chip("does-not-exist");
    }

    #[test]
    fn req_row_active_and_inactive() {
        // active=true takes the `.when(active, ..)` bg branch; active=false takes
        // the `.when(!active, ..)` hover branch. depth feeds the pl() math and
        // method feeds short_method/method_color.
        let _ = req_row("GET", "List users", true, 0);
        let _ = req_row("POST", "Create user", false, 2);
        let _ = req_row("DELETE", "Remove", false, 1);
        let _ = req_row("", "No method", true, 3);
    }

    #[test]
    fn folder_row_collapsed_and_expanded() {
        // collapsed=true -> "chevron-right", collapsed=false -> "chevron-down".
        let _ = folder_row("api", 0, true);
        let _ = folder_row("nested", 2, false);
    }

    #[test]
    fn check_box_on_and_off() {
        // on=true takes the `.when(on, ..)` filled-checkmark branch; off skips it.
        let _ = check_box(true);
        let _ = check_box(false);
    }

    #[test]
    fn ghost_btn_builds() {
        let _ = ghost_btn("Cancel");
        let _ = ghost_btn("");
    }

    #[test]
    fn solid_btn_builds() {
        let _ = solid_btn("Send");
        let _ = solid_btn("");
    }

    #[test]
    fn tab_chip_active_and_inactive() {
        // active=true -> text color + underline branch; active=false -> muted +
        // hover branch.
        let _ = tab_chip("Body", true);
        let _ = tab_chip("Headers", false);
    }

    #[test]
    fn status_color_boundary_values() {
        // Boundaries of each bucket, complementing the existing mid-range test.
        assert_eq!(status_color(200), theme::green());
        assert_eq!(status_color(299), theme::green());
        assert_eq!(status_color(300), theme::blue());
        assert_eq!(status_color(399), theme::blue());
        assert_eq!(status_color(400), theme::orange());
        assert_eq!(status_color(499), theme::orange());
        // Below 200 and at/above 500 fall through to red.
        assert_eq!(status_color(199), theme::red());
        assert_eq!(status_color(500), theme::red());
        assert_eq!(status_color(u16::MAX), theme::red());
    }
}
