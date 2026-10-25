//! The Cache tab: per core the program cache and the data cache (or data read buffer) with
//! their sizes and whether they are on, the segments cached for code and for data, read every
//! [`INTERVAL_MS`] while the tab or the memory tab is shown (the memory tab says when the
//! memory it shows may be cached). Measure counts the hits and misses with the performance
//! counters (`CCTRL` set for two reads, then put back): the only writes of the tab.
//!
//! The lines in the caches are not shown: their tags can be read only with the cache off and
//! mapped (`MTU_MEMMAP`), and their format is not public (`aurix_debug_model::cache`).

use aurix_debug_model::cache::{CacheRow, CacheView, Caches};
use aurix_debug_model::live_watch::INTERVAL_MS;
use aurix_debug_model::protocol::{ReadRanges, ReadRangesArguments};
use dap_types::requests::WriteMemory;
use gpui::{AnyElement, Context, div, prelude::*, px};

use super::panels::BottomTab;
use super::{Debugger, button, color, hsla};
use crate::recent::AttachSummary;

impl Debugger {
    /// The cache view for the chip of `attach`, if its caches are known.
    pub(super) fn load_cache_for(&mut self, attach: &str) {
        self.cache_busy = false;
        self.cache = AttachSummary::read(attach)
            .ok()
            .and_then(|summary| summary.chip)
            .and_then(|chip| Caches::for_chip(&chip))
            .map(CacheView::new);
    }

    /// Reads the cache registers while the Cache or the memory tab is shown, or a measurement
    /// runs (one read at a time), after the counter writes a measurement asks for.
    pub(super) fn read_cache(&mut self, cx: &mut Context<Self>) {
        if self.cache_busy || self.client.is_none() {
            return;
        }
        let Some(view) = &mut self.cache else {
            return;
        };
        let shown = matches!(self.bottom, BottomTab::Cache | BottomTab::Memory);
        if !shown && !view.measuring() {
            return;
        }
        for write in view.take_writes() {
            self.request::<WriteMemory>(write.arguments(), cx, move |this, result, _| {
                if let Err(error) = result {
                    this.log(format!(
                        "CPU{} CCTRL = {:#x}: {error}",
                        write.core, write.value
                    ));
                }
            });
        }
        let Some(view) = &self.cache else {
            return;
        };
        let (ranges, generation) = view.ranges();
        // The CSFRs are global addresses: any core reaches them.
        let core = self
            .selected_thread
            .or_else(|| self.threads.first().map(|t| t.id))
            .and_then(|id| u64::try_from(id).ok());
        self.cache_busy = true;
        self.request::<ReadRanges>(
            ReadRangesArguments { ranges, core },
            cx,
            move |this, result, _| {
                this.cache_busy = false;
                let Some(view) = &mut this.cache else {
                    return;
                };
                let was_measuring = view.measuring();
                match result {
                    Ok(response) => view.answered(generation, &response.ranges),
                    Err(error) => view.failed(&error),
                }
                if was_measuring && !view.measuring() {
                    let summary = view
                        .rows()
                        .into_iter()
                        .flatten()
                        .filter_map(|row| {
                            let (program, data) = row.measured?;
                            Some(format!("CPU{}: {program}; {data}", row.core))
                        })
                        .collect::<Vec<_>>();
                    for line in summary {
                        this.log(format!("cache {line}"));
                    }
                }
            },
        );
    }

    /// What the caches mean for the memory the memory tab shows, if anything.
    pub(super) fn cache_memory_note(&self) -> Option<String> {
        let start = self.memory.start()?;
        self.cache.as_ref()?.memory_note(start)
    }

    fn measure_cache(&mut self, cx: &mut Context<Self>) {
        if let Some(view) = &mut self.cache
            && view.measure()
        {
            self.log("cache: measuring the hits and misses (CCTRL, then put back)".to_string());
            self.read_cache(cx);
        }
    }

    pub(super) fn render_cache(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let Some(view) = &self.cache else {
            return div()
                .p_2()
                .text_sm()
                .text_color(hsla(color::MUTED))
                .child("No cache view for this chip (the TC27x caches are known)")
                .into_any_element();
        };
        let measuring = view.measuring();
        let note = view.error().map(str::to_string);
        let rows = view.rows();

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
            .child(cell("Core".into(), 48., color::MUTED))
            .child(cell("TriCore".into(), 64., color::MUTED))
            .child(cell("Program".into(), 220., color::MUTED))
            .child(cell("Data".into(), 270., color::MUTED))
            .child(cell("Code cached".into(), 96., color::MUTED))
            .child(cell("Data cached".into(), 96., color::MUTED));
        let line = |row: Result<CacheRow, String>, index: usize| match row {
            Ok(row) => {
                let on = |text: &str| {
                    if text.contains(" on") {
                        color::TEXT
                    } else {
                        color::MUTED
                    }
                };
                div()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex()
                            .font_family("Consolas")
                            .text_sm()
                            .child(cell(format!("CPU{}", row.core), 48., color::TEXT))
                            .child(cell(row.kind.name().into(), 64., color::MUTED))
                            .child(cell(row.program.clone(), 220., on(&row.program)))
                            .child(cell(row.data.clone(), 270., on(&row.data)))
                            .child(cell(row.code_segments, 96., color::TEXT))
                            .child(cell(row.data_segments, 96., color::TEXT)),
                    )
                    .when_some(row.measured, |this, (program, data)| {
                        this.child(
                            div()
                                .pl(px(112.))
                                .text_xs()
                                .text_color(hsla(color::ACCENT))
                                .child(format!("{program}; {data}")),
                        )
                    })
            }
            Err(error) => div()
                .flex()
                .text_sm()
                .child(cell(format!("CPU{index}"), 48., color::TEXT))
                .child(cell(error, 600., color::MUTED)),
        };
        let table = div()
            .flex()
            .flex_col()
            .gap_0p5()
            .child(header)
            .when(rows.is_empty(), |this| {
                this.child(
                    div()
                        .text_xs()
                        .text_color(hsla(color::MUTED))
                        .child("read at the next refresh"),
                )
            })
            .children(rows.into_iter().enumerate().map(|(i, row)| line(row, i)));

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
                    .child(button(
                        "cache-measure",
                        if measuring { "Measuring…" } else { "Measure hits" },
                        !measuring,
                        cx.listener(|this, _, _, cx| {
                            this.measure_cache(cx);
                            cx.notify();
                        }),
                    ))
                    .child(
                        div()
                            .text_xs()
                            .text_color(hsla(color::MUTED))
                            .child(format!(
                                "PCON0/2, DCON0/2, PMA0/1 read every {INTERVAL_MS} ms; Measure counts hits and misses between reads with CCTRL, then puts CCTRL back"
                            )),
                    )
                    .when_some(note, |this, note| {
                        this.child(div().text_xs().text_color(hsla(color::CHANGED)).child(note))
                    }),
            )
            .child(
                div()
                    .id("cache-body")
                    .flex()
                    .flex_col()
                    .gap_2()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px_2()
                    .child(table)
                    .child(
                        div()
                            .text_xs()
                            .text_color(hsla(color::MUTED))
                            .child(
                                "The debugger reads and writes memory, not a core's cache. The lines in a cache are not shown: \
                                 their tags can be read only with the cache off and mapped (MTU_MEMMAP), and their format is not public.",
                            ),
                    ),
            )
            .into_any_element()
    }
}
