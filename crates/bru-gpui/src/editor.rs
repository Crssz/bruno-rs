//! A minimal editable, multi-line code editor on gpui.
//!
//! Extends gpui's single-line `input` example to multiple lines: typing (via the
//! `EntityInputHandler` IME path), Enter/Backspace/Delete, arrow + Home/End
//! navigation, click-to-position, selection, and clipboard. Rendered by a custom
//! `Element` that shapes each line and paints the cursor/selection. Highlighting
//! is a follow-up; v1 is single-color monospace.

use std::ops::Range;

use gpui::{
    actions, div, fill, point, prelude::*, px, relative, size, App, Bounds, ClipboardItem, Context,
    CursorStyle, Element, ElementId, ElementInputHandler, Entity, EntityInputHandler, FocusHandle,
    Focusable, Font, GlobalElementId, HighlightStyle, Hsla, LayoutId, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point, ShapedLine, Style, TextRun,
    UTF16Selection, Window,
};
use unicode_segmentation::UnicodeSegmentation;

use crate::{highlight, theme};

/// What grammar (if any) to highlight the buffer with.
#[derive(Clone, Copy, PartialEq)]
pub enum Lang {
    Plain,
    Json,
}

actions!(
    code_editor,
    [
        Backspace, Delete, Left, Right, Up, Down, Home, End, Enter, Tab, SelectLeft, SelectRight,
        SelectUp, SelectDown, SelectAll, Paste, Cut, Copy,
    ]
);

/// Bind the editor's keys once at startup.
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        gpui::KeyBinding::new("backspace", Backspace, Some("CodeEditor")),
        gpui::KeyBinding::new("delete", Delete, Some("CodeEditor")),
        gpui::KeyBinding::new("left", Left, Some("CodeEditor")),
        gpui::KeyBinding::new("right", Right, Some("CodeEditor")),
        gpui::KeyBinding::new("up", Up, Some("CodeEditor")),
        gpui::KeyBinding::new("down", Down, Some("CodeEditor")),
        gpui::KeyBinding::new("home", Home, Some("CodeEditor")),
        gpui::KeyBinding::new("end", End, Some("CodeEditor")),
        gpui::KeyBinding::new("enter", Enter, Some("CodeEditor")),
        gpui::KeyBinding::new("tab", Tab, Some("CodeEditor")),
        gpui::KeyBinding::new("shift-left", SelectLeft, Some("CodeEditor")),
        gpui::KeyBinding::new("shift-right", SelectRight, Some("CodeEditor")),
        gpui::KeyBinding::new("shift-up", SelectUp, Some("CodeEditor")),
        gpui::KeyBinding::new("shift-down", SelectDown, Some("CodeEditor")),
        gpui::KeyBinding::new("ctrl-a", SelectAll, Some("CodeEditor")),
        gpui::KeyBinding::new("ctrl-c", Copy, Some("CodeEditor")),
        gpui::KeyBinding::new("ctrl-x", Cut, Some("CodeEditor")),
        gpui::KeyBinding::new("ctrl-v", Paste, Some("CodeEditor")),
    ]);
}

pub struct CodeEditor {
    focus_handle: FocusHandle,
    content: String,
    /// Selection as a byte range; cursor is the moving end.
    selected_range: Range<usize>,
    selection_reversed: bool,
    is_selecting: bool,
    lang: Lang,
    /// One line tall; Enter/Tab suppressed, paste/IME strip newlines. For the URL.
    single_line: bool,
    /// Cached tree-sitter highlight spans (recomputed on every content change).
    spans: Vec<(Range<usize>, HighlightStyle)>,
    // Layout caches (filled during paint) for mouse mapping.
    line_layouts: Vec<ShapedLine>,
    bounds: Option<Bounds<Pixels>>,
    line_height: Pixels,
}

impl CodeEditor {
    pub fn new(cx: &mut Context<Self>, text: &str) -> Self {
        let mut ed = Self {
            focus_handle: cx.focus_handle(),
            content: text.to_string(),
            selected_range: 0..0,
            selection_reversed: false,
            is_selecting: false,
            lang: Lang::Plain,
            single_line: false,
            spans: Vec::new(),
            line_layouts: Vec::new(),
            bounds: None,
            line_height: px(19.),
        };
        ed.recompute_highlight();
        ed
    }

    /// A single-line variant (for the URL field): one line tall, no newlines.
    pub fn single_line(cx: &mut Context<Self>, text: &str) -> Self {
        let mut ed = Self::new(cx, text);
        ed.single_line = true;
        ed
    }

