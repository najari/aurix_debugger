//! Sessions: starting one with an attach file, ending it, and the page shown without one (the
//! attach files used before, a file dialog, the server address).
//!
//! A new session starts from nothing but what belongs to the program rather than the session:
//! the breakpoints (sent again once attached) when the attach file is the same, the Live Watch
//! expressions (kept next to the attach file), the memory address shown.

use aurix_debug_model::breakpoints::InstructionBreakpoints;
use aurix_debug_model::live_watch::LiveWatch;
use aurix_debug_model::rtt::RttChannels;
use aurix_debug_model::source_breakpoints::SourceBreakpoints;
use dap_types::requests::{SetBreakpoints, SetInstructionBreakpoints};
use gpui::{AnyElement, Context, PathPromptOptions, SharedString, div, prelude::*, px};

use super::panels::{self, BottomTab};
use super::{Debugger, NOT_STOPPED, button, color, end_in_background, hsla};
use crate::recent::{self, AttachSummary};

impl Debugger {
    /// Starts a session with the attach file `attach` (ending the one there is).
    pub(super) fn start(&mut self, attach: String, cx: &mut Context<Self>) {
        let server = self.server_input.read(cx).text().trim().to_string();
        if !server.is_empty() {
            self.server = server;
        }
        if self.client.is_some() || self.connecting {
            self.end_session("Disconnected", cx);
        }
        let same_program = self
            .attach
            .as_deref()
            .is_some_and(|old| recent::same_path(old, &attach));
        if !same_program {
            self.breakpoints = InstructionBreakpoints::default();
            self.source_breakpoints = SourceBreakpoints::default();
            let watch_file = panels::watch_file(&attach);
            self.live_watch = LiveWatch::new(panels::read_watch_file(&watch_file));
            self.watch_file = Some(watch_file);
            self.load_os_for(&attach);
            self.load_gtm_for(&attach);
            self.load_cache_for(&attach);
        }
        if let Err(error) = recent::remember(&mut self.recent, &attach) {
            self.log(format!("recent attach files: {error}"));
        }
        self.log(format!("attach {attach} on {}", self.server));
        self.attach = Some(attach);
        self.connect(cx);
    }

    /// Ends the session, without waiting for the server; the page without one is shown.
    pub(super) fn end_session(&mut self, status: &str, cx: &mut Context<Self>) {
        self.session += 1;
        self.connecting = false;
        if let Some(client) = self.client.take() {
            end_in_background(client, cx);
        }
        self.status = status.to_string().into();
        self.reset_session();
    }

    /// Forgets what the ended session showed.
    fn reset_session(&mut self) {
        self.threads.clear();
        self.selected_thread = None;
        self.clear_stack();
        self.source = Err(NOT_STOPPED.into());
        self.rtt = RttChannels::default();
        if matches!(self.bottom, BottomTab::Rtt(_)) {
            self.bottom = BottomTab::Console;
        }
        // Reads under way in the old session never answer.
        self.live_watch = LiveWatch::new(self.live_watch.expressions().to_vec());
        self.os_busy = false;
        self.gtm_busy = false;
        self.cache_busy = false;
        // A measurement of the old session cannot go on (its CCTRL writes stay as they were).
        if let Some(view) = &mut self.cache
            && view.measuring()
        {
            *view = aurix_debug_model::cache::CacheView::new(view.caches().clone());
        }
        if let Some(start) = self.memory.start() {
            self.memory.show(start, self.memory.core());
        }
    }

