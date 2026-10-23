//! 股名／代號搜尋框：從 gpui-pre 0.3.7 `examples/input.rs` 精簡而來（取自 web-wasm 分支 src/search.rs；手機版放大字級、滿版寬）。
//!
//! - GPUI 沒有內建文字輸入元件；這裡用 `EntityInputHandler` 自己接平台輸入。X11 上 GPUI 走 XIM（PREEDIT_CALLBACKS），
//!   組字中的字（marked text）會畫在框內加底線；`bounds_for_range` 回傳游標位置，GPUI 用它設定候選窗位置（SpotLocation）。
//! - 只有內容改變才 emit `SearchEvent::Changed`；**組字中（有 marked text）不觸發搜尋**，選字確定後才搜。
//! - 和報價表／走勢圖是不同的 entity，打字只重畫這個框，不碰報價套用。

use std::ops::Range;

use gpui::{
    actions, div, fill, point, prelude::*, px, relative, size, App, Bounds, Context, CursorStyle,
    ElementId, ElementInputHandler, Entity, EntityInputHandler, EventEmitter, FocusHandle, Focusable,
    GlobalElementId, KeyBinding, LayoutId, MouseButton, MouseDownEvent, PaintQuad, Pixels, Point, ShapedLine,
    SharedString, Style, TextRun, UTF16Selection, UnderlineStyle, Window,
};

use crate::theme::*;

actions!(search_input, [Backspace, Delete, Left, Right, Home, End, SelectAll, Paste, Submit, Cancel, Down, Up]);

pub const KEY_CONTEXT: &str = "SearchInput";

pub fn bind_keys(cx: &mut App) {
    let c = Some(KEY_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, c),
        KeyBinding::new("delete", Delete, c),
        KeyBinding::new("left", Left, c),
        KeyBinding::new("right", Right, c),
        KeyBinding::new("home", Home, c),
        KeyBinding::new("end", End, c),
        KeyBinding::new("ctrl-a", SelectAll, c),
        KeyBinding::new("ctrl-v", Paste, c),
        KeyBinding::new("enter", Submit, c),
        KeyBinding::new("escape", Cancel, c),
        KeyBinding::new("down", Down, c),
        KeyBinding::new("up", Up, c),
    ]);
}

pub enum SearchEvent {
    /// 內容確定改變（不含組字中的暫存字）
    Changed(String),
    Submit,
    Cancel,
    Move(i32),
}

pub struct SearchInput {
    pub focus_handle: FocusHandle,
    pub content: SharedString,
    placeholder: SharedString,
    selected_range: Range<usize>,
    marked_range: Option<Range<usize>>,
    last_layout: Option<ShapedLine>,
    last_bounds: Option<Bounds<Pixels>>,
    last_emitted: String,
}

impl EventEmitter<SearchEvent> for SearchInput {}

impl SearchInput {
    pub fn new(cx: &mut Context<Self>) -> Self {
        SearchInput {
            focus_handle: cx.focus_handle(),
            content: "".into(),
            placeholder: "搜尋代號／名稱（例：2330、台積）".into(),
            selected_range: 0..0,
            marked_range: None,
            last_layout: None,
            last_bounds: None,
            last_emitted: String::new(),
        }
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.content = "".into();
        self.selected_range = 0..0;
        self.marked_range = None;
        self.changed(cx);
    }

    pub fn is_composing(&self) -> bool {
        self.marked_range.is_some()
    }

    fn changed(&mut self, cx: &mut Context<Self>) {
        if self.marked_range.is_none() && self.content.as_ref() != self.last_emitted {
            self.last_emitted = self.content.to_string();
            cx.emit(SearchEvent::Changed(self.last_emitted.clone()));
        }
        cx.notify();
    }

