//! The OS tab: the OS objects an ORTI file describes (tasks, ISRs, …), with the values of their
//! attributes read every [`INTERVAL_MS`] through probe-rs `liveWatch` while the tab is shown, the
//! core running or halted (`OsTcb[1].state`, `p->state`: the server reads such paths).
//!
//! The ORTI file is chosen in the tab; its path is kept next to the attach file
//! (`<attach>.orti`, one line), so the next session of the same program shows it again. A file
//! `<attach>.orti` that is an ORTI file itself is read as such. Without an ORTI file, or with
//! one that cannot be read, the tab says so and debugging is as before.

use std::path::{Path, PathBuf};

use aurix_debug_model::live_watch::INTERVAL_MS;
use aurix_debug_model::orti::{Orti, OsView};
use aurix_debug_model::protocol::{LiveWatch as LiveWatchRequest, LiveWatchArguments};
use gpui::{AnyElement, Context, PathPromptOptions, SharedString, div, prelude::*, px};

use super::panels::BottomTab;
use super::{Debugger, button, color, hsla};

/// The file the ORTI file of `attach` is noted in.
pub fn link_file(attach: &str) -> PathBuf {
    Path::new(attach).with_extension("orti")
}

/// What `<attach>.orti` gives: the path of the ORTI file (its first line, when that is a file),
/// else the file itself.
fn linked_orti(link: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(link).ok()?;
    let first = text.lines().map(str::trim).find(|line| !line.is_empty())?;
    // A relative path is relative to the folder of the attach file.
    let named = link
        .parent()
        .map_or_else(|| PathBuf::from(first), |dir| dir.join(first));
    Some(if named.is_file() {
        named
    } else {
        link.to_path_buf()
    })
}

impl Debugger {
    /// The ORTI file noted for `attach`, if any.
    pub(super) fn load_os_for(&mut self, attach: &str) {
        self.os = None;
        self.os_file = None;
        self.os_note = None;
        if let Some(path) = linked_orti(&link_file(attach)) {
            self.load_orti(path);
        }
    }

    fn load_orti(&mut self, path: PathBuf) {
        self.os_busy = false;
        let result = std::fs::read_to_string(&path)
            .map_err(|error| error.to_string())
            .and_then(|text| Orti::parse(&text));
        match result {
            Ok(orti) => {
                let objects = orti.objects.len();
                let warnings = orti.warnings.len();
                self.log(format!(
                    "ORTI {}: {objects} object(s), {warnings} warning(s)",
                    path.display()
                ));
                for warning in &orti.warnings {
                    self.log(format!("ORTI: {warning}"));
                }
                self.os_note = None;
                self.os = Some(OsView::new(orti));
            }
            Err(error) => {
                // The path is shown next to the note already.
                self.log(format!("ORTI {}: {error}", path.display()));
                self.os = None;
                self.os_note = Some(error);
            }
        }
        self.os_file = Some(path);
    }