    /// Replace the (single-line) content, keeping single-line mode.
    pub fn set_line(&mut self, text: &str, cx: &mut Context<Self>) {
        self.set_text(text, Lang::Plain, cx);
    }

    pub fn set_text(&mut self, text: &str, lang: Lang, cx: &mut Context<Self>) {
        self.content = text.to_string();
        self.lang = lang;
        self.selected_range = 0..0;
        self.selection_reversed = false;
        self.recompute_highlight();
        cx.notify();
    }

    /// Recompute cached highlight spans for the current content + language.
    fn recompute_highlight(&mut self) {
        self.spans = match self.lang {
            Lang::Json => highlight::json(&self.content),
            Lang::Plain => Vec::new(),
        };
    }

    pub fn text(&self) -> &str {
        &self.content
    }

    fn cursor(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    // ── offset / line math ──────────────────────────────────────────────────
    /// Byte offset at the start of each visual line.
    fn line_starts(&self) -> Vec<usize> {
        let mut starts = vec![0];
        for (i, b) in self.content.bytes().enumerate() {
            if b == b'\n' {
                starts.push(i + 1);
            }
        }
        starts
    }

    fn line_of(&self, offset: usize) -> usize {
        self.content[..offset].bytes().filter(|b| *b == b'\n').count()
    }

    fn prev_grapheme(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .rev()
            .find_map(|(i, _)| (i < offset).then_some(i))
            .unwrap_or(0)
    }

    fn next_grapheme(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .find_map(|(i, _)| (i > offset).then_some(i))
            .unwrap_or(self.content.len())
    }

    fn line_end(&self, line_start: usize) -> usize {
        self.content[line_start..]
            .find('\n')
            .map(|n| line_start + n)
            .unwrap_or(self.content.len())
    }

    // ── movement ────────────────────────────────────────────────────────────
    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.selected_range = offset..offset;
        self.selection_reversed = false;
        cx.notify();
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        if self.selection_reversed {
            self.selected_range.start = offset;
        } else {
            self.selected_range.end = offset;
        }
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        cx.notify();
    }

    /// Move cursor up/down `delta` lines, keeping the byte column where possible.
    fn vertical(&self, delta: isize) -> usize {
        let starts = self.line_starts();
        let cur = self.cursor();
        let line = self.line_of(cur);
        let col = cur - starts[line];
        let target = (line as isize + delta).clamp(0, starts.len() as isize - 1) as usize;
        let tstart = starts[target];
        let tend = self.line_end(tstart);
        (tstart + col).min(tend)
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        let o = if self.selected_range.is_empty() {
            self.prev_grapheme(self.cursor())
        } else {
            self.selected_range.start
        };
        self.move_to(o, cx);
    }
    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        let o = if self.selected_range.is_empty() {
            self.next_grapheme(self.cursor())
        } else {
            self.selected_range.end
        };
        self.move_to(o, cx);
    }
    fn up(&mut self, _: &Up, _: &mut Window, cx: &mut Context<Self>) {
        let o = if self.single_line { 0 } else { self.vertical(-1) };
        self.move_to(o, cx);
    }
    fn down(&mut self, _: &Down, _: &mut Window, cx: &mut Context<Self>) {
        let o = if self.single_line {
            self.content.len()
        } else {
            self.vertical(1)
        };
        self.move_to(o, cx);
    }
    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        let starts = self.line_starts();
        let o = starts[self.line_of(self.cursor())];
        self.move_to(o, cx);
    }
    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        let starts = self.line_starts();
        let o = self.line_end(starts[self.line_of(self.cursor())]);
        self.move_to(o, cx);
    }
    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        let o = self.prev_grapheme(self.cursor());
        self.select_to(o, cx);
    }
    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        let o = self.next_grapheme(self.cursor());
        self.select_to(o, cx);
    }
    fn select_up(&mut self, _: &SelectUp, _: &mut Window, cx: &mut Context<Self>) {
        let o = self.vertical(-1);
        self.select_to(o, cx);
    }
    fn select_down(&mut self, _: &SelectDown, _: &mut Window, cx: &mut Context<Self>) {
        let o = self.vertical(1);
        self.select_to(o, cx);
    }
    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.selected_range = 0..self.content.len();
        self.selection_reversed = false;
        cx.notify();
    }

    // ── editing ─────────────────────────────────────────────────────────────
    fn replace(&mut self, new_text: &str, cx: &mut Context<Self>) {
        let owned;
        let new_text = if self.single_line && new_text.contains('\n') {
            owned = new_text.replace('\n', " ");
            owned.as_str()
        } else {
            new_text
        };
        let r = self.selected_range.clone();
        self.content
            .replace_range(r.clone(), new_text); // r is a valid char-boundary range
        let at = r.start + new_text.len();
        self.selected_range = at..at;
        self.selection_reversed = false;
        self.recompute_highlight();
        cx.notify();
    }

    fn backspace(&mut self, _: &Backspace, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let prev = self.prev_grapheme(self.cursor());
            if prev == self.cursor() {
                return;
            }
            self.selected_range = prev..self.cursor();
        }
        self.replace("", cx);
    }
    fn delete(&mut self, _: &Delete, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let next = self.next_grapheme(self.cursor());
            if next == self.cursor() {
                return;
            }
            self.selected_range = self.cursor()..next;
        }
        self.replace("", cx);
    }
    fn enter(&mut self, _: &Enter, _: &mut Window, cx: &mut Context<Self>) {
        if self.single_line {
            return;
        }
        self.replace("\n", cx);
    }
    fn tab(&mut self, _: &Tab, _: &mut Window, cx: &mut Context<Self>) {
        if self.single_line {
            return;
        }
        self.replace("  ", cx);
    }
    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
        }
    }
    fn cut(&mut self, _: &Cut, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
            self.replace("", cx);
        }
    }
    fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|i| i.text()) {
            self.replace(&text, cx);
        }
    }

    // ── mouse ───────────────────────────────────────────────────────────────
    fn index_for_position(&self, position: Point<Pixels>) -> usize {
        let Some(bounds) = self.bounds.as_ref() else {
            return 0;
        };
        if self.line_layouts.is_empty() {
            return 0;
        }
        let starts = self.line_starts();
        let rel = f32::from(position.y - bounds.top());
        let lh = f32::from(self.line_height).max(1.0);
        let line = ((rel / lh).max(0.0) as usize).min(self.line_layouts.len() - 1);
        let col = self.line_layouts[line].closest_index_for_x(position.x - bounds.left());
        let lstart = starts[line];
        (lstart + col).min(self.line_end(lstart))
    }

    fn on_mouse_down(&mut self, e: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.is_selecting = true;
        window.focus(&self.focus_handle, cx);
        let idx = self.index_for_position(e.position);
        if e.modifiers.shift {
            self.select_to(idx, cx);
        } else {
            self.move_to(idx, cx);
        }
    }
    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }
    fn on_mouse_move(&mut self, e: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting {
            let idx = self.index_for_position(e.position);
            self.select_to(idx, cx);
        }
    }
}