    fn prev_boundary(&self, off: usize) -> usize {
        self.content[..off].char_indices().next_back().map(|(i, _)| i).unwrap_or(0)
    }
    fn next_boundary(&self, off: usize) -> usize {
        self.content[off..].chars().next().map(|c| off + c.len_utf8()).unwrap_or(self.content.len())
    }
    fn move_to(&mut self, off: usize, cx: &mut Context<Self>) {
        self.selected_range = off..off;
        cx.notify();
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        let o = if self.selected_range.is_empty() { self.prev_boundary(self.selected_range.start) } else { self.selected_range.start };
        self.move_to(o, cx);
    }
    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        let o = if self.selected_range.is_empty() { self.next_boundary(self.selected_range.end) } else { self.selected_range.end };
        self.move_to(o, cx);
    }
    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
    }
    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.content.len(), cx);
    }
    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.selected_range = 0..self.content.len();
        cx.notify();
    }
    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let p = self.prev_boundary(self.selected_range.start);
            self.selected_range = p..self.selected_range.start;
        }
        self.replace_text_in_range(None, "", window, cx);
    }
    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let n = self.next_boundary(self.selected_range.end);
            self.selected_range = self.selected_range.start..n;
        }
        self.replace_text_in_range(None, "", window, cx);
    }
    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(t) = cx.read_from_clipboard().and_then(|i| i.text()) {
            self.replace_text_in_range(None, &t.replace('\n', " "), window, cx);
        }
    }
    fn submit(&mut self, _: &Submit, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(SearchEvent::Submit);
    }
    fn cancel(&mut self, _: &Cancel, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(SearchEvent::Cancel);
    }
    fn down(&mut self, _: &Down, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(SearchEvent::Move(1));
    }
    fn up(&mut self, _: &Up, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(SearchEvent::Move(-1));
    }
    fn on_mouse_down(&mut self, ev: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus_handle, cx);
        let ix = match (&self.last_bounds, &self.last_layout) {
            (Some(b), Some(l)) if !self.content.is_empty() => l.closest_index_for_x(ev.position.x - b.left()),
            _ => 0,
        };
        self.move_to(ix.min(self.content.len()), cx);
    }

    fn off_from16(&self, o: usize) -> usize {
        let (mut u8o, mut u16o) = (0, 0);
        for ch in self.content.chars() {
            if u16o >= o {
                break;
            }
            u16o += ch.len_utf16();
            u8o += ch.len_utf8();
        }
        u8o
    }
    fn off_to16(&self, o: usize) -> usize {
        let (mut u16o, mut u8o) = (0, 0);
        for ch in self.content.chars() {
            if u8o >= o {
                break;
            }
            u8o += ch.len_utf8();
            u16o += ch.len_utf16();
        }
        u16o
    }
    fn r_to16(&self, r: &Range<usize>) -> Range<usize> {
        self.off_to16(r.start)..self.off_to16(r.end)
    }
    fn r_from16(&self, r: &Range<usize>) -> Range<usize> {
        self.off_from16(r.start)..self.off_from16(r.end)
    }
}

impl EntityInputHandler for SearchInput {
    fn text_for_range(&mut self, r16: Range<usize>, actual: &mut Option<Range<usize>>, _: &mut Window, _: &mut Context<Self>) -> Option<String> {
        let r = self.r_from16(&r16);
        actual.replace(self.r_to16(&r));
        Some(self.content[r].to_string())
    }
    fn selected_text_range(&mut self, _: bool, _: &mut Window, _: &mut Context<Self>) -> Option<UTF16Selection> {
        Some(UTF16Selection { range: self.r_to16(&self.selected_range), reversed: false })
    }
    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked_range.as_ref().map(|r| self.r_to16(r))
    }
    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.marked_range = None;
        self.changed(cx);
    }
    fn replace_text_in_range(&mut self, r16: Option<Range<usize>>, text: &str, _: &mut Window, cx: &mut Context<Self>) {
        let r = r16.as_ref().map(|r| self.r_from16(r)).or(self.marked_range.clone()).unwrap_or(self.selected_range.clone());
        self.content = (self.content[..r.start].to_owned() + text + &self.content[r.end..]).into();
        self.selected_range = r.start + text.len()..r.start + text.len();
        self.marked_range = None;
        self.changed(cx);
    }
    fn replace_and_mark_text_in_range(
        &mut self,
        r16: Option<Range<usize>>,
        text: &str,
        sel16: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let r = r16.as_ref().map(|r| self.r_from16(r)).or(self.marked_range.clone()).unwrap_or(self.selected_range.clone());
        self.content = (self.content[..r.start].to_owned() + text + &self.content[r.end..]).into();
        self.marked_range = if text.is_empty() { None } else { Some(r.start..r.start + text.len()) };
        // sel16 是相對於新組字字串的 UTF-16 位置
        self.selected_range = match sel16 {
            Some(s) => {
                let sub: String = text.to_string();
                let to8 = |o: usize| {
                    let (mut a, mut b) = (0, 0);
                    for ch in sub.chars() {
                        if b >= o {
                            break;
                        }
                        b += ch.len_utf16();
                        a += ch.len_utf8();
                    }
                    a
                };
                r.start + to8(s.start)..r.start + to8(s.end)
            }
            None => r.start + text.len()..r.start + text.len(),
        };
        self.changed(cx);
    }
    fn bounds_for_range(&mut self, r16: Range<usize>, b: Bounds<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<Bounds<Pixels>> {
        let l = self.last_layout.as_ref()?;
        let r = self.r_from16(&r16);
        Some(Bounds::from_corners(point(b.left() + l.x_for_index(r.start), b.top()), point(b.left() + l.x_for_index(r.end), b.bottom())))
    }
    fn character_index_for_point(&mut self, p: Point<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        let b = self.last_bounds?;
        let l = self.last_layout.as_ref()?;
        if self.content.is_empty() {
            return Some(0);
        }
        Some(self.off_to16(l.closest_index_for_x(p.x - b.left())))
    }
}

struct TextElement {
    input: Entity<SearchInput>,
}

