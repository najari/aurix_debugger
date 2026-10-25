//! The GTM tab: the eight channels of one MCS instance of the GTM (enabled, PC, R0–R7, flags,
//! stack depth) read every [`INTERVAL_MS`] while the tab is shown, the cores running or not,
//! with the changed values marked, and the MCS code around the program counter of the selected
//! channel. The registers are read word by word through the probe-rs fork's `readRanges`
//! request (`aurix_debug_model::gtm`). Only read: nothing is written to the GTM.
//!
//! The tab knows the GTM of the TC27x; for another chip it says so.

use aurix_debug_model::gtm::{Gtm, GtmView, MCS_COUNT};
use aurix_debug_model::live_watch::INTERVAL_MS;
use aurix_debug_model::protocol::{ReadRanges, ReadRangesArguments};
use gpui::{AnyElement, Context, SharedString, div, prelude::*, px};

use super::panels::BottomTab;
use super::{Debugger, button, color, hsla};
use crate::recent::AttachSummary;

impl Debugger {
    /// The GTM view for the chip of `attach`, if its GTM is known.
    pub(super) fn load_gtm_for(&mut self, attach: &str) {
        self.gtm_busy = false;
        self.gtm = AttachSummary::read(attach)
            .ok()
            .and_then(|summary| summary.chip)
            .and_then(|chip| Gtm::for_chip(&chip))
            .map(GtmView::new);
    }

    /// Reads the channels of the MCS shown while the tab is shown (one read at a time).
    pub(super) fn read_gtm(&mut self, cx: &mut Context<Self>) {
        if self.bottom != BottomTab::Gtm || self.gtm_busy || self.client.is_none() {
            return;
        }
        let Some(view) = &self.gtm else {
            return;
        };
        let ranges = view.ranges();
        let asked = ranges.clone();
        // Any core reaches the GTM: the selected thread's, else the first.
        let core = self
            .selected_thread
            .or_else(|| self.threads.first().map(|t| t.id))
            .and_then(|id| u64::try_from(id).ok());
        self.gtm_busy = true;
        self.request::<ReadRanges>(
            ReadRangesArguments { ranges, core },
            cx,
            move |this, result, _| {
                this.gtm_busy = false;
                if let Some(view) = &mut this.gtm {
                    match result {
                        Ok(response) => view.answered(&asked, &response.ranges),
                        Err(error) => view.failed(&error),
                    }
                }
            },
        );
    }