impl Focusable for CodeEditor {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for CodeEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .key_context("CodeEditor")
            .track_focus(&self.focus_handle(cx))
            .cursor(CursorStyle::IBeam)
            .size_full()
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::up))
            .on_action(cx.listener(Self::down))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::enter))
            .on_action(cx.listener(Self::tab))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_up))
            .on_action(cx.listener(Self::select_down))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::paste))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .child(EditorElement {
                editor: cx.entity(),
            })
    }
}

impl EntityInputHandler for CodeEditor {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        actual: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let r = self.clamp(range);
        actual.replace(r.clone());
        Some(self.content[r].to_string())
    }
    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.selected_range.clone(),
            reversed: self.selection_reversed,
        })
    }
    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        None
    }
    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {}
    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(r) = range {
            self.selected_range = self.clamp(r);
        }
        self.replace(new_text, cx);
    }
    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        new_text: &str,
        _: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(r) = range {
            self.selected_range = self.clamp(r);
        }
        self.replace(new_text, cx);
    }
    fn bounds_for_range(
        &mut self,
        _range: Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        Some(bounds)
    }
    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        Some(self.index_for_position(point))
    }
}

impl CodeEditor {
    /// Clamp a byte range to char boundaries within the content.
    fn clamp(&self, r: Range<usize>) -> Range<usize> {
        let len = self.content.len();
        let mut s = r.start.min(len);
        let mut e = r.end.min(len);
        while !self.content.is_char_boundary(s) && s > 0 {
            s -= 1;
        }
        while !self.content.is_char_boundary(e) && e < len {
            e += 1;
        }
        s..e.max(s)
    }
}

