//! The memory tab of the bottom panel: [`ROWS`] rows of [`ROW`] bytes from an address typed in
//! (hex, `@core` optional) or a variable's "mem", read again every 500 ms while the tab is
//! shown, through the core chosen (a core's local addresses are its own memory).

use aurix_debug_model::memory::{ROW, ROWS, parse_bytes, parse_location};
use aurix_debug_model::reference::{parse_address, split_core};
use dap_types::requests::{ReadMemory, WriteMemory};
use gpui::{AnyElement, Context, SharedString, div, prelude::*, px};

use super::panels::BottomTab;
use super::{Debugger, button, color, hsla};

impl Debugger {
    /// The address typed into the memory input.
    pub(super) fn memory_submitted(&mut self, text: &str) {
        match parse_location(text) {
            Ok((address, core)) => {
                let core = core.or_else(|| self.default_memory_core());
                self.memory.show(address, core);
                self.memory_focus = Some(address);
                self.memory_input_error = None;
            }
            Err(error) => self.memory_input_error = Some(error),
        }
        self.bottom = BottomTab::Memory;
    }

    /// Shows the memory at a variable's memory reference (`0x7000bfe4@0`).
    pub(super) fn show_memory_at(&mut self, reference: &str) {
        let (address, core) = split_core(reference);
        if let Some(address) = parse_address(address) {
            let core = core.or_else(|| self.default_memory_core());
            self.memory.show(address, core);
            self.memory_focus = Some(address);
            self.memory_input_error = None;
            self.bottom = BottomTab::Memory;
        }
    }

    /// The core of the HSM thread, if the session has one.
    fn hsm_core(&self) -> Option<u64> {
        self.hsm_thread.and_then(|id| u64::try_from(id).ok())
    }

    /// The core memory is read through when none is named: the selected thread's, in a session
    /// of several cores.
    fn default_memory_core(&self) -> Option<u64> {
        if self.threads.len() > 1 {
            self.selected_thread.and_then(|id| u64::try_from(id).ok())
        } else {
            None
        }
    }

    /// The bytes typed into the write input: written at the selected address if it is RAM.
    pub(super) fn memory_write(&mut self, text: &str, cx: &mut Context<Self>) {
        let Some(address) = self.memory_focus else {
            return;
        };
        // The RAM check knows the TriCore address map, not the HSM's own.
        if self.memory.core().is_some() && self.memory.core() == self.hsm_core() {
            self.memory_write_note = Some(Err(
                "writes through the HSM core are not supported: its address space is its own"
                    .into(),
            ));
            return;
        }
        let prepared = parse_bytes(text).and_then(|bytes| {
            let (request, ram) = self.memory.write_request(address, &bytes)?;
            Ok((request, ram, bytes.len()))
        });
        let (request, ram, count) = match prepared {
            Ok(prepared) => prepared,
            Err(error) => {
                self.memory_write_note = Some(Err(error));
                return;
            }
        };
        let reference = request.memory_reference.clone();
        self.request::<WriteMemory>(request, cx, move |this, result, _| {
            let note = match result {
                Ok(_) => Ok(format!("wrote {count} byte(s) to {ram} at {reference}")),
                Err(error) => Err(format!("writeMemory: {error}")),
            };
            match &note {
                Ok(note) | Err(note) => this.log(note.clone()),
            }
            this.memory_write_note = Some(note);
        });
    }

    /// Sends the next read of the memory tab, if it is shown.
    pub(super) fn read_memory(&mut self, cx: &mut Context<Self>) {
        if self.bottom != BottomTab::Memory || self.memory_paused {
            return;
        }
        let Some((request, generation)) = self.memory.take_request() else {
            return;
        };
        self.request::<ReadMemory>(request, cx, move |this, result, _| {
            this.memory.answered(
                generation,
                result.map(|response| (response.data, response.unreadable_bytes)),
            );
        });
    }

