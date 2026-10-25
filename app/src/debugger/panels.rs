//! The bottom panel: the console, the RTT channels of every core and the Live Watch.
//!
//! The Live Watch reads its expressions on every core every [`INTERVAL_MS`], while the cores
//! run too (probe-rs reads the memory without halting them). Its expressions are kept in a file
//! next to the attach file (`<attach>.watch`, one per line), so that they stay for the next
//! session of the same program.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::Duration;

use aurix_debug_model::live_watch::INTERVAL_MS;
use aurix_debug_model::protocol::LiveWatch as LiveWatchRequest;
use aurix_debug_model::rtt::ChannelKey;
use gpui::{AnyElement, Context, ScrollStrategy, SharedString, div, prelude::*, px, uniform_list};

use super::{Debugger, button, color, hsla};

/// The panel's height.
const HEIGHT: f32 = 210.;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BottomTab {
    Console,
    Rtt(ChannelKey),
    LiveWatch,
    Memory,
}

/// The file the Live Watch expressions of `attach` are kept in.
pub fn watch_file(attach: &str) -> PathBuf {
    Path::new(attach).with_extension("watch")
}

/// The expressions in `path`, none if it is missing. Empty lines and `#` comments are skipped.
pub fn read_watch_file(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .map(|text| parse_watch_file(&text))
        .unwrap_or_default()
}

fn parse_watch_file(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(String::from)
        .collect()
}

impl Debugger {
    /// Adds an expression to the Live Watch and shows it.
    pub(super) fn add_watch(&mut self, expression: &str) {
        if self.live_watch.add(expression) {
            self.save_watch();
        }
        self.bottom = BottomTab::LiveWatch;
    }

    fn remove_watch(&mut self, expression: &str) {
        self.live_watch.remove(expression);
        self.save_watch();
    }

    fn save_watch(&mut self) {
        let Some(path) = self.watch_file.clone() else {
            return;
        };
        let text = self
            .live_watch
            .expressions()
            .iter()
            .map(|expression| format!("{expression}\n"))
            .collect::<String>();
        if let Err(error) = std::fs::write(&path, text) {
            self.log(format!("{}: {error}", path.display()));
        }
    }

