//! The breakpoint edit row of the source view: a right click on the gutter of a line opens it,
//! with the condition, hit count and log message of the breakpoint there. probe-rs checks them at
//! each halt and runs the core on when they are not met.

use aurix_debug_model::breakpoints::BreakpointSettings;
use gpui::{Context, Entity, Focusable, SharedString, Window, div, prelude::*, px};

use super::{Debugger, button, color, hsla};
use crate::text_input::TextInput;

/// The fields of the edit row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Field {
    Condition,
    Hit,
    Log,
}

/// The line being edited and its inputs.
pub(super) struct BreakpointEdit {
    path: String,
    line: u64,
    condition: Entity<TextInput>,
    hit: Entity<TextInput>,
    log: Entity<TextInput>,
}

impl BreakpointEdit {
    pub(super) fn new(cx: &mut Context<Debugger>) -> Self {
        let edit = Self {
            path: String::new(),
            line: 0,
            condition: cx.new(|cx| TextInput::new("v == 3 && k != 0", cx)),
            hit: cx.new(|cx| TextInput::new("3 >=3 %4", cx)),
            log: cx.new(|cx| TextInput::new("v = {v}", cx)),
        };
        for (input, field) in [
            (&edit.condition, Field::Condition),
            (&edit.hit, Field::Hit),
            (&edit.log, Field::Log),
        ] {
            cx.subscribe(
                input,
                move |this, _, crate::text_input::Submitted(text), cx| {
                    this.apply_breakpoint_edit(Some((field, text.clone())), cx);
                    cx.notify();
                },
            )
            .detach();
        }
        edit
    }

    fn input(&self, field: Field) -> &Entity<TextInput> {
        match field {
            Field::Condition => &self.condition,
            Field::Hit => &self.hit,
            Field::Log => &self.log,
        }
    }
}

impl Debugger {
    /// Opens the edit row for `line` of `path`, with the settings of the breakpoint there.
    pub(super) fn open_breakpoint_edit(
        &mut self,
        path: String,
        line: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let settings = self
            .source_breakpoints
            .get(&path, line)
            .map(|state| state.settings.clone())
            .unwrap_or_default();
        let edit = &mut self.breakpoint_edit;
        edit.path = path;
        edit.line = line;
        for (field, text) in [
            (Field::Condition, settings.condition),
            (Field::Hit, settings.hit_condition),
            (Field::Log, settings.log_message),
        ] {
            edit.input(field)
                .update(cx, |input, cx| input.set_text(text.unwrap_or_default(), cx));
        }
        let focus = edit.condition.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
        self.breakpoint_editing = true;
        cx.notify();
    }

    /// Sets the breakpoint of the edit row with what its fields say and closes it. Enter in a
    /// field empties it, so `submitted` brings that field's text.
    fn apply_breakpoint_edit(
        &mut self,
        submitted: Option<(Field, String)>,
        cx: &mut Context<Self>,
    ) {
        if !self.breakpoint_editing {
            return;
        }
        let edit = &self.breakpoint_edit;
        let text = |field: Field| match &submitted {
            Some((which, text)) if *which == field => text.clone(),
            _ => edit.input(field).read(cx).text().to_string(),
        };
        let settings = BreakpointSettings::new(
            &text(Field::Condition),
            &text(Field::Hit),
            &text(Field::Log),
        );
        let (path, line) = (edit.path.clone(), edit.line);
        self.breakpoint_editing = false;
        self.source_breakpoints.set(&path, line, settings);
        self.send_source_breakpoints(path, cx);
    }

    /// Removes the breakpoint of the edit row, if there is one, and closes it.
    fn remove_breakpoint_edit(&mut self, cx: &mut Context<Self>) {
        self.breakpoint_editing = false;
        let (path, line) = (self.breakpoint_edit.path.clone(), self.breakpoint_edit.line);
        if self.source_breakpoints.get(&path, line).is_some() {
            self.source_breakpoints.toggle(&path, line);
            self.send_source_breakpoints(path, cx);
        }
    }

    pub(super) fn render_breakpoint_edit(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        if !self.breakpoint_editing {
            return None;
        }
        let edit = &self.breakpoint_edit;
        let file = edit.path.rsplit(['/', '\\']).next().unwrap_or(&edit.path);
        let exists = self.source_breakpoints.get(&edit.path, edit.line).is_some();
        let state = self.source_breakpoints.get(&edit.path, edit.line);
        let note = state
            .and_then(|state| (state.verified == Some(false)).then(|| state.message.clone()))
            .flatten();
        let field = |label: &'static str, input: &Entity<TextInput>, width: f32| {
            div()
                .flex()
                .items_center()
                .gap_1()
                .child(div().text_xs().text_color(hsla(color::MUTED)).child(label))
                .child(div().w(px(width)).child(input.clone()))
        };
        Some(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .px_2()
                .py_1()
                .bg(hsla(color::PANEL))
                .border_b_1()
                .border_color(hsla(color::BORDER))
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap_2()
                        .child(div().text_sm().child(SharedString::from(format!(
                            "Breakpoint {file}:{}",
                            edit.line
                        ))))
                        .child(field("if", &edit.condition, 180.))
                        .child(field("hit", &edit.hit, 80.))
                        .child(field("log", &edit.log, 150.))
                        .child(button(
                            "breakpoint-edit-set",
                            "Set",
                            true,
                            cx.listener(|this, _, _, cx| {
                                this.apply_breakpoint_edit(None, cx);
                                cx.notify();
                            }),
                        ))
                        .child(button(
                            "breakpoint-edit-remove",
                            "Remove",
                            exists,
                            cx.listener(|this, _, _, cx| {
                                this.remove_breakpoint_edit(cx);
                                cx.notify();
                            }),
                        ))
                        .child(button(
                            "breakpoint-edit-cancel",
                            "Cancel",
                            true,
                            cx.listener(|this, _, _, cx| {
                                this.breakpoint_editing = false;
                                cx.notify();
                            }),
                        )),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(hsla(if note.is_some() {
                            color::CHANGED
                        } else {
                            color::MUTED
                        }))
                        .child(SharedString::from(note.unwrap_or_else(|| {
                            "Blank: a plain breakpoint. A log message does not stop. The core \
                             halts at each hit and runs on when it does not stop."
                                .to_string()
                        }))),
                ),
        )
    }
}
