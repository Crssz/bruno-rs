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
    Focusable, Font, GlobalElementId, Hsla, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, PaintQuad, Pixels, Point, ShapedLine, Style, TextRun, UTF16Selection, Window,
};
use unicode_segmentation::UnicodeSegmentation;

use crate::{highlight, theme};

/// What grammar (if any) to highlight the buffer with.
#[derive(Clone, Copy, PartialEq)]
pub enum Lang {
    Plain,
    Json,
    JavaScript,
}

/// Emitted on every content change, so a parent can react live (e.g. filtering).
pub struct Changed;
impl gpui::EventEmitter<Changed> for CodeEditor {}

/// Emitted when the `{{var}}` under the cursor changes: `Some(name)` on entering
/// a template var, `None` on a click (so the parent can dismiss its var popup).
/// The parent resolves the value/scope (the editor has no variable context).
pub struct HoverVar {
    pub name: Option<String>,
    pub pos: Point<Pixels>,
}
impl gpui::EventEmitter<HoverVar> for CodeEditor {}

/// Emitted on a right-click: the parent opens a Cut/Copy/Paste/Select All edit
/// menu anchored at `pos`, acting back on `editor`. `read_only`/`has_selection`
/// let the parent grey out the items that don't apply.
pub struct EditorMenu {
    pub editor: Entity<CodeEditor>,
    pub pos: Point<Pixels>,
    pub read_only: bool,
    pub has_selection: bool,
    /// True when the buffer's language has a formatter (JS/TS or JSON), so the
    /// parent can offer a "Format" item.
    pub formattable: bool,
    /// A navigable target under the click (a `{{var}}` or a `require(...)` path),
    /// so the menu can offer "Go to Definition"/"Go to Implementation".
    pub goto: Option<GotoTarget>,
}
impl gpui::EventEmitter<EditorMenu> for CodeEditor {}

/// What a Ctrl/Cmd+click (or the right-click menu) landed on, for the parent to
/// navigate to.
#[derive(Clone)]
pub enum GotoTarget {
    /// A `{{name}}` template variable — jump to where the scope defines it.
    Var(String),
    /// A `require('spec')` module — open the resolved file. `symbol` is set when
    /// navigating from an imported identifier, so the parent can also scroll to
    /// that symbol's definition inside the file.
    Module {
        spec: String,
        symbol: Option<String>,
    },
    /// A symbol defined in the *same* buffer — jump to its declaration in place.
    Local(String),
}

/// Emitted on Ctrl/Cmd+click over a `{{var}}` or a `require(...)` path. The
/// editor only recognizes the syntax; the parent resolves and navigates.
pub struct GotoDefinition {
    pub target: GotoTarget,
}
impl gpui::EventEmitter<GotoDefinition> for CodeEditor {}

