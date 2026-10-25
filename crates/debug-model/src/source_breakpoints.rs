//! Breakpoints on source lines. `setBreakpoints` replaces the adapter's set for one source file,
//! so every change sends all the lines of that file; the answer tells, per line, whether the
//! adapter set it (and where: it may move a breakpoint to the next line with code).

use std::collections::BTreeMap;

use dap_types::{Breakpoint, SetBreakpointsArguments, Source, SourceBreakpoint};

use crate::reference::parse_address;

/// What the adapter answered for one breakpoint.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SourceBreakpointState {
    /// Whether the adapter set the breakpoint (`None` until it answered).
    pub verified: Option<bool>,
    /// The line the adapter set it on, if it said.
    pub line: Option<u64>,
    /// The address it set it at, if it said.
    pub instruction_reference: Option<String>,
    pub message: Option<String>,
}

/// The source breakpoints, by file path (as the adapter gave it in stack frames) and line.
#[derive(Clone, Debug, Default)]
pub struct SourceBreakpoints {
    files: BTreeMap<String, BTreeMap<u64, SourceBreakpointState>>,
}

impl SourceBreakpoints {
    /// Sets a breakpoint on `line` of `path`, or removes the one there. Returns whether one is
    /// set now.
    pub fn toggle(&mut self, path: &str, line: u64) -> bool {
        let lines = self.files.entry(path.to_string()).or_default();
        if lines.remove(&line).is_some() {
            if lines.is_empty() {
                // The file is kept out of `files` but its (now empty) request still goes out.
                self.files.remove(path);
            }
            false
        } else {
            lines.insert(line, SourceBreakpointState::default());
            true
        }
    }

    pub fn get(&self, path: &str, line: u64) -> Option<&SourceBreakpointState> {
        self.files.get(path)?.get(&line)
    }

    /// The breakpoints of `path`, by line.
    pub fn lines(&self, path: &str) -> impl Iterator<Item = (u64, &SourceBreakpointState)> {
        self.files
            .get(path)
            .into_iter()
            .flatten()
            .map(|(line, state)| (*line, state))
    }

    /// The set breakpoint the adapter placed at `address`, in any file.
    pub fn at_address(&self, address: u64) -> Option<&SourceBreakpointState> {
        self.files
            .values()
            .flat_map(BTreeMap::values)
            .find(|state| {
                state.verified == Some(true)
                    && state
                        .instruction_reference
                        .as_deref()
                        .and_then(parse_address)
                        == Some(address)
            })
    }

    /// The files with breakpoints.
    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.files.keys().map(String::as_str)
    }

    /// The request that sets all breakpoints of `path` (none: it clears them);
    /// [`Self::answered`] takes its answer.
    pub fn request(&self, path: &str) -> SetBreakpointsArguments {
        let lines = self.lines(path).map(|(line, _)| line).collect::<Vec<_>>();
        SetBreakpointsArguments {
            source: source(path),
            breakpoints: Some(
                lines
                    .iter()
                    .map(|line| SourceBreakpoint {
                        line: *line,
                        column: None,
                        condition: None,
                        hit_condition: None,
                        log_message: None,
                        mode: None,
                    })
                    .collect(),
            ),
            lines: Some(lines),
            source_modified: Some(false),
        }
    }

    /// The adapter's answer to `request` (made by [`Self::request`]): one breakpoint per line of
    /// the request, in its order. Lines removed since are skipped.
    pub fn answered(
        &mut self,
        request: &SetBreakpointsArguments,
        answer: Result<&[Breakpoint], &str>,
    ) {
        let Some(lines) = request
            .source
            .path
            .as_deref()
            .and_then(|path| self.files.get_mut(path))
        else {
            return;
        };
        let asked = request.breakpoints.iter().flatten().map(|asked| asked.line);
        match answer {
            Ok(breakpoints) => {
                for (line, breakpoint) in asked.zip(breakpoints) {
                    if let Some(state) = lines.get_mut(&line) {
                        *state = SourceBreakpointState {
                            verified: Some(breakpoint.verified),
                            line: breakpoint.line,
                            instruction_reference: breakpoint.instruction_reference.clone(),
                            message: breakpoint.message.clone(),
                        };
                    }
                }
            }
            Err(error) => {
                for line in asked {
                    if let Some(state) = lines.get_mut(&line) {
                        *state = SourceBreakpointState {
                            verified: Some(false),
                            message: Some(error.to_string()),
                            ..Default::default()
                        };
                    }
                }
            }
        }
    }
}

/// The source named by `path`, with its file name as the name.
fn source(path: &str) -> Source {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path).to_string();
    Source {
        name: Some(name),
        path: Some(path.to_string()),
        source_reference: None,
        presentation_hint: None,
        origin: None,
        sources: None,
        adapter_data: None,
        checksums: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const DEMO: &str = r"C:\work\aurix_tcf_demo\src\demo.c";

    #[test]
    fn a_file_sends_all_its_lines_and_the_answer_goes_to_them() {
        let mut breakpoints = SourceBreakpoints::default();
        assert!(breakpoints.toggle(DEMO, 23));
        assert!(breakpoints.toggle(DEMO, 21));
        let request = breakpoints.request(DEMO);
        assert_eq!(request.source.name.as_deref(), Some("demo.c"));
        assert_eq!(request.lines, Some(vec![21, 23]));
        assert_eq!(request.breakpoints.as_ref().unwrap().len(), 2);

        let answer: Vec<Breakpoint> = serde_json::from_value(json!([
            { "verified": true, "line": 22, "instructionReference": "0x8021C01C" },
            { "verified": true, "line": 23, "instructionReference": "0x8021C028",
              "message": "Source breakpoint at memory address: 0x8021C028 (cores 0, 1, 2)" }
        ]))
        .unwrap();
        breakpoints.answered(&request, Ok(&answer));
        // Line 21 has no code: the adapter moved it to line 22.
        let moved = breakpoints.get(DEMO, 21).unwrap();
        assert_eq!((moved.verified, moved.line), (Some(true), Some(22)));
        let state = breakpoints.get(DEMO, 23).unwrap();
        assert_eq!(state.instruction_reference.as_deref(), Some("0x8021C028"));
        // Shown in the disassembly too.
        assert!(breakpoints.at_address(0x8021_c028).is_some());
        assert!(breakpoints.at_address(0x8021_c02c).is_none());
    }

    #[test]
    fn removing_the_last_line_clears_the_file() {
        let mut breakpoints = SourceBreakpoints::default();
        breakpoints.toggle(DEMO, 23);
        let first = breakpoints.request(DEMO);
        assert!(!breakpoints.toggle(DEMO, 23));
        assert_eq!(breakpoints.paths().count(), 0);
        let clear = breakpoints.request(DEMO);
        assert_eq!(clear.lines, Some(vec![]));
        assert_eq!(clear.source.path.as_deref(), Some(DEMO));
        // The answer to the first request, late: nothing to update.
        breakpoints.answered(&first, Err("probe gone"));
        assert_eq!(breakpoints.get(DEMO, 23), None);

        breakpoints.toggle(DEMO, 30);
        breakpoints.answered(&breakpoints.request(DEMO), Err("probe gone"));
        let state = breakpoints.get(DEMO, 30).unwrap();
        assert_eq!(state.verified, Some(false));
        assert_eq!(state.message.as_deref(), Some("probe gone"));
    }
}