    fn open_orti_dialog(&mut self, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("ORTI file".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            this.update(cx, |this, cx| {
                if let Some(attach) = this.attach.clone() {
                    let link = link_file(&attach);
                    if link != path
                        && let Err(error) = std::fs::write(&link, format!("{}\n", path.display()))
                    {
                        this.log(format!("{}: {error}", link.display()));
                    }
                }
                this.load_orti(path);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Reads the OS attributes while the tab is shown (one read at a time).
    pub(super) fn read_os(&mut self, cx: &mut Context<Self>) {
        if self.bottom != BottomTab::Os || self.os_busy || self.client.is_none() {
            return;
        }
        let Some(os) = &self.os else {
            return;
        };
        let expressions = os.expressions();
        if expressions.is_empty() {
            return;
        }
        // The core of the selected thread, else the first (an ORTI file describes one OS).
        let core = self
            .selected_thread
            .or_else(|| self.threads.first().map(|t| t.id))
            .and_then(|id| u64::try_from(id).ok());
        self.os_busy = true;
        self.request::<LiveWatchRequest>(
            LiveWatchArguments { expressions, core },
            cx,
            |this, result, _| {
                this.os_busy = false;
                if let Some(os) = &mut this.os {
                    match result {
                        Ok(response) => os.answered(response.values),
                        Err(error) => os.failed(&error),
                    }
                }
            },
        );
    }

    pub(super) fn render_os(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let file = self
            .os_file
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "No ORTI file".to_string());
        let (note, note_colour) = match (&self.os_note, &self.os) {
            (Some(note), _) => (note.clone(), color::CHANGED),
            (None, Some(os)) => (
                match os.orti().warnings.len() {
                    0 => format!(
                        "ORTI {} · read every {INTERVAL_MS} ms",
                        os.orti().version.as_deref().unwrap_or("?")
                    ),
                    n => format!("{n} warning(s), see the console"),
                },
                color::MUTED,
            ),
            (None, None) => (
                "Choose the ORTI file (.ort, .orti) of the program".to_string(),
                color::MUTED,
            ),
        };
        let tables = self.os.as_ref().map(|os| os.tables()).unwrap_or_default();
        // The software trace of an object of the same name: its load and last run, two columns
        // more for a table with at least one such object.
        let trace = &self.sw_trace;
        let timing: Vec<Option<Vec<(String, String)>>> = tables
            .iter()
            .map(|table| {
                let cells: Vec<(String, String)> = table
                    .rows
                    .iter()
                    .map(|row| match trace.timing_of(&row.object) {
                        Some(t) => (
                            t.load
                                .map(|l| format!("{:.2} %", l * 100.0))
                                .unwrap_or_default(),
                            t.last_span.map(|s| trace.format(s)).unwrap_or_default(),
                        ),
                        None => Default::default(),
                    })
                    .collect();
                cells
                    .iter()
                    .any(|(load, last)| !load.is_empty() || !last.is_empty())
                    .then_some(cells)
            })
            .collect();
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
                        "orti-open",
                        "ORTI file…",
                        self.attach.is_some(),
                        cx.listener(|this, _, _, cx| this.open_orti_dialog(cx)),
                    ))
                    .child(div().text_xs().text_color(hsla(color::TEXT)).child(file))
                    .child(div().text_xs().text_color(hsla(note_colour)).child(note)),
            )
            .child(
                div()
                    .id("os-tables")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px_2()
                    .gap_2()
                    .children(tables.into_iter().zip(timing).enumerate().map(
                        |(t, (table, timing))| {
                            let traced = timing.is_some();
                            let mut timing = timing.unwrap_or_default().into_iter();
                            div()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(hsla(color::ACCENT))
                                        .child(format!("{} ({})", table.title, table.kind)),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .text_xs()
                                        .text_color(hsla(color::MUTED))
                                        .child(div().w(px(150.)).flex_none().child("Object"))
                                        .children(table.columns.iter().map(|column| {
                                            div()
                                                .w(px(170.))
                                                .flex_none()
                                                .overflow_hidden()
                                                .whitespace_nowrap()
                                                .child(column.clone())
                                        }))
                                        .when(traced, |this| {
                                            this.child(
                                                div().w(px(110.)).flex_none().child("Load (trace)"),
                                            )
                                            .child(
                                                div()
                                                    .w(px(110.))
                                                    .flex_none()
                                                    .child("Last run (trace)"),
                                            )
                                        }),
                                )
                                .children(table.rows.into_iter().enumerate().map(
                                    move |(r, row)| {
                                        let (load, last) = timing.next().unwrap_or_default();
                                        div()
                                            .id(SharedString::from(format!("os-{t}-{r}")))
                                            .flex()
                                            .font_family("Consolas")
                                            .text_sm()
                                            .hover(|style| style.bg(hsla(color::SELECTED)))
                                            .child(
                                                div()
                                                    .w(px(150.))
                                                    .flex_none()
                                                    .text_color(hsla(color::TEXT))
                                                    .child(row.object),
                                            )
                                            .children(row.cells.into_iter().map(|cell| {
                                                let colour = if cell.error {
                                                    color::MUTED
                                                } else if cell.changed {
                                                    color::CHANGED
                                                } else {
                                                    color::TEXT
                                                };
                                                div()
                                                    .w(px(170.))
                                                    .flex_none()
                                                    .overflow_hidden()
                                                    .whitespace_nowrap()
                                                    .text_color(hsla(colour))
                                                    .child(cell.text)
                                            }))
                                            .when(traced, |this| {
                                                this.child(
                                                    div()
                                                        .w(px(110.))
                                                        .flex_none()
                                                        .text_color(hsla(color::ACCENT))
                                                        .child(load),
                                                )
                                                .child(
                                                    div()
                                                        .w(px(110.))
                                                        .flex_none()
                                                        .text_color(hsla(color::ACCENT))
                                                        .child(last),
                                                )
                                            })
                                    },
                                ))
                        },
                    )),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_orti_link_sits_next_to_the_attach_file() {
        assert_eq!(
            link_file(r"C:\demo\tc275.json"),
            PathBuf::from(r"C:\demo\tc275.orti")
        );
        let dir = std::env::temp_dir().join(format!("aurix-os-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let orti = dir.join("demo.ort");
        std::fs::write(&orti, "VERSION { KOIL = \"2.2\"; };").unwrap();
        let link = dir.join("tc275.orti");
        std::fs::write(&link, format!("{}\n", orti.display())).unwrap();
        assert_eq!(linked_orti(&link), Some(orti.clone()));
        // An ORTI file under the link's name: read itself.
        std::fs::write(&link, "VERSION { KOIL = \"2.2\"; };").unwrap();
        assert_eq!(linked_orti(&link), Some(link.clone()));
        assert_eq!(linked_orti(&dir.join("missing.orti")), None);
        std::fs::remove_dir_all(&dir).ok();
    }
}