actions!(
    code_editor,
    [
        Backspace,
        Delete,
        Left,
        Right,
        Up,
        Down,
        Home,
        End,
        Enter,
        Tab,
        SelectLeft,
        SelectRight,
        SelectUp,
        SelectDown,
        SelectAll,
        Paste,
        Cut,
        Copy,
        Undo,
        Redo,
        WordLeft,
        WordRight,
        SelectWordLeft,
        SelectWordRight,
        DeleteWordLeft,
        DeleteWordRight,
        SelectHome,
        SelectEnd,
        DocStart,
        DocEnd,
        SelectDocStart,
        SelectDocEnd,
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
        // Cut/Copy/Paste aliases (Zed's defaults).
        gpui::KeyBinding::new("shift-delete", Cut, Some("CodeEditor")),
        gpui::KeyBinding::new("ctrl-insert", Copy, Some("CodeEditor")),
        gpui::KeyBinding::new("shift-insert", Paste, Some("CodeEditor")),
        // Undo / redo.
        gpui::KeyBinding::new("ctrl-z", Undo, Some("CodeEditor")),
        gpui::KeyBinding::new("ctrl-y", Redo, Some("CodeEditor")),
        gpui::KeyBinding::new("ctrl-shift-z", Redo, Some("CodeEditor")),
        // Word-wise movement, selection and deletion.
        gpui::KeyBinding::new("ctrl-left", WordLeft, Some("CodeEditor")),
        gpui::KeyBinding::new("ctrl-right", WordRight, Some("CodeEditor")),
        gpui::KeyBinding::new("ctrl-shift-left", SelectWordLeft, Some("CodeEditor")),
        gpui::KeyBinding::new("ctrl-shift-right", SelectWordRight, Some("CodeEditor")),
        gpui::KeyBinding::new("ctrl-backspace", DeleteWordLeft, Some("CodeEditor")),
        gpui::KeyBinding::new("ctrl-delete", DeleteWordRight, Some("CodeEditor")),
        // Select to line start/end.
        gpui::KeyBinding::new("shift-home", SelectHome, Some("CodeEditor")),
        gpui::KeyBinding::new("shift-end", SelectEnd, Some("CodeEditor")),
        // Document start/end + selection.
        gpui::KeyBinding::new("ctrl-home", DocStart, Some("CodeEditor")),
        gpui::KeyBinding::new("ctrl-end", DocEnd, Some("CodeEditor")),
        gpui::KeyBinding::new("ctrl-shift-home", SelectDocStart, Some("CodeEditor")),
        gpui::KeyBinding::new("ctrl-shift-end", SelectDocEnd, Some("CodeEditor")),
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
    /// Render each glyph as a same-byte-length mask char (for secret values).
    masked: bool,
    /// Block edits (still selectable + copyable) — for the response viewer.
    read_only: bool,
    /// Cached tree-sitter highlight spans as `(byte range, capture index)`,
    /// recomputed on every content change. The capture index is resolved to a
    /// color at paint time so a theme switch recolors syntax without a re-parse.
    spans: Vec<(Range<usize>, usize)>,
    // Layout caches (filled during paint) for mouse mapping.
    line_layouts: Vec<ShapedLine>,
    bounds: Option<Bounds<Pixels>>,
    line_height: Pixels,
    /// Horizontal scroll offset applied during paint (single-line inputs).
    scroll_x: Pixels,
    /// The `{{var}}` name currently under the cursor (for hover-popup emission).
    hovered_var: Option<String>,
    /// Undo/redo as (content, selection) snapshots. Consecutive single-char
    /// typing coalesces into one entry via `coalesce_pos`.
    undo_stack: Vec<(String, Range<usize>)>,
    redo_stack: Vec<(String, Range<usize>)>,
    /// When the last edit was a single-char insert, the cursor position right
    /// after it — so the next contiguous keystroke extends the same undo entry.
    coalesce_pos: Option<usize>,
    /// Latest pointer position during an active drag-select (window coords). Lets
    /// the auto-scroll tick keep extending the selection while held past an edge.
    drag_pos: Option<Point<Pixels>>,
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
            masked: false,
            read_only: false,
            spans: Vec::new(),
            line_layouts: Vec::new(),
            bounds: None,
            line_height: px(19.),
            scroll_x: px(0.),
            hovered_var: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            coalesce_pos: None,
            drag_pos: None,
        };
        ed.recompute_highlight();
        ed
    }

    /// The `{{name}}` template variable spanning byte `offset`, if any (trimmed,
    /// non-empty). Used to drive the hover popup.
    fn var_at(&self, offset: usize) -> Option<String> {
        var_at_offset(&self.content, offset)
    }

    /// A single-line variant (for the URL field): one line tall, no newlines.
    pub fn single_line(cx: &mut Context<Self>, text: &str) -> Self {
        let mut ed = Self::new(cx, text);
        ed.single_line = true;
        ed
    }

    /// A single-line variant whose glyphs render masked (for secret values).
    pub fn masked_line(cx: &mut Context<Self>, text: &str) -> Self {
        let mut ed = Self::single_line(cx, text);
        ed.masked = true;
        ed
    }

    /// Toggle masked rendering (e.g. a reveal-secrets eye). Content is unchanged.
    pub fn set_masked(&mut self, masked: bool, cx: &mut Context<Self>) {
        if self.masked != masked {
            self.masked = masked;
            cx.notify();
        }
    }

    /// A read-only editor (selectable + copyable, no edits) — the response view.
    pub fn read_only(cx: &mut Context<Self>, text: &str) -> Self {
        let mut ed = Self::new(cx, text);
        ed.read_only = true;
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
        // Loading new content is not an undoable edit — drop the history so undo
        // can't revert into a previously-loaded block or file.
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.coalesce_pos = None;
        self.recompute_highlight();
        cx.notify();
    }

    /// Recompute cached highlight spans for the current content + language.
    fn recompute_highlight(&mut self) {
        self.spans = match self.lang {
            Lang::Json => highlight::json(&self.content),
            Lang::JavaScript => highlight::javascript(&self.content),
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
        self.content[..offset]
            .bytes()
            .filter(|b| *b == b'\n')
            .count()
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

    /// Byte offset of the start of the line containing `offset`.
    fn line_start(&self, offset: usize) -> usize {
        self.content[..offset]
            .rfind('\n')
            .map(|i| i + 1)
            .unwrap_or(0)
    }

    fn next_word(&self, offset: usize) -> usize {
        next_word_boundary(&self.content, offset)
    }

    fn prev_word(&self, offset: usize) -> usize {
        prev_word_boundary(&self.content, offset)
    }

    // ── movement ────────────────────────────────────────────────────────────
    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.selected_range = offset..offset;
        self.selection_reversed = false;
        cx.notify();
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.set_selection_head(offset);
        cx.notify();
    }

    /// Move the selection's moving end to `offset` without notifying — for use
    /// during paint (the auto-scroll tick), where the redraw is driven by an
    /// explicit animation frame instead.
    fn set_selection_head(&mut self, offset: usize) {
        if self.selection_reversed {
            self.selected_range.start = offset;
        } else {
            self.selected_range.end = offset;
        }
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
    }

    /// During a drag, if the pointer is held past the left/right edge of a single-
    /// line box, advance the selection toward that edge so the view keeps scrolling
    /// while the pointer is held still. Returns true if it moved — the caller then
    /// schedules another frame to continue. Speed scales with distance past the edge.
    fn auto_scroll_tick(&mut self) -> bool {
        if !self.single_line {
            return false;
        }
        let (Some(bounds), Some(pos)) = (self.bounds, self.drag_pos) else {
            return false;
        };
        let cur = self.cursor();
        let past_right = f32::from(pos.x - (bounds.left() + bounds.size.width));
        let past_left = f32::from(bounds.left() - pos.x);
        let new = if past_right > 0.0 {
            let steps = ((past_right / 8.0) as usize).clamp(1, 40);
            (0..steps).fold(cur, |o, _| self.next_grapheme(o))
        } else if past_left > 0.0 {
            let steps = ((past_left / 8.0) as usize).clamp(1, 40);
            (0..steps).fold(cur, |o, _| self.prev_grapheme(o))
        } else {
            return false;
        };
        if new == cur {
            return false;
        }
        self.set_selection_head(new);
        true
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
        let o = if self.single_line {
            0
        } else {
            self.vertical(-1)
        };
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
    fn word_left(&mut self, _: &WordLeft, _: &mut Window, cx: &mut Context<Self>) {
        let o = self.prev_word(self.cursor());
        self.move_to(o, cx);
    }
    fn word_right(&mut self, _: &WordRight, _: &mut Window, cx: &mut Context<Self>) {
        let o = self.next_word(self.cursor());
        self.move_to(o, cx);
    }
    fn select_word_left(&mut self, _: &SelectWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        let o = self.prev_word(self.cursor());
        self.select_to(o, cx);
    }
    fn select_word_right(&mut self, _: &SelectWordRight, _: &mut Window, cx: &mut Context<Self>) {
        let o = self.next_word(self.cursor());
        self.select_to(o, cx);
    }
    fn select_home(&mut self, _: &SelectHome, _: &mut Window, cx: &mut Context<Self>) {
        let o = self.line_start(self.cursor());
        self.select_to(o, cx);
    }
    fn select_end(&mut self, _: &SelectEnd, _: &mut Window, cx: &mut Context<Self>) {
        let o = self.line_end(self.line_start(self.cursor()));
        self.select_to(o, cx);
    }
    fn doc_start(&mut self, _: &DocStart, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
    }
    fn doc_end(&mut self, _: &DocEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.content.len(), cx);
    }
    fn select_doc_start(&mut self, _: &SelectDocStart, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(0, cx);
    }
    fn select_doc_end(&mut self, _: &SelectDocEnd, _: &mut Window, cx: &mut Context<Self>) {
        let end = self.content.len();
        self.select_to(end, cx);
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
        self.do_select_all(cx);
    }
    /// Select the whole buffer (shared by Ctrl-A and the right-click menu).
    pub fn do_select_all(&mut self, cx: &mut Context<Self>) {
        self.selected_range = 0..self.content.len();
        self.selection_reversed = false;
        cx.notify();
    }
    /// Select a byte range (clamped to the buffer) — used to highlight a symbol
    /// the user jumped to via "Go to Implementation".
    pub fn select_byte_range(&mut self, range: Range<usize>, cx: &mut Context<Self>) {
        let end = self.content.len();
        self.selected_range = range.start.min(end)..range.end.min(end);
        self.selection_reversed = false;
        cx.notify();
    }

    // ── editing ─────────────────────────────────────────────────────────────
    /// Snapshot the current (content, selection) onto the undo stack and drop the
    /// redo stack. Capped so a long session can't grow without bound.
    fn push_undo(&mut self) {
        self.undo_stack
            .push((self.content.clone(), self.selected_range.clone()));
        if self.undo_stack.len() > 256 {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
        self.coalesce_pos = None;
    }

    fn replace(&mut self, new_text: &str, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        let owned;
        let new_text = if self.single_line && new_text.contains('\n') {
            owned = new_text.replace('\n', " ");
            owned.as_str()
        } else {
            new_text
        };
        let r = self.selected_range.clone();
        // Coalesce a run of single-character typing into one undo entry; any other
        // edit (delete, paste, newline, edit after moving) starts a fresh entry.
        let single_insert = r.is_empty() && new_text != "\n" && new_text.chars().count() == 1;
        let contiguous = single_insert && self.coalesce_pos == Some(r.start);
        if !contiguous {
            self.push_undo();
        }
        self.content.replace_range(r.clone(), new_text); // r is a valid char-boundary range
        let at = r.start + new_text.len();
        self.selected_range = at..at;
        self.selection_reversed = false;
        self.coalesce_pos = single_insert.then_some(at);
        self.recompute_highlight();
        cx.emit(Changed);
        cx.notify();
    }

    fn undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        if let Some((content, range)) = self.undo_stack.pop() {
            self.redo_stack
                .push((self.content.clone(), self.selected_range.clone()));
            self.restore(content, range, cx);
        }
    }
    fn redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        if let Some((content, range)) = self.redo_stack.pop() {
            self.undo_stack
                .push((self.content.clone(), self.selected_range.clone()));
            self.restore(content, range, cx);
        }
    }
    /// Replace the buffer + selection from an undo/redo snapshot.
    fn restore(&mut self, content: String, range: Range<usize>, cx: &mut Context<Self>) {
        self.content = content;
        let end = self.content.len();
        self.selected_range = range.start.min(end)..range.end.min(end);
        self.selection_reversed = false;
        self.coalesce_pos = None;
        self.recompute_highlight();
        cx.emit(Changed);
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
    fn delete_word_left(&mut self, _: &DeleteWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let o = self.prev_word(self.cursor());
            if o == self.cursor() {
                return;
            }
            self.selected_range = o..self.cursor();
        }
        self.replace("", cx);
    }
    fn delete_word_right(&mut self, _: &DeleteWordRight, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let o = self.next_word(self.cursor());
            if o == self.cursor() {
                return;
            }
            self.selected_range = self.cursor()..o;
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
        self.do_copy(cx);
    }
    fn cut(&mut self, _: &Cut, _: &mut Window, cx: &mut Context<Self>) {
        self.do_cut(cx);
    }
    fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        self.do_paste(cx);
    }
    /// Copy the selection to the clipboard (shared by Ctrl-C and the menu).
    pub fn do_copy(&mut self, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
        }
    }
    /// Cut the selection to the clipboard (shared by Ctrl-X and the menu).
    pub fn do_cut(&mut self, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
            self.replace("", cx);
        }
    }
    /// Paste clipboard text over the selection (shared by Ctrl-V and the menu).
    pub fn do_paste(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|i| i.text()) {
            self.replace(&text, cx);
        }
    }
    /// Reformat the whole buffer for its language (Prettier-grade JS/TS via
    /// dprint, JSON via serde_json). On a syntax error the buffer is left as-is
    /// and the message is returned for the caller to surface.
    pub fn do_format(&mut self, cx: &mut Context<Self>) -> Result<(), String> {
        if self.read_only {
            return Err("Editor is read-only".into());
        }
        let formatted = format_source(&self.content, self.lang)?;
        if formatted != self.content {
            self.push_undo(); // make Format a single undoable step
            let at = self.selected_range.start.min(formatted.len());
            self.content = formatted;
            self.selected_range = at..at;
            self.selection_reversed = false;
            self.recompute_highlight();
            cx.emit(Changed);
            cx.notify();
        }
        Ok(())
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
        let col =
            self.line_layouts[line].closest_index_for_x(position.x - bounds.left() + self.scroll_x);
        let lstart = starts[line];
        (lstart + col).min(self.line_end(lstart))
    }

    fn on_mouse_down(&mut self, e: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus_handle, cx);
        let idx = self.index_for_position(e.position);
        // Ctrl (Cmd on macOS) + click: navigate to a `{{var}}`, a `require(...)`
        // path, or an imported symbol. Falls through to a normal click if none hit.
        if e.modifiers.secondary() {
            if let Some(target) = self.goto_target_at(idx) {
                self.hovered_var = None;
                cx.emit(GotoDefinition { target });
                return;
            }
        }
        match e.click_count {
            // Double-click selects the word under the cursor.
            2 => {
                self.is_selecting = false;
                self.selected_range = self.word_range_at(idx);
                self.selection_reversed = false;
                cx.notify();
            }
            // Triple (or more) clicks select the whole line.
            n if n >= 3 => {
                self.is_selecting = false;
                let ls = self.line_starts()[self.line_of(idx)];
                self.selected_range = ls..self.line_end(ls);
                self.selection_reversed = false;
                cx.notify();
            }
            // Single click: place the caret, or extend with Shift.
            _ => {
                self.is_selecting = true;
                if e.modifiers.shift {
                    self.select_to(idx, cx);
                } else {
                    self.move_to(idx, cx);
                }
            }
        }
        // A click dismisses any open var popup.
        self.hovered_var = None;
        cx.emit(HoverVar {
            name: None,
            pos: e.position,
        });
    }

    /// The word (run of alphanumerics/`_`) surrounding `offset`, for double-click.
    fn word_range_at(&self, offset: usize) -> Range<usize> {
        word_range_at_offset(&self.content, offset)
    }

    /// The `require('...')` specifier whose string `offset` falls within, if any.
    fn require_spec_at(&self, offset: usize) -> Option<String> {
        require_spec_at_offset(&self.content, offset)
    }

    /// A navigable target at byte `offset`: a `{{var}}`, a `require('...')` path,
    /// or an identifier imported via `require` (→ that module).
    fn goto_target_at(&self, offset: usize) -> Option<GotoTarget> {
        if let Some(name) = self.var_at(offset) {
            return Some(GotoTarget::Var(name));
        }
        if let Some(spec) = self.require_spec_at(offset) {
            return Some(GotoTarget::Module { spec, symbol: None });
        }
        // Symbol navigation only makes sense for JavaScript buffers.
        if self.lang != Lang::JavaScript {
            return None;
        }
        let word = self.word_at(offset)?;
        // An imported identifier opens its module; otherwise a symbol defined in
        // this same buffer jumps to its local declaration.
        if let Some(spec) = import_spec_for_symbol(&self.content, &word) {
            return Some(GotoTarget::Module {
                spec,
                symbol: Some(word),
            });
        }
        highlight::js_symbol_range(&self.content, &word)
            .is_some()
            .then_some(GotoTarget::Local(word))
    }

    /// The identifier (JS-ish word) under `offset`, or `None` if not on one.
    fn word_at(&self, offset: usize) -> Option<String> {
        let w = &self.content[word_range_at_offset(&self.content, offset)];
        let first = w.chars().next()?;
        (first.is_alphabetic() || first == '_' || first == '$').then(|| w.to_string())
    }
    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
        self.drag_pos = None;
    }
    /// Right-click: focus, place the caret at the click *unless* it lands inside
    /// the current selection (so Cut/Copy keep acting on it), then ask the parent
    /// to open the edit menu. Mirrors Zed's `mouse_right_down`.
    fn on_right_mouse_down(
        &mut self,
        e: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus_handle, cx);
        let idx = self.index_for_position(e.position);
        if !self.selected_range.contains(&idx) {
            self.move_to(idx, cx);
        }
        self.hovered_var = None;
        cx.emit(HoverVar {
            name: None,
            pos: e.position,
        });
        // What the click landed on, so the menu can offer a "Go to" item.
        let goto = self.goto_target_at(idx);
        cx.emit(EditorMenu {
            editor: cx.entity(),
            pos: e.position,
            read_only: self.read_only,
            has_selection: !self.selected_range.is_empty(),
            formattable: matches!(self.lang, Lang::JavaScript | Lang::Json),
            goto,
        });
    }
    fn on_mouse_move(&mut self, e: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        let idx = self.index_for_position(e.position);
        if self.is_selecting {
            self.select_to(idx, cx);
            return; // no var-hover work mid drag-select
        }
        // Emit whenever the hovered `{{var}}` changes — `Some` on entering one,
        // `None` on leaving it. The parent opens the popup on `Some` and dismisses
        // it on `None` (after a short grace, so the pointer can travel into the
        // popup to reach Copy).
        let cur = self.var_at(idx);
        if cur != self.hovered_var {
            self.hovered_var = cur.clone();
            cx.emit(HoverVar {
                name: cur,
                pos: e.position,
            });
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
            // Fill the container width, and at least its height (so short buffers
            // fill the box and clicks below the text still land), but grow taller
            // with content so a parent `overflow_y_scroll` can actually scroll —
            // `size_full` would have pinned us to the container height.
            .w_full()
            .min_h(relative(1.))
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
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::redo))
            .on_action(cx.listener(Self::word_left))
            .on_action(cx.listener(Self::word_right))
            .on_action(cx.listener(Self::select_word_left))
            .on_action(cx.listener(Self::select_word_right))
            .on_action(cx.listener(Self::delete_word_left))
            .on_action(cx.listener(Self::delete_word_right))
            .on_action(cx.listener(Self::select_home))
            .on_action(cx.listener(Self::select_end))
            .on_action(cx.listener(Self::doc_start))
            .on_action(cx.listener(Self::doc_end))
            .on_action(cx.listener(Self::select_doc_start))
            .on_action(cx.listener(Self::select_doc_end))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_down(MouseButton::Right, cx.listener(Self::on_right_mouse_down))
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
/// the default color. `spans` are `(byte range, capture index)`; the color is
/// resolved per-paint via `highlight::color` (so a theme switch is live) and a
/// capture with no dedicated color also falls back to `default`. `spans` must be
/// sorted by start (tree-sitter emits them in document order) and non-overlapping.
fn build_line_runs(
    line: &str,
    lstart: usize,
    spans: &[(Range<usize>, usize)],
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
    for (range, kind) in spans {
        if range.end <= lstart || range.start >= lend {
            continue;
        }
        let s = range.start.max(lstart);
        let e = range.end.min(lend);
        if s > pos {
            runs.push(mk(s - pos, default));
        }
        if e > s {
            runs.push(mk(e - s, highlight::color(*kind).unwrap_or(default)));
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

/// Replace each character with a mask glyph of the **same UTF-8 byte length**,
/// so every byte-offset computation (cursor, selection, hit-testing) stays valid
/// against the real `content` while the display hides it. Newlines pass through
/// to keep line boundaries intact.
fn mask_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        out.push(match ch {
            '\n' => '\n',
            _ => match ch.len_utf8() {
                1 => '*',
                2 => '\u{00B7}',  // · middle dot (2 bytes)
                3 => '\u{2022}',  // • bullet (3 bytes)
                _ => '\u{10000}', // 𐀀 (4 bytes) — fallback; secrets are ~never 4-byte
            },
        });
    }
    out
}

/// The custom element that shapes lines and paints cursor/selection.
struct EditorElement {
    editor: Entity<CodeEditor>,
}

struct EditorPrepaint {
    lines: Vec<ShapedLine>,
    cursor: Option<PaintQuad>,
    selections: Vec<PaintQuad>,
    /// Horizontal scroll offset (single-line inputs) so the cursor stays in view.
    scroll_x: Pixels,
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
            // Mask preserves each char's byte length, so `runs` (built from the
            // real line) still line up with the shaped display string.
            let display = if editor.masked {
                mask_str(line)
            } else {
                line.to_string()
            };
            let shaped = window.text_system().shape_line(
                gpui::SharedString::from(display),
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
        // Single-line inputs scroll horizontally so the cursor stays inside the
        // box. Scroll *into view* (adjust only when the cursor leaves the visible
        // range, by the minimum amount) starting from the previous offset — rather
        // than pinning the cursor to the right edge, which made drag-selection jump
        // because the text scrolled out from under the pointer on every move.
        let scroll_x = if editor.single_line {
            let margin = px(6.);
            let view_w = (bounds.size.width - margin).max(px(0.));
            let line_w = cshaped.x_for_index(cshaped.text.len());
            let mut sx = editor.scroll_x;
            if cx_px < sx {
                sx = cx_px; // cursor past the left edge → reveal it
            } else if cx_px - sx > view_w {
                sx = cx_px - view_w; // cursor past the right edge → reveal it
            }
            let max_sx = (line_w - view_w).max(px(0.));
            sx.clamp(px(0.), max_sx)
        } else {
            px(0.)
        };
        let cursor = Some(fill(
            Bounds::new(
                point(
                    bounds.left() + cx_px - scroll_x,
                    bounds.top() + lh * cline as f32,
                ),
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
                            point(bounds.left() + x1 - scroll_x, bounds.top() + lh * i as f32),
                            point(
                                bounds.left() + x2 - scroll_x,
                                bounds.top() + lh * (i as f32 + 1.),
                            ),
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
            scroll_x,
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
        // Clip all painting to the editor's own bounds so long text (e.g. a long
        // env value or URL) and its selection never spill outside the input box.
        window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
            for q in prepaint.selections.drain(..) {
                window.paint_quad(q);
            }
            for (i, line) in prepaint.lines.iter().enumerate() {
                let _ = line.paint(
                    point(
                        bounds.left() - prepaint.scroll_x,
                        bounds.top() + lh * i as f32,
                    ),
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
        });
        // Cache layouts for mouse mapping.
        let lines = std::mem::take(&mut prepaint.lines);
        let scroll_x = prepaint.scroll_x;
        self.editor.update(cx, |ed, _| {
            ed.line_layouts = lines;
            ed.bounds = Some(bounds);
            ed.line_height = lh;
            ed.scroll_x = scroll_x;
        });
        // While a drag-select is active, keep extending it even when the pointer
        // leaves the box. The element-level `on_mouse_move` only fires while the
        // editor is hovered, so a drag past the edge would otherwise stall; these
        // window-level handlers (re-registered each frame for the drag's duration)
        // catch moves and the mouse-up anywhere on screen.
        if self.editor.read(cx).is_selecting {
            let editor = self.editor.clone();
            window.on_mouse_event(move |e: &MouseMoveEvent, phase, _window, cx: &mut App| {
                if phase == gpui::DispatchPhase::Bubble
                    && e.pressed_button == Some(MouseButton::Left)
                {
                    editor.update(cx, |ed, cx| {
                        ed.drag_pos = Some(e.position);
                        let idx = ed.index_for_position(e.position);
                        ed.select_to(idx, cx);
                    });
                }
            });
            let editor = self.editor.clone();
            window.on_mouse_event(move |_e: &MouseUpEvent, phase, _window, cx: &mut App| {
                if phase == gpui::DispatchPhase::Bubble {
                    editor.update(cx, |ed, _cx| {
                        ed.is_selecting = false;
                        ed.drag_pos = None;
                    });
                }
            });
            // Keep scrolling while the pointer is held past an edge (no move events
            // fire then) by ticking once per frame until it stops making progress.
            if self.editor.update(cx, |ed, _cx| ed.auto_scroll_tick()) {
                window.request_animation_frame();
            }
        }
    }
}

/// Word boundary to the right of `offset`: skip whitespace, then consume one run
/// of same-class chars (word vs. punctuation). Lands at the next word end.
fn next_word_boundary(content: &str, offset: usize) -> usize {
    let word = |c: char| c.is_alphanumeric() || c == '_';
    let mut class: Option<bool> = None;
    for (i, c) in content[offset..].char_indices() {
        if c.is_whitespace() {
            if class.is_some() {
                return offset + i;
            }
        } else {
            match class {
                None => class = Some(word(c)),
                Some(prev) if prev != word(c) => return offset + i,
                _ => {}
            }
        }
    }
    content.len()
}

/// Word boundary to the left of `offset` (symmetric to [`next_word_boundary`]).
fn prev_word_boundary(content: &str, offset: usize) -> usize {
    let word = |c: char| c.is_alphanumeric() || c == '_';
    let mut class: Option<bool> = None;
    let mut start = offset;
    for (i, c) in content[..offset].char_indices().rev() {
        if c.is_whitespace() {
            if class.is_some() {
                return start;
            }
        } else {
            match class {
                None => class = Some(word(c)),
                Some(prev) if prev != word(c) => return start,
                _ => {}
            }
            start = i;
        }
    }
    0
}

/// Find the `require(...)` module that imports `symbol`, scanning each line for a
/// `const … = require('spec')` whose left-hand binding mentions `symbol` (handles
/// both `const x = require(...)` and `const { a, x } = require(...)`). Lets a
/// click on an imported identifier jump to the file that defines it.
fn import_spec_for_symbol(content: &str, symbol: &str) -> Option<String> {
    for line in content.lines() {
        let Some(eq) = line.find('=') else { continue };
        let (lhs, rhs) = line.split_at(eq);
        let Some(spec) = require_spec_in_line(rhs) else {
            continue;
        };
        if binds_identifier(lhs, symbol) {
            return Some(spec);
        }
    }
    None
}

/// The string argument of the first `require(...)` in `s`, if any.
fn require_spec_in_line(s: &str) -> Option<String> {
    let after = s[s.find("require(")? + "require(".len()..].trim_start();
    let q = after.chars().next()?;
    if q != '\'' && q != '"' && q != '`' {
        return None;
    }
    let rest = &after[q.len_utf8()..];
    let end = rest.find(q)?;
    Some(rest[..end].to_string())
}

/// True if `symbol` appears in `lhs` as a whole identifier (word boundaries).
fn binds_identifier(lhs: &str, symbol: &str) -> bool {
    let id = |c: char| c.is_alphanumeric() || c == '_' || c == '$';
    lhs.match_indices(symbol).any(|(i, _)| {
        let before = lhs[..i].chars().next_back();
        let after = lhs[i + symbol.len()..].chars().next();
        !before.is_some_and(id) && !after.is_some_and(id)
    })
}

/// Format `content` for `lang`: JS/TS via dprint (Prettier-grade), JSON via
/// serde_json. Returns the formatted text, or an error message (e.g. a syntax
/// error) so the buffer can be left untouched. Plain text has no formatter.
fn format_source(content: &str, lang: Lang) -> Result<String, String> {
    match lang {
        Lang::JavaScript => format_javascript(content),
        Lang::Json => {
            let v: serde_json::Value =
                serde_json::from_str(content).map_err(|e| format!("Invalid JSON: {e}"))?;
            serde_json::to_string_pretty(&v).map_err(|e| format!("Format error: {e}"))
        }
        Lang::Plain => Err("No formatter for plain text".into()),
    }
}

/// Format JavaScript/TypeScript with dprint's Prettier-style defaults.
fn format_javascript(content: &str) -> Result<String, String> {
    use dprint_plugin_typescript::configuration::ConfigurationBuilder;
    use dprint_plugin_typescript::{format_text, FormatTextOptions};
    let config = ConfigurationBuilder::new().build();
    let options = FormatTextOptions {
        path: std::path::Path::new("script.js"),
        extension: None,
        text: content.to_string(),
        config: &config,
        external_formatter: None,
    };
    match format_text(options) {
        Ok(Some(formatted)) => Ok(formatted),
        Ok(None) => Ok(content.to_string()), // already formatted
        Err(e) => Err(format!("Format error: {e}")),
    }
}

/// The `{{name}}` template variable spanning byte `offset` in `content` (trimmed,
/// non-empty), or `None`. The match includes the surrounding `{{`/`}}`.
fn var_at_offset(content: &str, offset: usize) -> Option<String> {
    let mut i = 0;
    while let Some(open) = content[i..].find("{{") {
        let start = i + open;
        let Some(close_rel) = content[start + 2..].find("}}") else {
            break;
        };
        let close = start + 2 + close_rel;
        if offset >= start && offset < close + 2 {
            let inner = content[start + 2..close].trim();
            return (!inner.is_empty()).then(|| inner.to_string());
        }
        i = close + 2;
    }
    None
}

/// The word (run of alphanumerics/`_`) surrounding `offset`. When `offset` isn't
/// on a word char, falls back to the single character under it (so a double-click
/// on punctuation still selects something visible). Empty range at end-of-text.
fn word_range_at_offset(content: &str, offset: usize) -> Range<usize> {
    let offset = offset.min(content.len());
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let mut start = offset;
    for (i, c) in content[..offset].char_indices().rev() {
        if is_word(c) {
            start = i;
        } else {
            break;
        }
    }
    let mut end = offset;
    for (i, c) in content[offset..].char_indices() {
        if is_word(c) {
            end = offset + i + c.len_utf8();
        } else {
            break;
        }
    }
    if start == end {
        if let Some(c) = content[offset..].chars().next() {
            return offset..offset + c.len_utf8();
        }
    }
    start..end
}

/// If `offset` falls inside the quoted argument of a `require('...')` call on its
/// line, return the specifier (the inner string). Single-line only — that covers
/// every realistic `require` in a script.
fn require_spec_at_offset(content: &str, offset: usize) -> Option<String> {
    let offset = offset.min(content.len());
    let line_start = content[..offset].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let line_end = content[offset..]
        .find('\n')
        .map(|i| offset + i)
        .unwrap_or(content.len());
    let line = &content[line_start..line_end];
    let rel = offset - line_start;
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let q = bytes[i];
        if q == b'\'' || q == b'"' || q == b'`' {
            let Some(close_off) = line[i + 1..].find(q as char) else {
                break; // unterminated string
            };
            let close = i + 1 + close_off;
            if rel >= i && rel <= close && line[..i].trim_end().ends_with("require(") {
                return Some(line[i + 1..close].to_string());
            }
            i = close + 1;
        } else {
            i += 1;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{
        format_source, import_spec_for_symbol, next_word_boundary, prev_word_boundary,
        require_spec_at_offset, var_at_offset, word_range_at_offset, Lang,
    };

    #[test]
    fn detects_var_under_offset() {
        let s = "GET {{baseUrl}}/users/{{id}}";
        // Inside {{baseUrl}} (covers the braces too).
        assert_eq!(var_at_offset(s, 4).as_deref(), Some("baseUrl")); // first '{'
        assert_eq!(var_at_offset(s, 9).as_deref(), Some("baseUrl")); // mid-name
        assert!(var_at_offset(s, 15).is_none()); // the '/' after }}
                                                 // Second var.
        assert_eq!(var_at_offset(s, 24).as_deref(), Some("id"));
    }

    #[test]
    fn trims_inner_and_ignores_empty() {
        assert_eq!(
            var_at_offset("a {{ spaced }} b", 5).as_deref(),
            Some("spaced")
        );
        assert_eq!(var_at_offset("x {{}} y", 3), None); // empty braces
        assert_eq!(var_at_offset("plain text", 3), None); // no braces
    }

    #[test]
    fn unclosed_braces_are_safe() {
        assert_eq!(var_at_offset("{{oops", 2), None);
    }

    #[test]
    fn word_range_selects_whole_word() {
        let s = "const fooBar_1 = 2;";
        // Click mid-word selects the full identifier (incl. digits/underscore).
        assert_eq!(word_range_at_offset(s, 9), 6..14); // inside "fooBar_1"
        assert_eq!(&s[word_range_at_offset(s, 9)], "fooBar_1");
        // Click at a word's start boundary still selects it.
        assert_eq!(&s[word_range_at_offset(s, 0)], "const");
    }

    #[test]
    fn word_range_on_punctuation_selects_one_char() {
        let s = "a = b";
        assert_eq!(&s[word_range_at_offset(s, 2)], "="); // on '='
                                                         // Past the end clamps (no panic); here it catches the trailing word.
        assert_eq!(&s[word_range_at_offset(s, 99)], "b");
        // End-of-text after a non-word char yields an empty range.
        assert!(word_range_at_offset("a ", 2).is_empty());
    }

    #[test]
    fn require_spec_detected_inside_quotes_only() {
        let s = "const h = require('./helper.js');";
        let inside = s.find("./").unwrap() + 1; // within the path string
        assert_eq!(
            require_spec_at_offset(s, inside).as_deref(),
            Some("./helper.js")
        );
        // The bare `require` token (outside the quotes) is not a hit.
        assert_eq!(require_spec_at_offset(s, s.find("require").unwrap()), None);
    }

    #[test]
    fn word_boundaries_move_by_word() {
        let s = "foo.bar baz";
        // Forward from start: end of "foo" (the '.'), then end of "bar" (space)…
        assert_eq!(next_word_boundary(s, 0), 3); // after "foo"
        assert_eq!(next_word_boundary(s, 3), 4); // over the "." run
        assert_eq!(next_word_boundary(s, 4), 7); // after "bar"
                                                 // Backward from end: start of "baz", then start of "bar"…
        assert_eq!(prev_word_boundary(s, s.len()), 8); // start of "baz"
        assert_eq!(prev_word_boundary(s, 7), 4); // start of "bar"
        assert_eq!(prev_word_boundary(s, 3), 0); // start of "foo"
                                                 // Clamps at the ends, no panic.
        assert_eq!(next_word_boundary(s, s.len()), s.len());
        assert_eq!(prev_word_boundary(s, 0), 0);
    }

    #[test]
    fn import_symbol_resolves_to_its_require_module() {
        let src = "const { useOAPISetVar } = require('./hook')\nawait useOAPISetVar();";
        // Clicking the destructured symbol anywhere resolves to its module.
        assert_eq!(
            import_spec_for_symbol(src, "useOAPISetVar").as_deref(),
            Some("./hook")
        );
        // Default-import form: `const x = require('...')`.
        assert_eq!(
            import_spec_for_symbol("const hook = require('./hook.js')", "hook").as_deref(),
            Some("./hook.js")
        );
        // A non-imported identifier, and a mere substring of one, must not match.
        assert_eq!(import_spec_for_symbol(src, "fetch"), None);
        assert_eq!(import_spec_for_symbol(src, "useOAPI"), None);
    }

    #[test]
    fn format_javascript_reindents_and_spaces() {
        let src = "function f(){const x=1;if(x){console.log(x)}}";
        let out = format_source(src, Lang::JavaScript).unwrap();
        assert_ne!(out, src);
        assert!(out.contains("function f() {"), "dprint output: {out}");
        assert!(out.contains("\n"), "should be multi-line");
    }

    #[test]
    fn format_json_pretty_prints() {
        let out = format_source("{\"a\":1,\"b\":[1,2]}", Lang::Json).unwrap();
        assert!(out.contains("  \"a\": 1"), "json output: {out}");
    }

    #[test]
    fn format_rejects_invalid_js_and_plain() {
        assert!(format_source("function (", Lang::JavaScript).is_err());
        assert!(format_source("anything", Lang::Plain).is_err());
    }

    #[test]
    fn require_spec_handles_quotes_and_misses() {
        assert_eq!(
            require_spec_at_offset("x = require(\"../a\")", 14).as_deref(),
            Some("../a")
        );
        // A plain string that isn't a require argument is ignored.
        assert_eq!(require_spec_at_offset("let s = './nope.js';", 12), None);
        // Multi-line: resolution is scoped to the clicked line.
        let s = "line1\nrequire('./x')\nline3";
        let on_x = s.find("./x").unwrap() + 1;
        assert_eq!(require_spec_at_offset(s, on_x).as_deref(), Some("./x"));
    }
}