    /// Reads the Live Watch expressions on every core while the session lasts.
    pub(super) fn poll_live_watch(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(INTERVAL_MS))
                    .await;
                let alive = this
                    .update(cx, |this, cx| {
                        if this.client.is_none() {
                            return false;
                        }
                        for request in this.live_watch.take_requests() {
                            let core = request.core;
                            this.request::<LiveWatchRequest>(
                                request,
                                cx,
                                move |this, result, _| {
                                    this.live_watch
                                        .answered(core, result.map(|response| response.values));
                                },
                            );
                        }
                        this.read_memory(cx);
                        true
                    })
                    .unwrap_or(false);
                if !alive {
                    break;
                }
            }
        })
        .detach();
    }

    pub(super) fn scroll_rtt_to_end(&mut self) {
        if let BottomTab::Rtt(key) = self.bottom
            && let Some(channel) = self.rtt.get(key)
        {
            self.rtt_scroll.scroll_to_item(
                channel.lines.len().saturating_sub(1),
                ScrollStrategy::Bottom,
            );
        }
    }

    fn show_tab(&mut self, tab: BottomTab) {
        self.bottom = tab;
        match tab {
            BottomTab::Console => self
                .console_scroll
                .scroll_to_item(self.console.len().saturating_sub(1), ScrollStrategy::Bottom),
            BottomTab::Rtt(_) => self.scroll_rtt_to_end(),
            BottomTab::LiveWatch | BottomTab::Memory => {}
        }
    }

    pub(super) fn render_bottom(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut tabs = vec![(BottomTab::Console, "Console".to_string())];
        tabs.extend(self.rtt.channels().iter().map(|channel| {
            (
                BottomTab::Rtt(channel.key()),
                format!("RTT {}", channel.label()),
            )
        }));
        tabs.push((
            BottomTab::LiveWatch,
            format!("Live Watch ({})", self.live_watch.expressions().len()),
        ));
        tabs.push((
            BottomTab::Memory,
            match self.memory.start() {
                Some(start) => format!("Memory {start:08X}"),
                None => "Memory".to_string(),
            },
        ));
        let body = match self.bottom {
            BottomTab::Console => self.render_console(cx).into_any_element(),
            BottomTab::Rtt(key) => self.render_rtt(key, cx).into_any_element(),
            BottomTab::LiveWatch => self.render_live_watch(cx),
            BottomTab::Memory => self.render_memory(cx),
        };
        div()
            .flex()
            .flex_col()
            .h(px(HEIGHT))
            .flex_none()
            .border_t_1()
            .border_color(hsla(color::BORDER))
            .bg(hsla(color::PANEL))
            .child(
                div()
                    .flex()
                    .border_b_1()
                    .border_color(hsla(color::BORDER))
                    .children(tabs.into_iter().enumerate().map(|(ix, (tab, label))| {
                        let selected = tab == self.bottom;
                        div()
                            .id(SharedString::from(format!("tab-{ix}")))
                            .px_3()
                            .py_1()
                            .text_xs()
                            .cursor_pointer()
                            .text_color(hsla(if selected { color::TEXT } else { color::MUTED }))
                            .when(selected, |this| {
                                this.bg(hsla(color::BACKGROUND))
                                    .border_b_1()
                                    .border_color(hsla(color::ACCENT))
                            })
                            .hover(|style| style.text_color(hsla(color::TEXT)))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.show_tab(tab);
                                cx.notify();
                            }))
                            .child(label)
                    })),
            )
            .child(div().flex().flex_col().flex_1().min_h_0().child(body))
    }

    fn render_console(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        uniform_list(
            "console",
            self.console.len(),
            cx.processor(|this, range: Range<usize>, _window, _cx| {
                range
                    .filter_map(|ix| Some(text_line(this.console.get(ix)?.clone())))
                    .collect()
            }),
        )
        .track_scroll(&self.console_scroll)
        .flex_1()
    }

    fn render_rtt(&mut self, key: ChannelKey, cx: &mut Context<Self>) -> impl IntoElement {
        let (count, dropped) = self
            .rtt
            .get(key)
            .map(|channel| (channel.lines.len(), channel.dropped))
            .unwrap_or_default();
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
                    .child(button(
                        "rtt-clear",
                        "Clear",
                        count > 0,
                        cx.listener(move |this, _, _, cx| {
                            this.rtt.clear(key);
                            cx.notify();
                        }),
                    ))
                    .child(div().px_2().text_xs().text_color(hsla(color::MUTED)).child(
                        match dropped {
                            0 => format!("{count} lines"),
                            // Cleared, or beyond what a channel keeps.
                            dropped => format!("{count} lines ({dropped} earlier not shown)"),
                        },
                    )),
            )
            .child(
                uniform_list(
                    "rtt",
                    count,
                    cx.processor(move |this, range: Range<usize>, _window, _cx| {
                        let Some(channel) = this.rtt.get(key) else {
                            return Vec::new();
                        };
                        range
                            .filter_map(|ix| {
                                Some(text_line(SharedString::from(
                                    channel.lines.get(ix)?.clone(),
                                )))
                            })
                            .collect()
                    }),
                )
                .track_scroll(&self.rtt_scroll)
                .flex_1(),
            )
    }

    fn render_live_watch(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let rows = self
            .live_watch
            .expressions()
            .to_vec()
            .into_iter()
            .enumerate()
            .map(|(ix, expression)| self.render_watch_row(ix, expression, cx))
            .collect::<Vec<_>>();
        let paused = self.live_watch.paused();
        let targets = self.live_watch.targets().to_vec();
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
                    .child(div().w(px(360.)).child(self.watch_input.clone()))
                    .child(button(
                        "watch-pause",
                        if paused { "Resume" } else { "Pause reading" },
                        true,
                        cx.listener(move |this, _, _, cx| {
                            this.live_watch.set_paused(!paused);
                            cx.notify();
                        }),
                    ))
                    .child(
                        div()
                            .px_2()
                            .text_xs()
                            .text_color(hsla(color::MUTED))
                            .child(format!("read every {INTERVAL_MS} ms on every core")),
                    ),
            )
            .child(
                // Header: one column per core.
                div()
                    .flex()
                    .px_2()
                    .text_xs()
                    .text_color(hsla(color::MUTED))
                    .child(div().w(px(260.)).flex_none().child("Expression"))
                    .children(targets.iter().map(|target| {
                        let label = if target.label.is_empty() {
                            "Value".to_string()
                        } else {
                            target.label.clone()
                        };
                        div().w(px(200.)).flex_none().child(match &target.error {
                            Some(error) => format!("{label}: {error}"),
                            None => label,
                        })
                    })),
            )
            .child(
                div()
                    .id("watch-rows")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .children(rows),
            )
            .into_any_element()
    }

    fn render_watch_row(
        &self,
        ix: usize,
        expression: String,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let targets = self.live_watch.targets();
        let remove = expression.clone();
        div()
            .flex()
            .items_center()
            .px_2()
            .font_family("Consolas")
            .text_sm()
            .hover(|style| style.bg(hsla(color::SELECTED)))
            .child(
                div()
                    .w(px(260.))
                    .flex_none()
                    .flex()
                    .gap_2()
                    .child(
                        div()
                            .id(SharedString::from(format!("watch-remove-{ix}")))
                            .text_color(hsla(color::MUTED))
                            .cursor_pointer()
                            .hover(|style| style.text_color(hsla(color::BREAKPOINT)))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.remove_watch(&remove);
                                cx.notify();
                            }))
                            .child("×"),
                    )
                    .child(expression.clone()),
            )
            .children(targets.iter().map(|target| {
                let (text, colour) = match target.values.get(&expression) {
                    Some(value) => match (&value.value, &value.error) {
                        (Some(value), _) if target.changed.contains(&expression) => {
                            (value.clone(), color::CHANGED)
                        }
                        (Some(value), _) => (value.clone(), color::TEXT),
                        (None, Some(error)) => (error.clone(), color::MUTED),
                        (None, None) => (String::new(), color::MUTED),
                    },
                    None => ("…".to_string(), color::MUTED),
                };
                div()
                    .w(px(200.))
                    .flex_none()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_color(hsla(colour))
                    .child(text)
            }))
            .into_any_element()
    }
}

fn text_line(line: SharedString) -> AnyElement {
    div()
        .px_2()
        .font_family("Consolas")
        .text_xs()
        .text_color(hsla(color::TEXT))
        .whitespace_nowrap()
        .child(line)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_watch_file_sits_next_to_the_attach_file() {
        assert_eq!(
            watch_file(r"C:\demo\tc275.json"),
            PathBuf::from(r"C:\demo\tc275.watch")
        );
        assert_eq!(
            parse_watch_file("g_ticks\r\n\n# a comment\n  g_result  \n"),
            ["g_ticks", "g_result"]
        );
    }
}
