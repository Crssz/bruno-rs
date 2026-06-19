// Phase 2: the gpui app skeleton — the full shell (top bar / sidebar / center /
// status bar) styled to match the iced app, with the response pane showing real
// tree-sitter-highlighted JSON. Static sample data for now; real collection
// loading + editing land in later phases.
mod highlight;
mod theme;

use gpui::{
    div, prelude::*, px, size, App, Bounds, Context, SharedString, StyledText, Window, WindowBounds,
    WindowOptions,
};
use gpui_platform::application;

const SAMPLE: &str = r#"{
  "total_count": 30822,
  "incomplete_results": false,
  "items": [
    {
      "id": 542284380,
      "name": "bruno",
      "full_name": "usebruno/bruno",
      "private": false,
      "owner": {
        "login": "usebruno",
        "id": 114530840,
        "type": "Organization"
      },
      "stargazers_count": 38211,
      "topics": ["api", "rust", "graphql"],
      "license": null
    }
  ]
}"#;

/// A pill/button used in the chrome (ghost style).
fn chip(label: &str) -> impl IntoElement {
    div()
        .px_3()
        .py_1()
        .rounded_md()
        .bg(theme::surface0())
        .text_color(theme::text())
        .text_size(px(13.))
        .child(label.to_string())
}

fn icon_chip(label: &str) -> impl IntoElement {
    div()
        .px_2()
        .py_1()
        .rounded_md()
        .text_color(theme::subtext())
        .text_size(px(12.))
        .child(label.to_string())
}

/// A sidebar request row: colored method badge + name.
fn req_row(method: &str, name: &str, active: bool) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .px_2()
        .py_1()
        .rounded_md()
        .when(active, |d| d.bg(theme::surface0()))
        .child(
            div()
                .w(px(36.))
                .text_size(px(10.))
                .font_family("monospace")
                .text_color(theme::method_color(method))
                .child(method.to_string()),
        )
        .child(
            div()
                .text_size(px(13.))
                .text_color(if active {
                    theme::text()
                } else {
                    theme::subtext()
                })
                .child(name.to_string()),
        )
}

/// A tab label (request sub-tabs / response sub-tabs).
fn tab(label: &str, active: bool) -> impl IntoElement {
    div()
        .px_3()
        .py_1()
        .text_size(px(12.))
        .text_color(if active {
            theme::text()
        } else {
            theme::muted()
        })
        .when(active, |d| {
            d.border_b_1().border_color(theme::accent())
        })
        .child(label.to_string())
}

fn top_bar() -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_3()
        .w_full()
        .px_3()
        .py_2()
        .bg(theme::mantle())
        .border_b_1()
        .border_color(theme::border1())
        .child(icon_chip("\u{2302}")) // home
        .child(chip("Open Collection"))
        .child(chip("New"))
        .child(
            div()
                .text_color(theme::accent())
                .text_size(px(13.))
                .child("GitHub REST API"),
        )
        .child(
            div()
                .text_color(theme::muted())
                .text_size(px(12.))
                .child("\u{2022} main"),
        )
        .child(div().flex_1())
        .child(
            div()
                .text_color(theme::muted())
                .text_size(px(12.))
                .child("Env:"),
        )
        .child(chip("Prod"))
        .child(icon_chip("Vault"))
        .child(icon_chip("Eye"))
        .child(icon_chip("Theme"))
}

fn url_bar() -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .w_full()
        .px_2()
        .py_2()
        .bg(theme::mantle())
        .border_b_1()
        .border_color(theme::border1())
        .child(
            div()
                .px_2()
                .py_1()
                .rounded_md()
                .bg(theme::surface0())
                .text_color(theme::green())
                .text_size(px(12.))
                .font_family("monospace")
                .child("GET"),
        )
        .child(
            div()
                .flex_1()
                .px_2()
                .py_1()
                .rounded_md()
                .bg(theme::input_bg())
                .border_1()
                .border_color(theme::border1())
                .text_color(theme::text())
                .text_size(px(13.))
                .font_family("monospace")
                .child("{{baseUrl}}/search/repositories?q=brun&sort=stars"),
        )
        .child(icon_chip("</>"))
        .child(chip("Save"))
        .child(
            div()
                .px_3()
                .py_1()
                .rounded_md()
                .bg(theme::accent())
                .text_color(theme::bg())
                .text_size(px(13.))
                .child("Send \u{2192}"),
        )
}

