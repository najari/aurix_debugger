//! The splitters between the panels: pressed, the mouse moves them until it is let go (the
//! window's root follows the mouse while a drag is under way); a double click puts one back.

use gpui::{
    Context, MouseButton, MouseDownEvent, MouseMoveEvent, SharedString, Window, div, prelude::*, px,
};

use super::{Debugger, color, hsla};
use crate::layout::{self, Layout, Splitter};

/// How thick a splitter is.
const THICKNESS: f32 = 5.;

fn window_size(window: &Window) -> (f32, f32) {
    let size = window.viewport_size();
    (size.width.as_f32(), size.height.as_f32())
}

impl Debugger {
    fn splitter_down(&mut self, splitter: Splitter, event: &MouseDownEvent, window: &Window) {
        if event.click_count >= 2 {
            self.drag = None;
            self.layout.reset(splitter, window_size(window));
            self.save_layout();
            self.scroll_to_line = true;
            self.scroll_to_pc = true;
        } else {
            let mouse = (event.position.x.as_f32(), event.position.y.as_f32());
            self.drag = Some(self.layout.begin(splitter, mouse));
        }
    }

    /// The mouse moved over the window: the splitter being dragged follows it.
    pub(super) fn mouse_moved(
        &mut self,
        event: &MouseMoveEvent,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let Some(drag) = self.drag else {
            return;
        };
        if event.pressed_button != Some(MouseButton::Left) {
            // Let go where the window did not see it.
            self.end_drag();
            cx.notify();
            return;
        }
        let mouse = (event.position.x.as_f32(), event.position.y.as_f32());
        let layout = Layout::dragged(&drag, mouse, window_size(window));
        if layout != self.layout {
            self.layout = layout;
            cx.notify();
        }
    }

    pub(super) fn end_drag(&mut self) {
        if self.drag.take().is_some() {
            self.save_layout();
            // A smaller view may have lost the line and the instruction it showed.
            self.scroll_to_line = true;
            self.scroll_to_pc = true;
        }
    }

    fn save_layout(&mut self) {
        if let Err(error) = layout::save(&self.layout) {
            self.log(format!("layout: {error}"));
        }
    }

    /// The layout fitted to the window as it is now (it may have been made smaller).
    pub(super) fn fit_layout(&mut self, window: &Window) {
        self.layout = self.layout.fitted(window_size(window));
    }

    pub(super) fn splitter(&self, splitter: Splitter, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.drag.is_some_and(|drag| drag.splitter == splitter);
        div()
            .id(SharedString::from(format!("splitter-{splitter:?}")))
            .flex_none()
            .bg(hsla(if active { color::ACCENT } else { color::BORDER }))
            .hover(|style| style.bg(hsla(color::ACCENT)))
            .map(|this| {
                if splitter.across() {
                    this.w(px(THICKNESS)).h_full().cursor_ew_resize()
                } else {
                    this.h(px(THICKNESS)).w_full().cursor_ns_resize()
                }
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    this.splitter_down(splitter, event, window);
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
    }
}