    pub(super) fn render_memory(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let shown = self.memory.start().is_some();
        let paused = self.memory_paused;
        let cores = if self.threads.len() > 1 {
            self.threads
                .iter()
                .filter_map(|row| Some((u64::try_from(row.id).ok()?, row.name.clone())))
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        let current_core = self.memory.core();
        let note = match (&self.memory_input_error, self.memory.error()) {
            (Some(error), _) => Some(error.clone()),
            (None, Some(error)) => Some(error.to_string()),
            (None, None) => None,
        };
        let rows = self
            .memory
            .rows()
            .map(|(address, bytes, changed)| {
                let mut hex = div().flex().gap(px(6.));
                let mut ascii = String::with_capacity(ROW);
                for (ix, (byte, changed)) in bytes.iter().zip(changed).enumerate() {
                    let (text, colour) = match byte {
                        Some(byte) if *changed => (format!("{byte:02X}"), color::CHANGED),
                        Some(byte) => (format!("{byte:02X}"), color::TEXT),
                        None => ("??".to_string(), color::MUTED),
                    };
                    // The address asked for (typed, the variable's, or clicked): writes go there.
                    let at = address + ix as u64;
                    let focused = self.memory_focus == Some(at);
                    hex = hex.child(
                        div()
                            .id(SharedString::from(format!("byte-{at:x}")))
                            .when(ix == ROW / 2, |this| this.ml(px(8.)))
                            .when(focused, |this| this.bg(hsla(color::SELECTED)).rounded_sm())
                            .cursor_pointer()
                            .hover(|style| style.bg(hsla(color::BUTTON_HOVER)))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.memory_focus = Some(at);
                                this.memory_write_note = None;
                                cx.notify();
                            }))
                            .text_color(hsla(colour))
                            .child(text),
                    );
                    ascii.push(match byte {
                        Some(byte) if byte.is_ascii_graphic() || *byte == b' ' => *byte as char,
                        Some(_) => '.',
                        None => ' ',
                    });
                }
                div()
                    .flex()
                    .gap_4()
                    .px_2()
                    .font_family("Consolas")
                    .text_xs()
                    .child(
                        div()
                            .w(px(80.))
                            .text_color(hsla(color::MUTED))
                            .child(format!("{address:08X}")),
                    )
                    .child(hex)
                    .child(div().text_color(hsla(color::MUTED)).child(ascii))
                    .into_any_element()
            })
            .collect::<Vec<_>>();
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .px_1()
                    .py_0p5()
                    .child(div().w(px(220.)).child(self.memory_input.clone()))
                    .child(button(
                        "memory-back",
                        "◀",
                        shown,
                        cx.listener(|this, _, _, cx| {
                            this.memory.scroll(-(ROWS as i64));
                            cx.notify();
                        }),
                    ))
                    .child(button(
                        "memory-forward",
                        "▶",
                        shown,
                        cx.listener(|this, _, _, cx| {
                            this.memory.scroll(ROWS as i64);
                            cx.notify();
                        }),
                    ))
                    .child(button(
                        "memory-pause",
                        if paused { "Resume" } else { "Pause reading" },
                        shown,
                        cx.listener(move |this, _, _, cx| {
                            this.memory_paused = !paused;
                            cx.notify();
                        }),
                    ))
                    .children(cores.into_iter().map(|(core, name)| {
                        let selected = current_core == Some(core);
                        div()
                            .id(SharedString::from(format!("memory-core-{core}")))
                            .px_2()
                            .rounded_sm()
                            .text_xs()
                            .cursor_pointer()
                            .text_color(hsla(if selected { color::TEXT } else { color::MUTED }))
                            .when(selected, |this| this.bg(hsla(color::SELECTED)))
                            .hover(|style| style.text_color(hsla(color::TEXT)))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.memory.set_core(Some(core));
                                cx.notify();
                            }))
                            .child(name)
                    }))
                    .when_some(note, |this, note| {
                        this.child(
                            div()
                                .px_2()
                                .text_xs()
                                .text_color(hsla(color::CHANGED))
                                .child(note),
                        )
                    }),
            )
            .when_some(self.cache_memory_note().filter(|_| shown), |this, note| {
                this.child(
                    div()
                        .px_2()
                        .pb_0p5()
                        .text_xs()
                        .text_color(hsla(color::CHANGED))
                        .child(note),
                )
            })
            .when_some(self.memory_focus.filter(|_| shown), |this, at| {
                let (note, colour) = match &self.memory_write_note {
                    Some(Ok(note)) => (note.clone(), color::ACCENT),
                    Some(Err(error)) => (error.clone(), color::CHANGED),
                    None => (
                        "RAM only (scratch-pad RAMs, LMU); flash, UCBs and registers are refused"
                            .to_string(),
                        color::MUTED,
                    ),
                };
                this.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_1()
                        .pb_0p5()
                        .child(
                            div()
                                .text_xs()
                                .font_family("Consolas")
                                .child(format!("Write at {at:08X}")),
                        )
                        .child(div().w(px(220.)).child(self.memory_write_input.clone()))
                        .child(div().text_xs().text_color(hsla(colour)).child(note)),
                )
            })
            .child(
                div()
                    .id("memory-rows")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .when(!shown, |this| {
                        this.child(div().px_2().text_sm().text_color(hsla(color::MUTED)).child(
                            "Type an address (hex, @core optional), or \"mem\" on a variable",
                        ))
                    })
                    .children(rows),
            )
            .into_any_element()
    }
}