    /// Sends the breakpoints kept from the session before (same program).
    pub(super) fn resend_breakpoints(&mut self, cx: &mut Context<Self>) {
        let paths = self
            .source_breakpoints
            .paths()
            .map(String::from)
            .collect::<Vec<_>>();
        for path in paths {
            let request = self.source_breakpoints.request(&path);
            let asked = request.clone();
            self.request::<SetBreakpoints>(request, cx, move |this, result, _| match result {
                Ok(response) => this
                    .source_breakpoints
                    .answered(&asked, Ok(&response.breakpoints)),
                Err(error) => this.source_breakpoints.answered(&asked, Err(&error)),
            });
        }
        if !self.breakpoints.is_empty() {
            let request = self.breakpoints.request();
            let asked = request.clone();
            self.request::<SetInstructionBreakpoints>(request, cx, move |this, result, _| {
                match result {
                    Ok(response) => this.breakpoints.answered(&asked, Ok(&response.breakpoints)),
                    Err(error) => this.breakpoints.answered(&asked, Err(&error)),
                }
            });
        }
    }

    fn open_attach_dialog(&mut self, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Attach".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            this.update(cx, |this, cx| {
                this.start(path.to_string_lossy().into_owned(), cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The page without a session.
    pub(super) fn render_connect(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let connecting = self.connecting;
        let entries = self
            .recent
            .clone()
            .into_iter()
            .enumerate()
            .map(|(ix, path)| {
                let (summary, colour) = match AttachSummary::read(&path) {
                    Ok(summary) => (summary.line(), color::MUTED),
                    Err(error) => (error, color::CHANGED),
                };
                let current = self
                    .attach
                    .as_deref()
                    .is_some_and(|attach| recent::same_path(attach, &path));
                let start = path.clone();
                div()
                    .id(SharedString::from(format!("recent-{ix}")))
                    .flex()
                    .flex_col()
                    .px_3()
                    .py_1()
                    .rounded_sm()
                    .when(current, |this| this.bg(hsla(color::SELECTED)))
                    .when(!connecting, |this| {
                        this.cursor_pointer()
                            .hover(|style| style.bg(hsla(color::BUTTON_HOVER)))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.start(start.clone(), cx);
                                cx.notify();
                            }))
                    })
                    .child(
                        div()
                            .text_sm()
                            .text_color(hsla(color::TEXT))
                            .child(recent::file_name(&path).to_string()),
                    )
                    .child(div().text_xs().text_color(hsla(colour)).child(summary))
                    .child(div().text_xs().text_color(hsla(color::MUTED)).child(path))
                    .into_any_element()
            })
            .collect::<Vec<_>>();
        let empty = entries.is_empty();
        div()
            .id("connect")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .items_center()
            .overflow_y_scroll()
            .child(
                div()
                    .w(px(720.))
                    .flex()
                    .flex_col()
                    .gap_2()
                    .py_6()
                    .child(div().text_xl().child("Attach to a target"))
                    .child(div().text_sm().text_color(hsla(color::MUTED)).child(
                        "An attach file holds the probe-rs attach arguments: chip, probe, \
                                 and the program of each core (coreConfigs).",
                    ))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(div().w(px(60.)).text_sm().child("Server"))
                            .child(div().w(px(260.)).child(self.server_input.clone()))
                            .child(button(
                                "open-attach",
                                "Open attach file…",
                                !connecting,
                                cx.listener(|this, _, _, cx| this.open_attach_dialog(cx)),
                            )),
                    )
                    .when(connecting, |this| {
                        this.child(
                            div()
                                .text_sm()
                                .text_color(hsla(color::ACCENT))
                                .child(self.status.clone()),
                        )
                    })
                    .when_some(self.connect_error.clone(), |this, error| {
                        this.child(
                            div()
                                .text_sm()
                                .text_color(hsla(color::CHANGED))
                                .child(format!("Not attached: {error}")),
                        )
                    })
                    .child(
                        div()
                            .pt_2()
                            .text_xs()
                            .text_color(hsla(color::MUTED))
                            .child("RECENT"),
                    )
                    .when(empty, |this| {
                        this.child(
                            div()
                                .text_sm()
                                .text_color(hsla(color::MUTED))
                                .child("None yet: open an attach file."),
                        )
                    })
                    .children(entries),
            )
            .into_any_element()
    }
}
