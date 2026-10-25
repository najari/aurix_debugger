//! The SW Trace tab: the software trace a program writes with `aurix_trace.h` to an RTT up
//! channel in binary (`"dataFormat": "BinaryLE"` in the attach file), read while the cores run.
//! Per core and id: how many records, the interval between them, the spans from `BEGIN` to
//! `END`; the latest events, newest first; the records the target dropped. The ids are named
//! by `<attach>.trace` next to the attach file (`id = name` a line). Freeze keeps the view as
//! it is while the trace is read on; Clear forgets what was read.

use std::collections::HashMap;
use std::path::Path;

use aurix_debug_model::protocol::RttData;
use aurix_debug_model::sw_trace::{Kind, Names, SwTrace, binary_bytes};
use gpui::{AnyElement, Context, div, prelude::*, px};

use super::{Debugger, button, color, hsla};

/// How many events the list shows.
const SHOWN: usize = 300;

impl Debugger {
    /// The names of the trace ids for `attach`: `<attach>.trace`, if there is one.
    pub(super) fn load_sw_trace_names(&mut self, attach: &str) {
        let path = Path::new(attach).with_extension("trace");
        self.sw_trace_note = None;
        self.sw_trace_frozen = None;
        let names = match std::fs::read_to_string(&path) {
            Ok(text) => match Names::parse(&text) {
                Ok(names) => names,
                Err(error) => {
                    let note = format!("{}: {error}", path.display());
                    self.log(note.clone());
                    self.sw_trace_note = Some(note);
                    Names::default()
                }
            },
            Err(_) => Names::default(),
        };
        self.sw_trace_names = path.exists().then_some(path);
        self.sw_trace = SwTrace::new(names);
    }

    /// Data of a binary RTT channel.
    pub(super) fn sw_trace_received(&mut self, data: RttData) {
        match binary_bytes(&data.data) {
            Ok(bytes) => {
                self.sw_trace.received(data.core, &bytes);
            }
            Err(error) => {
                if self.sw_trace_note.is_none() {
                    self.log(format!("SW Trace: {error}"));
                }
                self.sw_trace_note = Some(error);
            }
        }
    }