struct Prepaint {
    line: Option<ShapedLine>,
    cursor: Option<PaintQuad>,
    selection: Option<PaintQuad>,
}

impl IntoElement for TextElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = Prepaint;
    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }
    fn request_layout(&mut self, _: Option<&GlobalElementId>, _: Option<&gpui::InspectorElementId>, window: &mut Window, cx: &mut App) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = window.line_height().into();
        (window.request_layout(style, [], cx), ())
    }
    fn prepaint(&mut self, _: Option<&GlobalElementId>, _: Option<&gpui::InspectorElementId>, bounds: Bounds<Pixels>, _: &mut (), window: &mut Window, cx: &mut App) -> Prepaint {
        let input = self.input.read(cx);
        let content = input.content.clone();
        let sel = input.selected_range.clone();
        let style = window.text_style();
        let (shown, color) = if content.is_empty() { (input.placeholder.clone(), dim().into()) } else { (content, style.color) };
        let run = TextRun { len: shown.len(), font: style.font(), color, background_color: None, underline: None, strikethrough: None };
        let runs = match input.marked_range.clone().filter(|_| !input.content.is_empty()) {
            Some(m) => vec![
                TextRun { len: m.start, ..run.clone() },
                TextRun {
                    len: m.end - m.start,
                    underline: Some(UnderlineStyle { color: Some(run.color), thickness: px(1.0), wavy: false }),
                    ..run.clone()
                },
                TextRun { len: shown.len() - m.end, ..run },
            ]
            .into_iter()
            .filter(|r| r.len > 0)
            .collect(),
            None => vec![run],
        };
        let fs = style.font_size.to_pixels(window.rem_size());
        let line = window.text_system().shape_line(shown, fs, &runs, None);
        let (selection, cursor) = if sel.is_empty() {
            let x = if input.content.is_empty() { px(0.) } else { line.x_for_index(sel.start) };
            (None, Some(fill(Bounds::new(point(bounds.left() + x, bounds.top()), size(px(1.5), bounds.bottom() - bounds.top())), accent())))
        } else {
            (
                Some(fill(
                    Bounds::from_corners(
                        point(bounds.left() + line.x_for_index(sel.start), bounds.top()),
                        point(bounds.left() + line.x_for_index(sel.end), bounds.bottom()),
                    ),
                    gpui::rgba(0x3b82f655),
                )),
                None,
            )
        };
        Prepaint { line: Some(line), cursor, selection }
    }
    fn paint(&mut self, _: Option<&GlobalElementId>, _: Option<&gpui::InspectorElementId>, bounds: Bounds<Pixels>, _: &mut (), pp: &mut Prepaint, window: &mut Window, cx: &mut App) {
        let fh = self.input.read(cx).focus_handle.clone();
        window.handle_input(&fh, ElementInputHandler::new(bounds, self.input.clone()), cx);
        if let Some(s) = pp.selection.take() {
            window.paint_quad(s);
        }
        let line = pp.line.take().unwrap();
        let _ = line.paint(bounds.origin, window.line_height(), gpui::TextAlign::Left, None, window, cx);
        if fh.is_focused(window) {
            if let Some(c) = pp.cursor.take() {
                window.paint_quad(c);
            }
        }
        let empty = self.input.read(cx).content.is_empty();
        self.input.update(cx, |i, _| {
            i.last_layout = if empty { None } else { Some(line) };
            i.last_bounds = Some(bounds);
        });
    }
}

impl Render for SearchInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = self.focus_handle.is_focused(window);
        div()
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::submit))
            .on_action(cx.listener(Self::cancel))
            .on_action(cx.listener(Self::down))
            .on_action(cx.listener(Self::up))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .w_full()
            .h(px(30.))
            .px_1()
            .flex()
            .items_center()
            .rounded_sm()
            .border_1()
            .border_color(if focused { accent() } else { border() })
            .bg(bg())
            .text_size(px(15.))
            .line_height(px(20.))
            .overflow_hidden()
            .child(TextElement { input: cx.entity() })
    }
}

impl Focusable for SearchInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

/// 搜尋比對：代號前綴（不分大小寫）或名稱包含。最多回傳 `limit` 筆列索引。
pub fn find_matches<'a>(rows: impl Iterator<Item = (usize, &'a str, &'a str)>, q: &str, limit: usize) -> Vec<usize> {
    let q = q.trim();
    if q.is_empty() {
        return Vec::new();
    }
    let qu = q.to_ascii_uppercase();
    let mut exact = Vec::new();
    let mut out = Vec::new();
    for (i, sym, name) in rows {
        if sym.eq_ignore_ascii_case(q) || name == q {
            exact.push(i);
        } else if sym.to_ascii_uppercase().starts_with(&qu) || name.contains(q) {
            if out.len() < limit {
                out.push(i);
            }
        }
        if exact.len() >= limit {
            break;
        }
    }
    exact.extend(out);
    exact.truncate(limit);
    exact
}