/// Build per-line `TextRun`s from the cached highlight spans, filling gaps with
/// the default color. `spans` must be sorted by start (tree-sitter emits them in
/// document order) and non-overlapping.
fn build_line_runs(
    line: &str,
    lstart: usize,
    spans: &[(Range<usize>, HighlightStyle)],
    font: &Font,
    default: Hsla,
) -> Vec<TextRun> {
    if line.is_empty() {
        return vec![];
    }
    let lend = lstart + line.len();
    let mk = |len: usize, color: Hsla| TextRun {
        len,
        font: font.clone(),
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let mut runs = Vec::new();
    let mut pos = lstart;
    for (range, style) in spans {
        if range.end <= lstart || range.start >= lend {
            continue;
        }
        let s = range.start.max(lstart);
        let e = range.end.min(lend);
        if s > pos {
            runs.push(mk(s - pos, default));
        }
        if e > s {
            runs.push(mk(e - s, style.color.unwrap_or(default)));
        }
        if e > pos {
            pos = e;
        }
    }
    if pos < lend {
        runs.push(mk(lend - pos, default));
    }
    runs
}

/// The custom element that shapes lines and paints cursor/selection.
struct EditorElement {
    editor: Entity<CodeEditor>,
}

struct EditorPrepaint {
    lines: Vec<ShapedLine>,
    cursor: Option<PaintQuad>,
    selections: Vec<PaintQuad>,
}

impl IntoElement for EditorElement {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for EditorElement {
    type RequestLayoutState = ();
    type PrepaintState = EditorPrepaint;

    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let editor = self.editor.read(cx);
        let line_count = if editor.single_line {
            1
        } else {
            editor.content.split('\n').count().max(1)
        };
        let lh = window.line_height();
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = (lh * line_count as f32).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> EditorPrepaint {
        let editor = self.editor.read(cx);
        let style = window.text_style();
        let font_size = style.font_size.to_pixels(window.rem_size());
        let lh = window.line_height();
        let text_color: Hsla = theme::text();

        let starts = editor.line_starts();
        let font = style.font();
        let mut lines = Vec::new();
        for (i, line) in editor.content.split('\n').enumerate() {
            let runs = build_line_runs(line, starts[i], &editor.spans, &font, text_color);
            let shaped = window.text_system().shape_line(
                gpui::SharedString::from(line.to_string()),
                font_size,
                &runs,
                None,
            );
            lines.push((shaped, starts[i]));
        }

        // Cursor.
        let cur = editor.cursor();
        let cline = editor.line_of(cur);
        let (cshaped, cstart) = &lines[cline];
        let cx_px = cshaped.x_for_index(cur - cstart);
        let cursor = Some(fill(
            Bounds::new(
                point(bounds.left() + cx_px, bounds.top() + lh * cline as f32),
                size(px(1.5), lh),
            ),
            theme::accent(),
        ));

        // Selection (per line).
        let sel = editor.selected_range.clone();
        let mut selections = Vec::new();
        if !sel.is_empty() {
            for (i, (shaped, lstart)) in lines.iter().enumerate() {
                let lend = lstart + shaped.text.len();
                let a = sel.start.max(*lstart);
                let b = sel.end.min(lend);
                let spans_newline = sel.end > lend;
                if a <= b && (a < b || (spans_newline && a >= *lstart)) {
                    let x1 = shaped.x_for_index(a - lstart);
                    let mut x2 = shaped.x_for_index(b - lstart);
                    if spans_newline {
                        x2 += px(6.);
                    }
                    selections.push(fill(
                        Bounds::from_corners(
                            point(bounds.left() + x1, bounds.top() + lh * i as f32),
                            point(bounds.left() + x2, bounds.top() + lh * (i as f32 + 1.)),
                        ),
                        gpui::rgba(0xd9a34230),
                    ));
                }
            }
        }

        EditorPrepaint {
            lines: lines.into_iter().map(|(s, _)| s).collect(),
            cursor,
            selections,
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        prepaint: &mut EditorPrepaint,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus = self.editor.read(cx).focus_handle.clone();
        window.handle_input(
            &focus,
            ElementInputHandler::new(bounds, self.editor.clone()),
            cx,
        );
        let lh = window.line_height();
        for q in prepaint.selections.drain(..) {
            window.paint_quad(q);
        }
        for (i, line) in prepaint.lines.iter().enumerate() {
            let _ = line.paint(
                point(bounds.left(), bounds.top() + lh * i as f32),
                lh,
                gpui::TextAlign::Left,
                None,
                window,
                cx,
            );
        }
        if focus.is_focused(window) {
            if let Some(cursor) = prepaint.cursor.take() {
                window.paint_quad(cursor);
            }
        }
        // Cache layouts for mouse mapping.
        let lines = std::mem::take(&mut prepaint.lines);
        self.editor.update(cx, |ed, _| {
            ed.line_layouts = lines;
            ed.bounds = Some(bounds);
            ed.line_height = lh;
        });
    }
}
