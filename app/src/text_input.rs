//! A one-line text input: characters come through GPUI's text input handler (typing, the IME,
//! pasting), Enter submits the text, Escape clears it. The caret stays at the end of the text.

use std::ops::Range;

use gpui::{
    App, Bounds, ClipboardItem, Context, ElementInputHandler, EntityInputHandler, EventEmitter,
    FocusHandle, Focusable, KeyDownEvent, Pixels, Point, SharedString, UTF16Selection, Window,
    canvas, div, prelude::*, px, rgb,
};

/// Enter was pressed with this text; the input is empty again.
pub struct Submitted(pub String);

pub struct TextInput {
    text: String,
    /// The IME composition being typed, a byte range of `text`.
    marked: Option<Range<usize>>,
    /// Where the input was drawn, for the IME's candidate window.
    bounds: Option<Bounds<Pixels>>,
    placeholder: SharedString,
    focus: FocusHandle,
}

impl EventEmitter<Submitted> for TextInput {}

impl Focusable for TextInput {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl TextInput {
    pub fn new(placeholder: impl Into<SharedString>, cx: &mut Context<Self>) -> Self {
        Self {
            text: String::new(),
            marked: None,
            bounds: None,
            placeholder: placeholder.into(),
            focus: cx.focus_handle(),
        }
    }

    fn key_down(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        match keystroke.key.as_str() {
            "enter" => {
                let text = std::mem::take(&mut self.text);
                self.marked = None;
                cx.emit(Submitted(text));
            }
            "backspace" => {
                self.marked = None;
                self.text.pop();
            }
            "escape" => {
                self.marked = None;
                self.text.clear();
            }
            "v" if keystroke.modifiers.control || keystroke.modifiers.platform => {
                if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                    self.insert(None, &text);
                }
            }
            // A character: it comes through the input handler.
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }

    /// Replaces `range` (bytes; the composition or the end if none) with `text`, one line.
    fn insert(&mut self, range: Option<Range<usize>>, text: &str) -> Range<usize> {
        let range = range
            .or_else(|| self.marked.clone())
            .unwrap_or(self.text.len()..self.text.len());
        let range = clamp(&self.text, range);
        let text = text.chars().filter(|c| !c.is_control()).collect::<String>();
        self.text.replace_range(range.clone(), &text);
        range.start..range.start + text.len()
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        self.text[..offset.min(self.text.len())]
            .chars()
            .map(char::len_utf16)
            .sum()
    }

    fn utf16_to_offset(&self, offset: usize) -> usize {
        let mut utf16 = 0;
        for (ix, c) in self.text.char_indices() {
            if utf16 >= offset {
                return ix;
            }
            utf16 += c.len_utf16();
        }
        self.text.len()
    }

    fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.utf16_to_offset(range.start)..self.utf16_to_offset(range.end)
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }
}

/// `range` limited to `text` and to character boundaries.
fn clamp(text: &str, range: Range<usize>) -> Range<usize> {
    let fix = |mut offset: usize| {
        offset = offset.min(text.len());
        while !text.is_char_boundary(offset) {
            offset -= 1;
        }
        offset
    };
    let (start, end) = (fix(range.start), fix(range.end));
    start.min(end)..end.max(start)
}

impl EntityInputHandler for TextInput {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = clamp(&self.text, self.range_from_utf16(&range));
        adjusted.replace(self.range_to_utf16(&range));
        Some(self.text[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let end = self.offset_to_utf16(self.text.len());
        Some(UTF16Selection {
            range: end..end,
            reversed: false,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked.as_ref().map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.marked = None;
    }

    fn paste(&mut self, item: ClipboardItem, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = item.text() {
            self.insert(None, &text);
            cx.notify();
        }
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range.map(|range| self.range_from_utf16(&range));
        self.insert(range, text);
        self.marked = None;
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _selected: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range.map(|range| self.range_from_utf16(&range));
        let inserted = self.insert(range, text);
        self.marked = (!inserted.is_empty()).then_some(inserted);
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        _range: Range<usize>,
        bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        Some(self.bounds.unwrap_or(bounds))
    }

    fn character_index_for_point(
        &mut self,
        _point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        Some(self.offset_to_utf16(self.text.len()))
    }
}

impl Render for TextInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = self.focus.is_focused(window);
        let entity = cx.entity();
        let focus = self.focus.clone();
        let content = if self.text.is_empty() && !focused {
            div()
                .text_color(rgb(0x8a8a8a))
                .child(self.placeholder.clone())
        } else {
            div().child(format!("{}{}", self.text, if focused { "▏" } else { "" }))
        };
        div()
            .id("text-input")
            .relative()
            .track_focus(&self.focus)
            .w_full()
            .px_2()
            .rounded_sm()
            .border_1()
            .border_color(rgb(if focused { 0x569cd6 } else { 0x3c3c3c }))
            .bg(rgb(0x1e1e1e))
            .font_family("Consolas")
            .text_sm()
            .cursor_text()
            .on_click(cx.listener(|this, _, window, cx| {
                window.focus(&this.focus, cx);
                cx.notify();
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| this.key_down(event, cx)))
            .child(content)
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, cx| {
                        entity.update(cx, |input, _| input.bounds = Some(bounds));
                        window.handle_input(
                            &focus,
                            ElementInputHandler::new(bounds, entity.clone()),
                            cx,
                        );
                    },
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
            .min_h(px(22.))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_stay_on_character_boundaries() {
        assert_eq!(clamp("g_ticks", 3..99), 3..7);
        // "가" is 3 bytes.
        assert_eq!(clamp("a가b", 2..3), 1..1);
        // A reversed range comes back ordered.
        assert_eq!(clamp("a가b", Range { start: 5, end: 1 }), 1..5);
    }
}