    pub(super) fn render_gtm(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let Some(view) = &self.gtm else {
            return div()
                .p_2()
                .text_sm()
                .text_color(hsla(color::MUTED))
                .child("No GTM view for this chip (the TC27x GTM is known)")
                .into_any_element();
        };
        let mcs = view.mcs();
        let selected = view.selected();
        let layout = match (view.mcfg_ctrl(), view.memory_size()) {
            (Some(mcfg), Some((mp0, mp1))) => format!(
                "MCFG_CTRL {mcfg:#04x}: MCS{mcs} memory {:#x} bytes (page 0 {mp0:#x}, page 1 {:#x}), at {:#010x}",
                mp1,
                mp1 - mp0,
                view.gtm().memory(mcs)
            ),
            _ => format!("read every {INTERVAL_MS} ms"),
        };
        let note = view.error().map(str::to_string);
        let rows = view.rows();
        let code = view.code();

        const IDS: [&str; MCS_COUNT] = ["gtm-mcs-0", "gtm-mcs-1", "gtm-mcs-2", "gtm-mcs-3"];
        const NAMES: [&str; MCS_COUNT] = ["MCS0", "MCS1", "MCS2", "MCS3"];
        let mut picker = div().flex().items_center().gap_1();
        for i in 0..MCS_COUNT {
            picker = picker.child(button(
                IDS[i],
                NAMES[i],
                i != mcs,
                cx.listener(move |this, _, _, cx| {
                    if let Some(view) = &mut this.gtm {
                        view.show_mcs(i);
                    }
                    cx.notify();
                }),
            ));
        }

        let cell = |text: String, width: f32, colour| {
            div()
                .w(px(width))
                .flex_none()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_color(hsla(colour))
                .child(text)
        };
        let header = div()
            .flex()
            .text_xs()
            .text_color(hsla(color::MUTED))
            .child(cell("Ch".into(), 32., color::MUTED))
            .child(cell("EN".into(), 32., color::MUTED))
            .child(cell("PC".into(), 64., color::MUTED))
            .children((0..8).map(|i| cell(format!("R{i}"), 72., color::MUTED)))
            .child(cell("NVZC".into(), 48., color::MUTED))
            .child(cell("SP".into(), 28., color::MUTED))
            .child(cell("IRQ ERR".into(), 64., color::MUTED));
        let table = div()
            .flex()
            .flex_col()
            .child(header)
            .children(rows.into_iter().map(|row| {
                let channel = row.channel;
                let line = div()
                    .id(SharedString::from(format!("gtm-ch-{channel}")))
                    .flex()
                    .font_family("Consolas")
                    .text_sm()
                    .cursor_pointer()
                    .hover(|style| style.bg(hsla(color::SELECTED)))
                    .when(channel == selected, |this| this.bg(hsla(color::SELECTED)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(view) = &mut this.gtm {
                            view.select(channel);
                        }
                        cx.notify();
                    }))
                    .child(cell(channel.to_string(), 32., color::TEXT));
                match row.state {
                    Ok(state) => {
                        let on = state.enabled();
                        let base = if on { color::TEXT } else { color::MUTED };
                        let mark = |changed: bool| if changed { color::CHANGED } else { base };
                        line.child(cell(
                            if on { "on" } else { "off" }.into(),
                            32.,
                            mark(row.changed[9]),
                        ))
                        .child(cell(
                            format!("{:#06x}", state.pc()),
                            64.,
                            mark(row.changed[0]),
                        ))
                        .children((0..8).map(|i| {
                            cell(
                                format!("{:#08x}", state.r[i] & 0xFF_FFFF),
                                72.,
                                mark(row.changed[1 + i]),
                            )
                        }))
                        .child(cell(state.flags(), 48., mark(row.changed[9])))
                        .child(cell(
                            state.stack_depth().to_string(),
                            28.,
                            mark(row.changed[9]),
                        ))
                        .child(cell(
                            format!(
                                "{} {}",
                                if state.irq() { "IRQ" } else { "-" },
                                if state.error() { "ERR" } else { "-" }
                            ),
                            64.,
                            mark(row.changed[9]),
                        ))
                    }
                    Err(error) => line.child(cell(error, 600., color::MUTED)),
                }
            }));

        let code_panel =
            match code {
                Ok(lines) if lines.is_empty() => div()
                    .text_xs()
                    .text_color(hsla(color::MUTED))
                    .child(format!(
                        "Code of channel {selected}: read at the next refresh"
                    )),
                Ok(lines) => div()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_xs()
                            .text_color(hsla(color::MUTED))
                            .child(format!("MCS{mcs} channel {selected}")),
                    )
                    .children(lines.into_iter().map(|line| {
                        div()
                            .flex()
                            .gap_2()
                            .font_family("Consolas")
                            .text_sm()
                            .when(line.at_pc, |this| this.bg(hsla(color::SELECTED)))
                            .child(
                                div()
                                    .w(px(16.))
                                    .text_color(hsla(color::ACCENT))
                                    .child(if line.at_pc { "▶" } else { "" }),
                            )
                            .child(
                                div()
                                    .w(px(56.))
                                    .text_color(hsla(color::MUTED))
                                    .child(format!("{:#06x}", line.address)),
                            )
                            .child(
                                div()
                                    .w(px(88.))
                                    .text_color(hsla(color::MUTED))
                                    .child(format!("{:08x}", line.word)),
                            )
                            .child(div().text_color(hsla(color::TEXT)).child(line.text))
                    })),
                Err(error) => div()
                    .text_xs()
                    .text_color(hsla(color::MUTED))
                    .child(format!("Code of channel {selected}: {error}")),
            };

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .px_1()
                    .py_0p5()
                    .child(picker)
                    .child(div().text_xs().text_color(hsla(color::MUTED)).child(layout))
                    .when_some(note, |this, note| {
                        this.child(div().text_xs().text_color(hsla(color::CHANGED)).child(note))
                    }),
            )
            .child(
                div()
                    .id("gtm-body")
                    .flex()
                    .gap_4()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px_2()
                    .child(table)
                    .child(code_panel),
            )
            .into_any_element()
    }
}
