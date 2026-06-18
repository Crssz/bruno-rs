//! Coverage tests for `App::view` and the standalone view-builder functions.
//!
//! bru-app is a binary crate, so this runs as an inline submodule with access to
//! private items via `use super::*`. View code just builds an `Element` tree (no
//! renderer / GPU needed): calling each fn and dropping the result executes the
//! branch for coverage. Tests construct `App` in many states and walk every
//! view path — each `ReqTab`/`RespTab`/`RespFormat`, every `Modal`/`Auth`/`Body`
//! variant, the overlays, and the standalone builders — once under the dark
//! palette and once under the light one.
#![allow(unused_imports, clippy::field_reassign_with_default)]
use super::*;

use bru_core::{
    ApiKeyPlacement, Assertion, Auth, Body, FileBodyItem, KeyVal, MultipartField, MultipartValue,
    OAuth2, Request, Var,
};
use bru_engine::{RunOutcome, TestResult};
use bru_http::HttpResponse;
use std::collections::HashSet;
use std::path::PathBuf;

// ── temp-dir helper (Drop-guard cleanup; tempfile crate is unavailable) ───────
struct TempDir(PathBuf);
impl TempDir {
    fn new(tag: &str) -> Self {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let p = std::env::temp_dir().join(format!(
            "bru-view-{tag}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

// A small Bruno collection on disk: bruno.json, two requests, a sub-folder, env.
const REQ_SRC: &str =
    "meta {\n  name: Get One\n  type: http\n  seq: 1\n}\n\nget {\n  url: https://api.test/{{host}}/one\n  body: none\n  auth: none\n}\n\nheaders {\n  Accept: application/json\n  ~X-Off: 1\n}\n\nparams:query {\n  q: 1\n}\n\nassert {\n  res.status: eq 200\n}\n\nvars:pre-request {\n  @tok: secret\n}\n\ndocs {\n  some docs\n}\n";

fn build_collection(tag: &str) -> TempDir {
    let d = TempDir::new(tag);
    let dir = d.path();
    std::fs::write(
        dir.join("bruno.json"),
        "{\n  \"version\": \"1\",\n  \"name\": \"Test Coll\"\n}\n",
    )
    .unwrap();
    std::fs::write(dir.join("one.bru"), REQ_SRC).unwrap();
    std::fs::write(
        dir.join("two.bru"),
        "meta {\n  name: Post Two\n  type: http\n  seq: 2\n}\n\npost {\n  url: https://api.test/two\n  body: json\n  auth: none\n}\n\nbody:json {\n  {\"a\":1}\n}\n",
    )
    .unwrap();
    let sub = dir.join("Sub");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(
        sub.join("folder.bru"),
        "meta {\n  name: Sub\n  type: folder\n  seq: 3\n}\n",
    )
    .unwrap();
    std::fs::write(
        sub.join("inner.bru"),
        "meta {\n  name: Inner\n  type: http\n  seq: 1\n}\n\nget {\n  url: https://api.test/inner\n  body: none\n  auth: none\n}\n",
    )
    .unwrap();
    let envs = dir.join("environments");
    std::fs::create_dir_all(&envs).unwrap();
    std::fs::write(envs.join("dev.bru"), "vars {\n  host: dev.test\n}\n").unwrap();
    d
}

fn parse(s: &str) -> BruFile {
    bru_lang::parse(s).unwrap()
}

fn req(method: &str, url: &str) -> Request {
    Request {
        method: method.to_string(),
        url: url.to_string(),
        ..Default::default()
    }
}

fn resp(status: u16, ct: &str, body: &[u8]) -> HttpResponse {
    HttpResponse {
        status,
        status_text: "OK".to_string(),
        headers: vec![("content-type".to_string(), ct.to_string())],
        body: body.to_vec(),
        duration_ms: 7,
    }
}

/// A `RunOutcome` carrying a real response plus one passing + one failing check.
fn outcome_with_response(ct: &str, body: &[u8]) -> RunOutcome {
    let mut o = RunOutcome::default();
    o.response = Some(resp(200, ct, body));
    o.assertions = vec![
        bru_core::AssertOutcome {
            expr: "res.status".into(),
            operator: "eq".into(),
            expected: "200".into(),
            actual: "200".into(),
            passed: true,
        },
        bru_core::AssertOutcome {
            expr: "res.body".into(),
            operator: "eq".into(),
            expected: "x".into(),
            actual: "y".into(),
            passed: false,
        },
    ];
    o.tests = vec![
        TestResult {
            name: "ok test".into(),
            passed: true,
            error: None,
        },
        TestResult {
            name: "bad test".into(),
            passed: false,
            error: Some("nope".into()),
        },
    ];
    o.console = vec!["log line".into()];
    o
}

/// Run a closure under both palettes, so theme-dependent branches execute twice.
fn both_themes(mut f: impl FnMut()) {
    theme::set_light(false);
    f();
    theme::set_light(true);
    f();
    theme::set_light(false);
}

// ─────────────────────────────────────────────────────────────────────────────
//  App::view — empty / no-collection state
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn view_empty_app() {
    both_themes(|| {
        let app = App::default();
        drop(app.view());
    });
}

#[test]
fn view_console_open_both_devtools_tabs() {
    both_themes(|| {
        let mut app = App::default();
        app.console_open = true;
        // Empty console + empty network first (the "is empty" branches).
        app.devtools_tab = DevTab::Console;
        drop(app.view());
        app.devtools_tab = DevTab::Network;
        drop(app.view());
        // Populated console + network rows (ok and error rows).
        app.console = vec!["hello".into(), "world".into()];
        app.network = vec![
            NetEntry {
                method: "GET".into(),
                url: "https://a/x".into(),
                status: 200,
                ms: 10,
                size: 1234,
                ok: true,
            },
            NetEntry {
                method: "POST".into(),
                url: "https://a/y".into(),
                status: 0,
                ms: 5,
                size: 0,
                ok: false,
            },
        ];
        app.devtools_tab = DevTab::Console;
        drop(app.view());
        app.devtools_tab = DevTab::Network;
        drop(app.view());
    });
}

// ─────────────────────────────────────────────────────────────────────────────
//  App::view — loaded collection, sidebar, tabs, request panel
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn view_loaded_collection_and_sidebar_states() {
    let d = build_collection("loaded");
    both_themes(|| {
        let mut app = App::default();
        app.load(d.path().to_path_buf());
        // No active tab -> sidebar + "Select a request." main panel.
        drop(app.view());
        // With a search filter (forces folders open, filters requests).
        app.search = "inner".to_string();
        drop(app.view());
        app.search = "zzz-no-match".to_string();
        drop(app.view());
        app.search.clear();
        // Collapse a folder so the collapsed-chevron branch runs.
        let sub = d.path().join("Sub");
        app.collapsed.insert(sub.clone());
        drop(app.view());
        app.collapsed.clear();
        // Drag state: hovering a request row as a drop target.
        let one = d.path().join("one.bru");
        app.dragging = Some(one.clone());
        app.drag_over = Some(one.clone());
        drop(app.view());
        app.dragging = None;
        app.drag_over = None;
    });
}

#[test]
fn view_open_request_every_req_tab() {
    let d = build_collection("reqtabs");
    both_themes(|| {
        let mut app = App::default();
        app.load(d.path().to_path_buf());
        app.open_request(d.path().join("one.bru"));
        let i = app.active.unwrap();
        for t in ReqTab::ALL {
            app.tabs[i].req_tab = t;
            load_editors_for(&mut app.tabs[i]);
            drop(app.view());
        }
        // Examples tab is hidden when there are no examples; add one then show it.
        edit::push_text_block(
            &mut app.tabs[i].file,
            "example",
            build_example_text(
                "Ex1",
                &req("GET", "https://a.test"),
                &resp(200, "application/json", b"{\"a\":1}"),
            ),
        );
        app.tabs[i].req_tab = ReqTab::Examples;
        drop(app.view());
    });
}

#[test]
fn view_request_tabs_strip_dirty_and_menu_target() {
    let d = build_collection("strip");
    both_themes(|| {
        let mut app = App::default();
        app.load(d.path().to_path_buf());
        app.open_request(d.path().join("one.bru"));
        app.open_request(d.path().join("two.bru"));
        // Mark one tab dirty so the unsaved-dot branch renders.
        app.tabs[0].dirty = true;
        // Activate the first tab so its active styling differs from the second.
        app.active = Some(0);
        drop(app.view());
    });
}

#[test]
fn view_body_modes_and_var_strip() {
    let d = build_collection("bodies");
    let bodies: Vec<(&str, &str)> = vec![
        ("body: none", ""),
        ("body: json", "\n\nbody:json {\n  {\"k\":\"{{host}}\"}\n}"),
        ("body: text", "\n\nbody:text {\n  hello {{host}}\n}"),
        ("body: xml", "\n\nbody:xml {\n  <x/>\n}"),
        ("body: sparql", "\n\nbody:sparql {\n  SELECT *\n}"),
        (
            "body: graphql",
            "\n\nbody:graphql {\n  { me }\n}\n\nbody:graphql:vars {\n  {\"id\":1}\n}",
        ),
        (
            "body: form-urlencoded",
            "\n\nbody:form-urlencoded {\n  a: 1\n}",
        ),
        (
            "body: multipart-form",
            "\n\nbody:multipart-form {\n  t: v\n  f: @file(/tmp/x)\n}",
        ),
        (
            "body: file",
            "\n\nbody:file {\n  @file(/tmp/a.bin) @contentType(text/plain)\n}",
        ),
    ];
    both_themes(|| {
        for (mode, block) in &bodies {
            let mut app = App::default();
            app.load(d.path().to_path_buf());
            let src = format!(
                "meta {{\n  name: B\n  type: http\n}}\n\npost {{\n  url: https://api.test\n  {mode}\n  auth: none\n}}{block}\n"
            );
            let file = parse(&src);
            let saved = bru_lang::serialize(&file);
            let mut tab = app.blank_tab(None, file, saved);
            tab.req_tab = ReqTab::Body;
            load_editors_for(&mut tab);
            app.tabs.push(tab);
            app.active = Some(app.tabs.len() - 1);
            drop(app.view());
        }
    });
}

#[test]
fn view_file_body_no_selection() {
    // File body with no selected/empty list -> "No file selected" branch.
    both_themes(|| {
        let mut app = App::default();
        let file = parse("meta {\n  name: F\n  type: http\n}\n\nput {\n  url: https://a.test\n  body: file\n  auth: none\n}\n");
        let saved = bru_lang::serialize(&file);
        let mut tab = app.blank_tab(None, file, saved);
        tab.req_tab = ReqTab::Body;
        load_editors_for(&mut tab);
        app.tabs.push(tab);
        app.active = Some(0);
        drop(app.view());
    });
}

#[test]
fn view_bulk_edit_mode_for_each_section() {
    both_themes(|| {
        let mut app = App::default();
        let file = parse(REQ_SRC);
        let saved = bru_lang::serialize(&file);
        let mut tab = app.blank_tab(None, file, saved);
        tab.req_tab = ReqTab::Headers;
        load_editors_for(&mut tab);
        app.tabs.push(tab);
        app.active = Some(0);
        let i = 0;
        // Toggle bulk mode for each KV-table section that has a tab view.
        for (section, rt) in [
            (KvSection::Headers, ReqTab::Headers),
            (KvSection::Query, ReqTab::Params),
            (KvSection::VarsPre, ReqTab::Vars),
        ] {
            app.tabs[i].req_tab = rt;
            app.tabs[i].bulk = Some(section);
            drop(app.view());
        }
        app.tabs[i].bulk = None;
    });
}

#[test]
fn view_source_invalid_path() {
    // A file with no method block -> req is None: req_content shows the
    // "no HTTP method block" fallback editor.
    both_themes(|| {
        let mut app = App::default();
        let file = parse("meta {\n  name: NoMethod\n  type: http\n}\n");
        let saved = bru_lang::serialize(&file);
        let mut tab = app.blank_tab(None, file, saved);
        tab.req_tab = ReqTab::Params; // not Source: triggers the `req is None` arm
        load_editors_for(&mut tab);
        app.tabs.push(tab);
        app.active = Some(0);
        drop(app.view());
        app.tabs[0].req_tab = ReqTab::Source;
        drop(app.view());
    });
}

#[test]
fn view_layout_horizontal_and_vertical() {
    let d = build_collection("layout");
    both_themes(|| {
        let mut app = App::default();
        app.load(d.path().to_path_buf());
        app.open_request(d.path().join("one.bru"));
        app.layout_horizontal = false;
        app.split = 0.6;
        drop(app.view());
        app.layout_horizontal = true;
        drop(app.view());
    });
}

// ─────────────────────────────────────────────────────────────────────────────
//  Response pane — every RespTab x RespFormat, plus image/html/large/error
// ─────────────────────────────────────────────────────────────────────────────

fn app_with_outcome(o: RunOutcome) -> App {
    let mut app = App::default();
    let file = parse(REQ_SRC);
    let saved = bru_lang::serialize(&file);
    let mut tab = app.blank_tab(None, file, saved);
    tab.result = Some(o);
    tab.rebuild_resp_editor();
    app.tabs.push(tab);
    app.active = Some(0);
    app
}

#[test]
fn view_response_pane_all_tabs_and_formats() {
    both_themes(|| {
        let mut app = app_with_outcome(outcome_with_response(
            "application/json",
            b"{\"a\":1,\"b\":[1,2]}",
        ));
        for rt in RespTab::ALL {
            app.tabs[0].resp_tab = rt;
            for fmt in [
                RespFormat::Pretty,
                RespFormat::Raw,
                RespFormat::Hex,
                RespFormat::Tree,
            ] {
                app.tabs[0].resp_format = fmt;
                // For the Tree format also exercise an expanded node.
                if fmt == RespFormat::Tree {
                    app.tabs[0].resp_expanded.insert("$".to_string());
                    app.tabs[0].resp_expanded.insert("$/o:b".to_string());
                }
                app.tabs[0].rebuild_resp_editor();
                drop(app.view());
            }
        }
    });
}

#[test]
fn view_response_tree_non_json() {
    both_themes(|| {
        let mut app = app_with_outcome(outcome_with_response("text/plain", b"not json"));
        app.tabs[0].resp_tab = RespTab::Response;
        app.tabs[0].resp_format = RespFormat::Tree;
        drop(app.view());
    });
}

#[test]
fn view_response_image_and_html() {
    both_themes(|| {
        let app = app_with_outcome(outcome_with_response("image/png", &[0u8, 1, 2, 3]));
        drop(app.view());
        let app2 = app_with_outcome(outcome_with_response("text/html", b"<html></html>"));
        drop(app2.view());
    });
}

#[test]
fn view_response_large_guard_and_reveal() {
    both_themes(|| {
        let big = vec![b'x'; 11 * 1024 * 1024];
        let mut app = app_with_outcome(outcome_with_response("text/plain", &big));
        app.tabs[0].resp_tab = RespTab::Response;
        app.tabs[0].resp_format = RespFormat::Pretty;
        // Guard shown (reveal_large false).
        drop(app.view());
        // Revealed -> renders the body.
        app.tabs[0].reveal_large = true;
        app.tabs[0].rebuild_resp_editor();
        drop(app.view());
    });
}

#[test]
fn view_response_error_and_empty_states() {
    both_themes(|| {
        // Error outcome.
        let mut o = RunOutcome::default();
        o.error = Some("boom".into());
        let app = app_with_outcome(o);
        drop(app.view());
        // Outcome with no response (None) across tabs.
        let app2_o = RunOutcome::default();
        let mut app2 = app_with_outcome(app2_o);
        for rt in RespTab::ALL {
            app2.tabs[0].resp_tab = rt;
            drop(app2.view());
        }
        // No result at all -> "No response yet".
        let mut app3 = App::default();
        let file = parse(REQ_SRC);
        let saved = bru_lang::serialize(&file);
        let tab = app3.blank_tab(None, file, saved);
        app3.tabs.push(tab);
        app3.active = Some(0);
        drop(app3.view());
    });
}

#[test]
fn view_response_no_console_branch() {
    // outcome with empty console list -> the `body` (no console column) branch.
    both_themes(|| {
        let mut o = RunOutcome::default();
        o.response = Some(resp(200, "application/json", b"{}"));
        // console intentionally empty
        let mut app = app_with_outcome(o);
        app.tabs[0].resp_tab = RespTab::Response;
        drop(app.view());
    });
}

// ─────────────────────────────────────────────────────────────────────────────
//  Settings tabs (collection / folder)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn view_settings_tab_every_subtab() {
    let d = build_collection("settings");
    both_themes(|| {
        let mut app = App::default();
        app.load(d.path().to_path_buf());
        app.open_settings(d.path().join("collection.bru"), TabKind::CollectionSettings);
        let i = app.active.unwrap();
        app.tabs[i].dirty = true; // dirty-dot branch in the settings bar
        for t in [
            ReqTab::Headers,
            ReqTab::Vars,
            ReqTab::Auth,
            ReqTab::Script,
            ReqTab::Tests,
            ReqTab::Docs,
            ReqTab::Source,
            ReqTab::Params, // hits the "Not available here." default arm
        ] {
            app.tabs[i].req_tab = t;
            load_editors_for(&mut app.tabs[i]);
            drop(app.view());
        }
        // Folder settings title path (parent folder name).
        app.open_settings(
            d.path().join("Sub").join("folder.bru"),
            TabKind::FolderSettings,
        );
        drop(app.view());
    });
}

// ─────────────────────────────────────────────────────────────────────────────
//  Overlays: menu, var-popup, modal (each variant), env, runner
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn view_menu_overlay_each_target() {
    let d = build_collection("menu");
    both_themes(|| {
        let mut app = App::default();
        app.load(d.path().to_path_buf());
        app.open_request(d.path().join("one.bru"));
        let one = d.path().join("one.bru");
        let sub = d.path().join("Sub");
        for target in [
            MenuTarget::Request(one.clone()),
            MenuTarget::Folder(sub.clone()),
            MenuTarget::Collection,
            MenuTarget::Tab(0),
        ] {
            app.menu = Some(MenuState {
                target,
                at: Point::new(50.0, 60.0),
            });
            drop(app.view());
        }
        // Folder/Collection menu with a clipboard item present -> Paste row.
        app.clipboard_item = Some((one.clone(), false));
        app.menu = Some(MenuState {
            target: MenuTarget::Folder(sub),
            at: Point::ORIGIN,
        });
        drop(app.view());
        app.menu = Some(MenuState {
            target: MenuTarget::Collection,
            at: Point::ORIGIN,
        });
        drop(app.view());
        // Tab menu when the tab is dirty and has a path (Revert/Clone/Copy Path).
        app.tabs[0].dirty = true;
        app.menu = Some(MenuState {
            target: MenuTarget::Tab(0),
            at: Point::ORIGIN,
        });
        drop(app.view());
        // Tab menu for an out-of-range index (the unwrap_or fallback).
        app.menu = Some(MenuState {
            target: MenuTarget::Tab(99),
            at: Point::ORIGIN,
        });
        drop(app.view());
        app.menu = None;
    });
}

#[test]
fn view_var_popup_overlay_variants() {
    both_themes(|| {
        let mut app = App::default();
        // Resolved non-empty value (Copy button present).
        app.var_popup = Some(VarPopup {
            name: "host".into(),
            value: Some("example.com".into()),
        });
        drop(app.view());
        // Resolved empty value -> "(empty)".
        app.var_popup = Some(VarPopup {
            name: "host".into(),
            value: Some(String::new()),
        });
        drop(app.view());
        // Unresolved -> "not set", no Copy button.
        app.var_popup = Some(VarPopup {
            name: "missing".into(),
            value: None,
        });
        drop(app.view());
        app.var_popup = None;
    });
}

#[test]
fn view_modal_overlay_each_variant() {
    let d = build_collection("modal");
    both_themes(|| {
        let mut app = App::default();
        app.load(d.path().to_path_buf());
        app.open_request(d.path().join("one.bru"));
        let p = d.path().join("one.bru");
        let modals = vec![
            Modal::NewRequest {
                dir: d.path().to_path_buf(),
                name: "n".into(),
                method: "GET".into(),
                url: "u".into(),
                error: Some("bad".into()),
            },
            Modal::NewFolder {
                parent: d.path().to_path_buf(),
                name: "f".into(),
                error: None,
            },
            Modal::Rename {
                path: p.clone(),
                is_folder: false,
                name: "r".into(),
                error: None,
            },
            Modal::Clone {
                path: p.clone(),
                is_folder: true,
                name: "c".into(),
                error: Some("dup".into()),
            },
            Modal::Delete {
                path: p.clone(),
                is_folder: false,
                name: "d".into(),
            },
            Modal::Delete {
                path: p.clone(),
                is_folder: true,
                name: "d".into(),
            },
            Modal::ConfirmClose { id: app.tabs[0].id },
            Modal::ConfirmClose { id: 99999 }, // not found -> unwrap_or_default name
            Modal::Palette {
                query: "one".into(),
                selected: 0,
            },
            Modal::SaveExample { name: "ex".into() },
            Modal::Prefs,
            Modal::Code {
                code: "curl -X GET 'x'".into(),
            },
        ];
        for m in modals {
            app.modal = Some(m);
            drop(app.view());
        }
        app.modal = None;
    });
}

#[test]
fn view_palette_empty_results() {
    both_themes(|| {
        // No collection -> palette_results empty -> "No matching requests".
        let mut app = App::default();
        app.modal = Some(Modal::Palette {
            query: "nothing".into(),
            selected: 5,
        });
        drop(app.view());
    });
}

#[test]
fn view_env_overlay_states() {
    let d = build_collection("env");
    both_themes(|| {
        let mut app = App::default();
        app.load(d.path().to_path_buf());
        // No environment selected -> "Select or create" placeholder.
        app.env_editor = Some(EnvEditor::default());
        drop(app.view());
        // A selected env with rows (one secret, one plain) + an error message.
        let mut ed = EnvEditor::default();
        ed.selected = "dev".into();
        ed.rename_buf = "dev".into();
        ed.rows = vec![
            fsops::EnvRow {
                name: "host".into(),
                value: "dev.test".into(),
                enabled: true,
                secret: false,
            },
            fsops::EnvRow {
                name: "key".into(),
                value: "sekret".into(),
                enabled: false,
                secret: true,
            },
        ];
        ed.error = Some("save failed".into());
        app.env_editor = Some(ed);
        drop(app.view());
        app.env_editor = None;
    });
}

#[test]
fn view_runner_overlay_states() {
    both_themes(|| {
        let mut app = App::default();
        // Running, no results yet.
        app.runner = Some(Runner {
            title: "Coll".into(),
            running: true,
            results: vec![],
        });
        drop(app.view());
        // Done, mixed pass/fail (each branch of mark/color + error/no-error detail).
        app.runner = Some(Runner {
            title: "Coll".into(),
            running: false,
            results: vec![
                RunResult {
                    name: "a".into(),
                    passed: true,
                    status: 200,
                    ms: 12,
                    error: None,
                },
                RunResult {
                    name: "b".into(),
                    passed: false,
                    status: 0,
                    ms: 0,
                    error: Some("conn refused".into()),
                },
            ],
        });
        drop(app.view());
        // Done, all passed (the `passed == total` green branch).
        app.runner = Some(Runner {
            title: "Coll".into(),
            running: false,
            results: vec![RunResult {
                name: "a".into(),
                passed: true,
                status: 204,
                ms: 3,
                error: None,
            }],
        });
        drop(app.view());
        app.runner = None;
    });
}

#[test]
fn view_all_overlays_stacked() {
    // Exercise the `view()` layer-stacking branches with everything open at once.
    let d = build_collection("stack");
    both_themes(|| {
        let mut app = App::default();
        app.load(d.path().to_path_buf());
        app.open_request(d.path().join("one.bru"));
        app.console_open = true;
        app.menu = Some(MenuState {
            target: MenuTarget::Collection,
            at: Point::ORIGIN,
        });
        app.var_popup = Some(VarPopup {
            name: "host".into(),
            value: Some("v".into()),
        });
        app.modal = Some(Modal::Prefs);
        app.env_editor = Some(EnvEditor::default());
        app.runner = Some(Runner {
            title: "R".into(),
            running: true,
            results: vec![],
        });
        drop(app.view());
    });
}

// ─────────────────────────────────────────────────────────────────────────────
//  Standalone builder functions
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn builders_dropdown_selected_and_fallback() {
    both_themes(|| {
        // value matches an option (selected branch).
        drop(dropdown(
            pairs(METHODS),
            "GET",
            Length::Fixed(100.0),
            Message::MethodChanged,
        ));
        // value matches nothing (fallback synthesizes an Opt).
        drop(dropdown(
            pairs(METHODS),
            "WEIRD",
            Length::Fixed(100.0),
            Message::MethodChanged,
        ));
    });
}

#[test]
fn builders_kv_and_vars_and_assert_tables() {
    both_themes(|| {
        let kv = vec![
            ("a".to_string(), "1".to_string(), true),
            ("b".to_string(), "2".to_string(), false),
        ];
        drop(kv_table(KvSection::Headers, "Name", "Value", kv.clone()));
        drop(kv_table(KvSection::Query, "Name", "Value", vec![])); // empty rows
        let vrows = vec![
            ("x".to_string(), "1".to_string(), true, false),
            ("y".to_string(), "2".to_string(), false, true),
        ];
        drop(vars_table(KvSection::VarsPre, vrows));
        drop(multipart_table(kv.clone()));
        // assert_table: a unary op row (operand disabled) and a binary op row.
        let arows = vec![
            ("res.body".to_string(), "isArray".to_string(), true),
            ("res.status".to_string(), "eq 200".to_string(), true),
            ("res.x".to_string(), "200".to_string(), false), // bare value -> eq
        ];
        drop(assert_table(arows));
    });
}

#[test]
fn builders_var_pill_and_preview() {
    both_themes(|| {
        drop(var_pill("name", Some("v".to_string())));
        drop(var_pill("name", None));
        // preview: literals + resolved + unresolved pills.
        let e = var_preview("pre {{host}} mid {{missing}} post", |n| {
            if n == "host" {
                Some("x".to_string())
            } else {
                None
            }
        });
        assert!(e.is_some());
        drop(e);
        // No braces -> None.
        assert!(var_preview("plain", |_| None).is_none());
        // Unbalanced -> None.
        assert!(var_preview("dangling {{ never", |_| None).is_none());
    });
}

#[test]
fn builders_editor_box_each_syntax() {
    both_themes(|| {
        let content = text_editor::Content::with_text("body");
        for (field, syn) in [
            (EditorField::Body, "json"),
            (EditorField::GqlQuery, "js"),
            (EditorField::GqlVars, "json"),
            (EditorField::ScriptPre, "js"),
            (EditorField::ScriptPost, "js"),
            (EditorField::Tests, "js"),
            (EditorField::Docs, "md"),
        ] {
            drop(editor_box(&content, field, syn, Length::Fill));
        }
    });
}

#[test]
fn builders_auth_field_and_auth_view_all_variants() {
    both_themes(|| {
        drop(auth_field("Label", "value", AuthField::BasicUser));
        drop(auth_view(&Auth::None));
        drop(auth_view(&Auth::Inherit));
        drop(auth_view(&Auth::Basic {
            username: "u".into(),
            password: "p".into(),
        }));
        drop(auth_view(&Auth::Bearer { token: "t".into() }));
        drop(auth_view(&Auth::ApiKey {
            key: "k".into(),
            value: "v".into(),
            placement: ApiKeyPlacement::Header,
        }));
        drop(auth_view(&Auth::ApiKey {
            key: "k".into(),
            value: "v".into(),
            placement: ApiKeyPlacement::Query,
        }));
        drop(auth_view(&Auth::Digest {
            username: "u".into(),
            password: "p".into(),
        }));
        drop(auth_view(&Auth::AwsV4 {
            access_key_id: "a".into(),
            secret_access_key: "s".into(),
            session_token: "t".into(),
            service: "svc".into(),
            region: "r".into(),
            profile_name: "p".into(),
        }));
        let mut o = OAuth2::default();
        o.grant_type = "client_credentials".into();
        drop(auth_view(&Auth::OAuth2(o)));
        let mut o2 = OAuth2::default();
        o2.grant_type = "password".into();
        drop(auth_view(&Auth::OAuth2(o2)));
    });
}

#[test]
fn builders_settings_widgets() {
    both_themes(|| {
        drop(setting_bool("Flag", "encodeUrl", true));
        drop(setting_bool("Flag", "encodeUrl", false));
        drop(setting_num("Num", "timeout", "30"));
        let file = parse("settings {\n  encodeUrl: true\n  followRedirects: false\n  maxRedirects: 5\n  timeout: 3000\n}\n");
        drop(settings_view(&file));
        // Settings block absent -> empty/false defaults.
        drop(settings_view(&parse(
            "meta {\n  name: X\n  type: http\n}\n",
        )));
    });
}

#[test]
fn builders_header_table_and_misc() {
    both_themes(|| {
        drop(header_table(&[("X".to_string(), "1".to_string())]));
        drop(header_table(&[])); // empty
        drop(section("Title"));
        drop(menu_row("Open", false, Message::CloseMenu));
        drop(menu_row("Delete", true, Message::CloseMenu));
        drop(menu_sep());
        drop(labeled("Name", text("v").size(12)));
        drop(modal_error(&Some("err".to_string())));
        drop(modal_error(&None));
        drop(modal_card_view(
            "T",
            text("body").size(12).into(),
            "OK",
            false,
        ));
        drop(modal_card_view(
            "T",
            text("body").size(12).into(),
            "Delete",
            true,
        ));
        drop(code_block("some code"));
        drop(check_row(true, "passed"));
        drop(check_row(false, "failed"));
        let _ = fill_x();
        let _ = hspace(10.0);
        let _ = vspace(10.0);
        drop(indent(0, text("x").size(12).into()));
        drop(indent(3, text("x").size(12).into()));
    });
}

#[test]
fn builders_json_tree_and_nodes() {
    both_themes(|| {
        let v: serde_json::Value = serde_json::json!({
            "s": "str",
            "n": 42,
            "b": true,
            "z": null,
            "obj": {"k": "v"},
            "arr": [1, 2, {"deep": true}],
        });
        // Collapsed (nothing expanded).
        let empty = HashSet::new();
        drop(json_tree(&v, &empty));
        // Expanded root + nested object + array (recurses through every node kind).
        let mut exp = HashSet::new();
        exp.insert("$".to_string());
        exp.insert("$/o:obj".to_string());
        exp.insert("$/o:arr".to_string());
        exp.insert("$/o:arr/a:2".to_string());
        drop(json_tree(&v, &exp));
        // A bare scalar value at the root.
        drop(json_tree(&serde_json::json!("scalar"), &empty));
        drop(json_tree(&serde_json::json!(1.5), &empty));
        drop(json_tree(&serde_json::Value::Null, &empty));
    });
}

// ─────────────────────────────────────────────────────────────────────────────
//  impl App view-helper methods, called directly
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn impl_view_helpers_directly() {
    let d = build_collection("helpers");
    both_themes(|| {
        let mut app = App::default();
        app.load(d.path().to_path_buf());
        app.open_request(d.path().join("one.bru"));
        let tab_idx = app.active.unwrap();

        drop(app.top_bar());
        drop(app.sidebar());
        drop(app.request_tabs());
        drop(app.main_panel());
        drop(app.console_panel());

        // Overlays via their own methods.
        let menu = MenuState {
            target: MenuTarget::Collection,
            at: Point::ORIGIN,
        };
        drop(app.menu_overlay(&menu));
        let vp = VarPopup {
            name: "host".into(),
            value: Some("x".into()),
        };
        drop(app.var_popup_overlay(&vp));
        drop(app.modal_overlay(&Modal::Prefs));
        let ed = EnvEditor::default();
        drop(app.env_overlay(&ed));
        let r = Runner {
            title: "R".into(),
            running: false,
            results: vec![],
        };
        drop(app.runner_overlay(&r));
        drop(app.palette_view("one", 0));
        drop(app.dismiss_layer(Message::CloseMenu));

        // Request/response sub-views against the open tab.
        // (Clone the request projection so we don't hold an immutable borrow of
        // `app` across the method calls, which take &Tab.)
        let req = app.tabs[tab_idx].file.to_request();
        {
            let tab = &app.tabs[tab_idx];
            drop(app.url_bar(tab, req.as_ref()));
            drop(app.req_tab_strip(tab, req.as_ref()));
            drop(app.req_content(tab, req.as_ref()));
            drop(app.response_pane(tab));
            drop(app.resp_content(tab));
            drop(app.request_settings_view(tab));
            drop(app.body_view(tab, &bru_core::Body::None));
            drop(app.kv_or_bulk(tab, KvSection::Headers, "Name", "Value", vec![]));
            drop(app.vars_or_bulk(tab, KvSection::VarsPre, vec![]));
            drop(app.bulk_view(tab, KvSection::Headers));
            // tab_indicator: with a request, exercise several arms.
            for t in ReqTab::ALL {
                drop(app.tab_indicator(t, tab, req.as_ref()));
            }
            // resp_indicator with the current (no-result) tab.
            for t in RespTab::ALL {
                drop(app.resp_indicator(t, tab));
            }
            // var_strip with and without variables.
            drop(app.var_strip(tab, "no vars here"));
            drop(app.var_strip(tab, "has {{host}} var"));
        }

        // collect_rows directly with an active path + query.
        if let Some(tree) = &app.collection {
            let mut rows: Vec<Element<Message>> = Vec::new();
            app.collect_rows(&tree.root, 0, None, "", &mut rows);
            drop(rows);
            let mut rows2: Vec<Element<Message>> = Vec::new();
            app.collect_rows(&tree.root, 0, None, "inner", &mut rows2);
            drop(rows2);
        }
    });
}

#[test]
fn impl_tab_indicator_and_resp_indicator_populated() {
    // tab_indicator / resp_indicator with rich content so the count/dot branches run.
    both_themes(|| {
        let mut app = App::default();
        let file = parse(
            "meta {\n  name: Rich\n  type: http\n}\n\npost {\n  url: https://a.test\n  body: json\n  auth: bearer\n}\n\nbody:json {\n  {}\n}\n\nauth:bearer {\n  token: t\n}\n\nheaders {\n  A: 1\n}\n\nparams:query {\n  q: 1\n}\n\nassert {\n  res.status: eq 200\n}\n\nvars:pre-request {\n  v: 1\n}\n\nscript:pre-request {\n  console.log('x')\n}\n\ntests {\n  test('t', () => {})\n}\n\ndocs {\n  doc text\n}\n",
        );
        let saved = bru_lang::serialize(&file);
        let mut tab = app.blank_tab(None, file, saved);
        tab.result = Some(outcome_with_response("application/json", b"{}"));
        app.tabs.push(tab);
        app.active = Some(0);
        let req = app.tabs[0].file.to_request();
        let tab = &app.tabs[0];
        for t in ReqTab::ALL {
            drop(app.tab_indicator(t, tab, req.as_ref()));
        }
        for t in RespTab::ALL {
            drop(app.resp_indicator(t, tab));
        }
        // resp_indicator: all-pass case (green) — replace outcome with passing-only.
        drop(app.view());
    });
}

#[test]
fn impl_opt_eq_and_display() {
    // `Opt` compares by value (field 0) but displays its label (field 1).
    let a = Opt("v".to_string(), "Label A".to_string());
    let b = Opt("v".to_string(), "Label B".to_string());
    let c = Opt("other".to_string(), "Label A".to_string());
    // `Opt` has no Debug impl, so compare with the bool form of (Partial)Eq.
    assert!(a == b); // same value, different label -> equal
    assert!(a != c); // different value -> not equal
    assert_eq!(format!("{a}"), "Label A"); // Display writes the label
    assert_eq!(format!("{c}"), "Label A");
}

#[test]
fn view_body_var_strip_resolved_from_pre_var() {
    // A body whose `{{tok}}` matches an enabled pre-request var -> the var_strip
    // lookup hits the request-pre branch (resolved pill, gold).
    both_themes(|| {
        let mut app = App::default();
        let file = parse(
            "meta {\n  name: V\n  type: http\n}\n\npost {\n  url: https://a.test\n  body: text\n  auth: none\n}\n\nbody:text {\n  value is {{tok}}\n}\n\nvars:pre-request {\n  tok: secret\n}\n",
        );
        let saved = bru_lang::serialize(&file);
        let mut tab = app.blank_tab(None, file, saved);
        tab.req_tab = ReqTab::Body;
        load_editors_for(&mut tab);
        app.tabs.push(tab);
        app.active = Some(0);
        drop(app.view());
    });
}

#[test]
fn view_graphql_body_with_var_strip() {
    // GraphQl body whose query/vars contain `{{var}}` -> var_strip Some(..) arm.
    both_themes(|| {
        let mut app = App::default();
        let file = parse(
            "meta {\n  name: G\n  type: http\n}\n\npost {\n  url: https://a.test\n  body: graphql\n  auth: none\n}\n\nbody:graphql {\n  query($id: ID) {{ node(id: {{id}}) }}\n}\n\nbody:graphql:vars {\n  {\"id\": \"{{host}}\"}\n}\n",
        );
        let saved = bru_lang::serialize(&file);
        let mut tab = app.blank_tab(None, file, saved);
        tab.req_tab = ReqTab::Body;
        load_editors_for(&mut tab);
        app.tabs.push(tab);
        app.active = Some(0);
        drop(app.view());
    });
}

#[test]
fn view_multipart_and_file_body_all_arms() {
    // Multipart with a text field, a file field WITH content-type, and a file
    // field WITHOUT; plus a File body with a selected item carrying a CT.
    both_themes(|| {
        let mut app = App::default();
        let mp = parse(
            "meta {\n  name: M\n  type: http\n}\n\npost {\n  url: https://a.test\n  body: multipart-form\n  auth: none\n}\n\nbody:multipart-form {\n  t: plain text\n  withct: @file(/tmp/a) @contentType(text/plain)\n  noct: @file(/tmp/b)\n}\n",
        );
        let saved = bru_lang::serialize(&mp);
        let mut tab = app.blank_tab(None, mp, saved);
        tab.req_tab = ReqTab::Body;
        load_editors_for(&mut tab);
        app.tabs.push(tab);
        app.active = Some(0);
        drop(app.view());

        // File body with a real selected item path (the path-shown branch).
        let mut app2 = App::default();
        let f = parse(
            "meta {\n  name: F\n  type: http\n}\n\nput {\n  url: https://a.test\n  body: file\n  auth: none\n}\n\nbody:file {\n  @file(/tmp/a.bin) @contentType(application/octet-stream)\n}\n",
        );
        let saved2 = bru_lang::serialize(&f);
        let mut tab2 = app2.blank_tab(None, f, saved2);
        tab2.req_tab = ReqTab::Body;
        load_editors_for(&mut tab2);
        app2.tabs.push(tab2);
        app2.active = Some(0);
        drop(app2.view());
    });
}

#[test]
fn impl_body_view_every_variant_directly() {
    // Call body_view directly with each Body variant so the multipart/file
    // decorator arms (3683-3702) and form/graphql arms execute deterministically.
    both_themes(|| {
        let mut app = App::default();
        let file = parse(REQ_SRC);
        let saved = bru_lang::serialize(&file);
        let tab = app.blank_tab(None, file, saved);
        app.tabs.push(tab);
        app.active = Some(0);
        let tab = &app.tabs[0];

        drop(app.body_view(tab, &Body::None));
        drop(app.body_view(tab, &Body::Json("{}".into())));
        drop(app.body_view(tab, &Body::Text("hi {{host}}".into())));
        drop(app.body_view(tab, &Body::Xml("<x/>".into())));
        drop(app.body_view(tab, &Body::Sparql("SELECT *".into())));
        drop(app.body_view(
            tab,
            &Body::GraphQl {
                query: "{ {{q}} }".into(),
                variables: "{}".into(),
            },
        ));
        drop(app.body_view(
            tab,
            &Body::FormUrlEncoded(vec![KeyVal {
                name: "a".into(),
                value: "1".into(),
                enabled: true,
            }]),
        ));
        // Multipart: text field, file WITH content-type, file WITHOUT.
        drop(app.body_view(
            tab,
            &Body::MultipartForm(vec![
                MultipartField {
                    name: "t".into(),
                    value: MultipartValue::Text("v".into()),
                    content_type: None,
                    enabled: true,
                },
                MultipartField {
                    name: "withct".into(),
                    value: MultipartValue::File("/tmp/a".into()),
                    content_type: Some("text/plain".into()),
                    enabled: true,
                },
                MultipartField {
                    name: "noct".into(),
                    value: MultipartValue::File("/tmp/b".into()),
                    content_type: None,
                    enabled: false,
                },
            ]),
        ));
        // File body: a selected item with a content-type (path-shown branch).
        drop(app.body_view(
            tab,
            &Body::File(vec![
                FileBodyItem {
                    path: "/tmp/a.bin".into(),
                    content_type: Some("application/octet-stream".into()),
                    selected: true,
                },
                FileBodyItem {
                    path: "/tmp/b.bin".into(),
                    content_type: None,
                    selected: false,
                },
            ]),
        ));
        // File body: empty list -> "No file selected".
        drop(app.body_view(tab, &Body::File(vec![])));
    });
}

#[test]
fn view_request_settings_with_tags() {
    // A request carrying meta tags -> the chips loop (3593-3597) renders a chip.
    both_themes(|| {
        let mut app = App::default();
        let file = parse(
            "meta {\n  name: T\n  type: http\n  tags: [\n    smoke\n    regression\n  ]\n}\n\nget {\n  url: https://a.test\n  body: none\n  auth: none\n}\n",
        );
        let saved = bru_lang::serialize(&file);
        let mut tab = app.blank_tab(None, file, saved);
        tab.req_tab = ReqTab::Settings;
        load_editors_for(&mut tab);
        app.tabs.push(tab);
        app.active = Some(0);
        drop(app.view());
    });
}

#[test]
fn impl_url_bar_with_var_preview_resolution() {
    // url_bar's preview lookup: request pre-vars override cached env vars.
    both_themes(|| {
        let mut app = App::default();
        app.vars
            .insert("host".to_string(), "env.example".to_string());
        let file = parse(REQ_SRC); // url contains {{host}}, vars:pre has @tok
        let saved = bru_lang::serialize(&file);
        let mut tab = app.blank_tab(None, file, saved);
        load_editors_for(&mut tab);
        app.tabs.push(tab);
        app.active = Some(0);
        drop(app.view());

        // A URL whose `{{tok}}` matches an enabled pre-request var -> the
        // preview lookup returns from the request-pre branch (3341-3343).
        let mut app2 = App::default();
        let file2 = parse(
            "meta {\n  name: U\n  type: http\n}\n\nget {\n  url: https://a.test/{{tok}}\n  body: none\n  auth: none\n}\n\nvars:pre-request {\n  tok: hello\n}\n",
        );
        let saved2 = bru_lang::serialize(&file2);
        let mut tab2 = app2.blank_tab(None, file2, saved2);
        load_editors_for(&mut tab2);
        app2.tabs.push(tab2);
        app2.active = Some(0);
        drop(app2.view());
    });
}