fn req_subtabs() -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .w_full()
        .px_2()
        .bg(theme::surface0())
        .border_b_1()
        .border_color(theme::border2())
        .child(tab("Params", true))
        .child(tab("Body", false))
        .child(tab("Headers", false))
        .child(tab("Auth", false))
        .child(tab("Vars", false))
        .child(tab("Script", false))
        .child(tab("Assert", false))
        .child(tab("Tests", false))
        .child(tab("Docs", false))
}

fn resp_header() -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .w_full()
        .px_2()
        .py_1()
        .bg(theme::surface0())
        .border_b_1()
        .border_color(theme::border2())
        .child(tab("Response", true))
        .child(tab("Headers", false))
        .child(tab("Timeline", false))
        .child(tab("Tests", false))
        .child(div().flex_1())
        .child(
            div()
                .text_color(theme::green())
                .text_size(px(12.))
                .child("200 OK"),
        )
        .child(
            div()
                .text_color(theme::subtext())
                .text_size(px(12.))
                .child("849 ms"),
        )
        .child(
            div()
                .text_color(theme::subtext())
                .text_size(px(12.))
                .child("153.78 KB"),
        )
}

fn status_bar() -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_3()
        .w_full()
        .px_3()
        .py_1()
        .bg(theme::mantle())
        .border_t_1()
        .border_color(theme::border1())
        .child(div().flex_1())
        .child(icon_chip("Search"))
        .child(icon_chip("Cookies"))
        .child(icon_chip("Dev Tools"))
        .child(
            div()
                .text_color(theme::muted())
                .text_size(px(11.))
                .child("v0.0.0"),
        )
}

struct BruApp {
    code: SharedString,
}

impl BruApp {
    fn sidebar(&self) -> impl IntoElement {
        let rows = [
            ("GET", "Search Repos", true),
            ("GET", "Repository Info", false),
            ("GET", "Repository Tags", false),
            ("GET", "Search Issues", false),
            ("POST", "Create Issue", false),
            ("GET", "Search Users", false),
            ("DELETE", "Delete Repo", false),
        ];
        div()
            .flex()
            .flex_col()
            .gap_1()
            .w(px(260.))
            .h_full()
            .bg(theme::bg())
            .border_r_1()
            .border_color(theme::border1())
            .p_2()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .px_1()
                    .py_1()
                    .child(
                        div()
                            .text_color(theme::muted())
                            .text_size(px(12.))
                            .child("GITHUB REST API"),
                    )
                    .child(div().text_color(theme::subtext()).child("+")),
            )
            .child(
                div()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .bg(theme::input_bg())
                    .border_1()
                    .border_color(theme::border1())
                    .text_color(theme::muted())
                    .text_size(px(12.))
                    .child("Search..."),
            )
            .children(rows.into_iter().map(|(m, n, a)| req_row(m, n, a)))
    }

    fn response_body(&self, window: &mut Window) -> impl IntoElement {
        let mut base = window.text_style();
        base.font_family = "monospace".into();
        base.color = theme::text();
        base.font_size = px(13.).into();
        let spans = highlight::json(&self.code);
        div()
            .id("resp")
            .overflow_y_scroll()
            .flex_1()
            .w_full()
            .bg(theme::bg())
            .p_3()
            .font_family("monospace")
            .text_size(px(13.))
            .line_height(px(19.))
            .child(StyledText::new(self.code.clone()).with_default_highlights(&base, spans))
    }
}

impl Render for BruApp {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let center = div()
            .flex()
            .flex_col()
            .flex_1()
            .h_full()
            .child(url_bar())
            .child(req_subtabs())
            .child(resp_header())
            .child(self.response_body(window));

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme::bg())
            .text_color(theme::text())
            .child(top_bar())
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .w_full()
                    .child(self.sidebar())
                    .child(center),
            )
            .child(status_bar())
    }
}

fn main() {
    application().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1100.), px(720.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| {
                cx.new(|_| BruApp {
                    code: SAMPLE.into(),
                })
            },
        )
        .unwrap();
        cx.activate(true);
    });
}
