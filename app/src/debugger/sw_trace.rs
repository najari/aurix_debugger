//! The SW Trace tab: the software trace a program writes with `aurix_trace.h` to an RTT up
//! channel in binary (`"dataFormat": "BinaryLE"` in the attach file), read while the cores run.
//! Per core and id: how many records, the interval between them, the spans from `BEGIN` to
//! `END`; the latest events, newest first; the records the target dropped. The ids are named
//! by `<attach>.trace` next to the attach file (`id = name` a line). Freeze keeps the view as
//! it is while the trace is read on; Clear forgets what was read.

use std::ops::Range;
use std::path::Path;

use aurix_debug_model::protocol::RttData;
use aurix_debug_model::sw_trace::{Kind, Names, SwTrace, binary_bytes};
use gpui::{AnyElement, Context, div, prelude::*, px, uniform_list};

use super::{Debugger, button, color, hsla};

/// How far back a row looks for the event before on its core.
const LOOK_BACK: usize = 500;

fn cell(text: String, width: f32, colour: u32) -> gpui::Div {
    div()
        .w(px(width))
        .flex_none()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_color(hsla(colour))
        .child(text)
}

/// Row `ix` of the event list (0 the newest): time, core, id, kind, arg, span, and the time
/// since the event before on the same core.
fn event_row(trace: &SwTrace, ix: usize, several: bool) -> AnyElement {
    let events = trace.events();
    let Some(at) = events.len().checked_sub(ix + 1) else {
        return div().into_any_element();
    };
    let event = events[at];
    let delta = (at.saturating_sub(LOOK_BACK)..at)
        .rev()
        .find(|i| events[*i].core == event.core)
        .map(|i| event.time - events[i].time);
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
            this.child(cell(
                event.core.map_or("CPU".into(), |c| format!("CPU{c}")),
                44.,
                color::MUTED,
            ))
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
        .into_any_element()
}
/// The chart's windows: name and nanoseconds.
const WINDOWS: [(&str, u64); 4] = [
    ("10 ms", 10_000_000),
    ("100 ms", 100_000_000),
    ("1 s", 1_000_000_000),
    ("5 s", 5_000_000_000),
];
const WINDOW_IDS: [&str; 4] = [
    "sw-trace-10ms",
    "sw-trace-100ms",
    "sw-trace-1s",
    "sw-trace-5s",
];
/// The chart's plot width in pixels: a column a pixel.
const CHART_WIDTH: u32 = 760;

/// The chart of `trace`'s last `window` nanoseconds: a row per core and id, spans as bars,
/// single records as ticks, the time under it.
fn render_chart(trace: &SwTrace, window_ns: u64, several: bool) -> AnyElement {
    let ticks = (window_ns as u128 * trace.tick_hz() as u128 / 1_000_000_000) as u64;
    let Some(chart) = trace.chart(ticks, CHART_WIDTH) else {
        return div().into_any_element();
    };
    let label = |row: &aurix_debug_model::sw_trace::ChartRow| match (several, row.core) {
        (true, Some(core)) => format!("CPU{core} {}", row.name),
        _ => row.name.clone(),
    };
    let rows = chart.rows.iter().map(|row| {
        let mut plot = div()
            .relative()
            .w(px(CHART_WIDTH as f32))
            .h(px(14.))
            .flex_none()
            .border_b_1()
            .border_color(hsla(color::BORDER));
        for (a, b) in &row.segments {
            plot = plot.child(
                div()
                    .absolute()
                    .left(px(*a as f32))
                    .top(px(2.))
                    .w(px((b - a) as f32))
                    .h(px(10.))
                    .bg(hsla(color::ACCENT)),
            );
        }
        for c in &row.marks {
            plot = plot.child(
                div()
                    .absolute()
                    .left(px(*c as f32))
                    .top(px(0.))
                    .w(px(1.))
                    .h(px(14.))
                    .bg(hsla(color::TEXT)),
            );
        }
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .w(px(150.))
                    .flex_none()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_xs()
                    .text_color(hsla(color::TEXT))
                    .child(label(row)),
            )
            .child(plot)
            .child(
                div()
                    .text_xs()
                    .text_color(hsla(color::MUTED))
                    .child(if row.spans > 0 {
                        format!("{} span(s)", row.spans)
                    } else {
                        format!("{} record(s)", row.marks.len())
                    }),
            )
    });
    // Five times under the plot, from the start of the window to its end.
    let mut axis = div()
        .relative()
        .w(px(CHART_WIDTH as f32))
        .h(px(14.))
        .flex_none();
    for i in 0..=4u32 {
        let x = CHART_WIDTH * i / 4;
        let time = chart.start + (chart.end - chart.start) * u64::from(i) / 4;
        axis = axis.child(
            div()
                .absolute()
                .left(px((x as f32 - if i == 4 { 60. } else { 0. }).max(0.)))
                .text_xs()
                .text_color(hsla(color::MUTED))
                .child(trace.format(time)),
        );
    }
    div()
        .flex()
        .flex_col()
        .gap_0p5()
        .children(rows)
        .child(
            div()
                .flex()
                .gap_2()
                .child(div().w(px(150.)).flex_none())
                .child(axis),
        )
        .into_any_element()
}

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

        // Every event kept, newest first, drawn only where the list shows (uniform_list).
        let count = trace.events().len();
        let events = uniform_list(
            "sw-trace-events",
            count,
            cx.processor(move |this, range: Range<usize>, _window, _cx| {
                let trace = this.sw_trace_frozen.as_ref().unwrap_or(&this.sw_trace);
                range.map(|ix| event_row(trace, ix, several)).collect()
            }),
        )
        .flex_1();
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

        let empty = trace.total() == 0;
        let note = self.sw_trace_note.clone();
        let window = self.sw_trace_window;
        let chart = window
            .filter(|_| !empty)
            .map(|i| render_chart(trace, WINDOWS[i].1, several));
        let mut views = div().flex().items_center().gap_1().child(button(
            "sw-trace-list",
            "List",
            window.is_some(),
            cx.listener(|this, _, _, cx| {
                this.sw_trace_window = None;
                cx.notify();
            }),
        ));
        for (i, (name, _)) in WINDOWS.iter().enumerate() {
            views = views.child(button(
                WINDOW_IDS[i],
                name,
                window != Some(i),
                cx.listener(move |this, _, _, cx| {
                    this.sw_trace_window = Some(i);
                    cx.notify();
                }),
            ));
        }
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
                    .child(views)
                    .child(
                        div()
                            .text_xs()
                            .text_color(hsla(color::MUTED))
                            .child(summary),
                    )
                    .when_some(note, |this, note| {
                        this.child(div().text_xs().text_color(hsla(color::CHANGED)).child(note))
                    }),
            )
            .when(empty, |this| {
                this.child(div().px_2().text_sm().text_color(hsla(color::MUTED)).child(
                    "No records: the program writes aurix_trace.h records to an RTT channel \
                     whose dataFormat is BinaryLE in the attach file (rttEnabled)",
                ))
            })
            .when_some(chart, |this, chart| {
                this.child(
                    div()
                        .id("sw-trace-chart")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .px_2()
                        .child(chart),
                )
            })
            // The list: the statistics, then the events filling the rest of the panel.
            .when(!empty && window.is_none(), |this| {
                this.child(
                    div()
                        .flex()
                        .flex_col()
                        .px_2()
                        .pb_1()
                        .child(stats_header)
                        .children(stats),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_h_0()
                        .px_2()
                        .child(events_header)
                        .child(events),
                )
            })
            .into_any_element()
    }
}