    pub(super) fn render_sw_trace(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let frozen = self.sw_trace_frozen.is_some();
        let trace = self.sw_trace_frozen.as_ref().unwrap_or(&self.sw_trace);
        let several = self.threads.len() > 1;
        let core_name = |core: Option<u64>| match core {
            Some(core) => format!("CPU{core}"),
            None => "CPU".to_string(),
        };

        let dropped: Vec<String> = trace
            .dropped()
            .iter()
            .filter(|(_, n)| **n > 0)
            .map(|(core, n)| format!("{} dropped {n}", core_name(*core)))
            .collect();
        let summary = format!(
            "{} record(s){}{} · names: {}",
            trace.total(),
            if dropped.is_empty() {
                ", none dropped".to_string()
            } else {
                format!(", {}", dropped.join(", "))
            },
            if frozen { " · frozen" } else { "" },
            self.sw_trace_names
                .as_ref()
                .map_or("no <attach>.trace file".to_string(), |p| p
                    .display()
                    .to_string())
        );

        let cell = |text: String, width: f32, colour| {
            div()
                .w(px(width))
                .flex_none()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_color(hsla(colour))
                .child(text)
        };
        let spread = |s: &aurix_debug_model::sw_trace::Spread| {
            if s.count == 0 {
                String::new()
            } else {
                format!(
                    "{} / {} / {}",
                    trace.format(s.min),
                    trace.format(s.average().unwrap_or(0)),
                    trace.format(s.max)
                )
            }
        };

        // The statistics, a line per core and id.
        let stats_header = div()
            .flex()
            .text_xs()
            .text_color(hsla(color::MUTED))
            .when(several, |this| {
                this.child(cell("Core".into(), 44., color::MUTED))
            })
            .child(cell("Id".into(), 120., color::MUTED))
            .child(cell("Count".into(), 56., color::MUTED))
            .child(cell("Interval min / avg / max".into(), 260., color::MUTED))
            .child(cell("Span last".into(), 80., color::MUTED))
            .child(cell("Span min / avg / max".into(), 260., color::MUTED));
        let stats = trace.stats().into_iter().map(|row| {
            let s = row.stats;
            div()
                .flex()
                .font_family("Consolas")
                .text_sm()
                .when(several, |this| {
                    this.child(cell(core_name(row.core), 44., color::MUTED))
                })
                .child(cell(row.name, 120., color::TEXT))
                .child(cell(s.count.to_string(), 56., color::TEXT))
                .child(cell(spread(&s.interval), 260., color::TEXT))
                .child(cell(
                    if s.span.count > 0 {
                        trace.format(s.span.last)
                    } else {
                        String::new()
                    },
                    80.,
                    color::ACCENT,
                ))
                .child(cell(spread(&s.span), 260., color::TEXT))
        });

        // The latest events, newest first, with the time since the event before on its core.
        let start = trace.events().len().saturating_sub(SHOWN);
        let mut before: HashMap<Option<u64>, u64> = HashMap::new();
        let mut lines = Vec::with_capacity(SHOWN);
        for event in trace.events().iter().skip(start) {
            let delta = before
                .insert(event.core, event.time)
                .map(|t| event.time - t);
            lines.push((*event, delta));
        }
        let events_header = div()
            .flex()
            .text_xs()
            .text_color(hsla(color::MUTED))
            .child(cell("Time".into(), 96., color::MUTED))
            .when(several, |this| {
                this.child(cell("Core".into(), 44., color::MUTED))
            })
            .child(cell("Id".into(), 110., color::MUTED))
            .child(cell("".into(), 56., color::MUTED))
            .child(cell("Arg".into(), 52., color::MUTED))
            .child(cell("Span".into(), 80., color::MUTED))
            .child(cell("After".into(), 80., color::MUTED));
        let events = lines.into_iter().rev().map(|(event, delta)| {
            let kind = trace.kind(&event);
            let (word, colour) = match kind {
                Kind::Begin => ("begin", color::TEXT),
                Kind::End => ("end", color::MUTED),
                Kind::Event => ("event", color::TEXT),
                Kind::Dropped => ("dropped", color::CHANGED),
            };
            div()
                .flex()
                .font_family("Consolas")
                .text_xs()
                .child(cell(
                    trace.format(trace.since_start(event.time)),
                    96.,
                    color::MUTED,
                ))
                .when(several, |this| {
                    this.child(cell(core_name(event.core), 44., color::MUTED))
                })
                .child(cell(trace.names().name(event.id), 110., colour))
                .child(cell(word.into(), 56., colour))
                .child(cell(
                    if kind == Kind::End {
                        String::new()
                    } else {
                        event.arg.to_string()
                    },
                    52.,
                    colour,
                ))
                .child(cell(
                    event.span.map(|s| trace.format(s)).unwrap_or_default(),
                    80.,
                    color::ACCENT,
                ))
                .child(cell(
                    delta.map(|d| trace.format(d)).unwrap_or_default(),
                    80.,
                    color::MUTED,
                ))
        });

        let empty = trace.total() == 0;
        let note = self.sw_trace_note.clone();
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
                        "sw-trace-freeze",
                        if frozen { "Follow" } else { "Freeze" },
                        true,
                        cx.listener(|this, _, _, cx| {
                            this.sw_trace_frozen = match this.sw_trace_frozen {
                                Some(_) => None,
                                None => Some(this.sw_trace.clone()),
                            };
                            cx.notify();
                        }),
                    ))
                    .child(button(
                        "sw-trace-clear",
                        "Clear",
                        !empty,
                        cx.listener(|this, _, _, cx| {
                            this.sw_trace.clear();
                            this.sw_trace_frozen = None;
                            cx.notify();
                        }),
                    ))
                    .child(div().text_xs().text_color(hsla(color::MUTED)).child(summary))
                    .when_some(note, |this, note| {
                        this.child(div().text_xs().text_color(hsla(color::CHANGED)).child(note))
                    }),
            )
            .child(
                div()
                    .id("sw-trace-body")
                    .flex()
                    .flex_col()
                    .gap_2()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px_2()
                    .when(empty, |this| {
                        this.child(div().text_sm().text_color(hsla(color::MUTED)).child(
                            "No records: the program writes aurix_trace.h records to an RTT channel \
                             whose dataFormat is BinaryLE in the attach file (rttEnabled)",
                        ))
                    })
                    .when(!empty, |this| {
                        this.child(div().flex().flex_col().child(stats_header).children(stats))
                            .child(div().flex().flex_col().child(events_header).children(events))
                    }),
            )
            .into_any_element()
    }
}
