//! Breakpoints on instructions. `setInstructionBreakpoints` replaces the adapter's set, so every
//! change sends all of them; the answer tells, per breakpoint, whether the adapter set it.
//!
//! [`BreakpointSettings`] is what makes any breakpoint conditional.

use std::collections::BTreeMap;

use dap_types::{Breakpoint, InstructionBreakpoint, SetInstructionBreakpointsArguments};

/// The condition, hit count and log message of a breakpoint (DAP `condition`, `hitCondition`,
/// `logMessage`). probe-rs checks them at each halt there and runs the core on when they are not
/// met: a condition is `<name> <op> <value>` joined with `&&` and `||`, a hit count `N`, `>= N`,
/// `> N` or `% N`, a log message text with `{name}` in it (the core does not stop).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BreakpointSettings {
    pub condition: Option<String>,
    pub hit_condition: Option<String>,
    pub log_message: Option<String>,
}

impl BreakpointSettings {
    /// From the text of edit fields; a blank field is none.
    pub fn new(condition: &str, hit_condition: &str, log_message: &str) -> Self {
        let given = |text: &str| {
            let text = text.trim();
            (!text.is_empty()).then(|| text.to_string())
        };
        Self {
            condition: given(condition),
            hit_condition: given(hit_condition),
            log_message: given(log_message),
        }
    }

    /// Whether the breakpoint stops every time (none of the three).
    pub fn is_plain(&self) -> bool {
        self.condition.is_none() && self.hit_condition.is_none() && self.log_message.is_none()
    }

    /// Whether the breakpoint only logs.
    pub fn logs(&self) -> bool {
        self.log_message.is_some()
    }

    /// A short line for a tooltip or list: `if v == 3 · hit % 4 · log v = {v}`.
    pub fn summary(&self) -> String {
        [
            self.condition.as_ref().map(|c| format!("if {c}")),
            self.hit_condition.as_ref().map(|h| format!("hit {h}")),
            self.log_message.as_ref().map(|l| format!("log {l}")),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ")
    }
}

/// What the adapter answered for one breakpoint.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InstructionBreakpointState {
    /// Whether the adapter set the breakpoint (`None` until it answered).
    pub verified: Option<bool>,
    pub message: Option<String>,
    /// The condition and hit count sent with it (a log message is for source breakpoints).
    pub settings: BreakpointSettings,
}

/// The instruction breakpoints, keyed by their instruction reference (the address as the
/// adapter wrote it, with its `@core` suffix if any).
#[derive(Clone, Debug, Default)]
pub struct InstructionBreakpoints {
    breakpoints: BTreeMap<String, InstructionBreakpointState>,
}

impl InstructionBreakpoints {
    /// Sets a breakpoint at `reference`, or removes the one there. Returns whether one is set
    /// now.
    pub fn toggle(&mut self, reference: impl Into<String>) -> bool {
        let reference = reference.into();
        if self.breakpoints.remove(&reference).is_some() {
            false
        } else {
            self.breakpoints
                .insert(reference, InstructionBreakpointState::default());
            true
        }
    }

    pub fn get(&self, reference: &str) -> Option<&InstructionBreakpointState> {
        self.breakpoints.get(reference)
    }

    /// Sets a breakpoint at `reference` with `settings` (or gives the one there new ones).
    pub fn set(&mut self, reference: impl Into<String>, settings: BreakpointSettings) {
        self.breakpoints.insert(
            reference.into(),
            InstructionBreakpointState {
                settings,
                ..Default::default()
            },
        );
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &InstructionBreakpointState)> {
        self.breakpoints.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.breakpoints.is_empty()
    }

    /// The request that sets all of them; [`Self::answered`] takes its answer.
    pub fn request(&self) -> SetInstructionBreakpointsArguments {
        SetInstructionBreakpointsArguments {
            breakpoints: self
                .breakpoints
                .iter()
                .map(|(reference, state)| InstructionBreakpoint {
                    instruction_reference: reference.clone(),
                    offset: None,
                    condition: state.settings.condition.clone(),
                    hit_condition: state.settings.hit_condition.clone(),
                    mode: None,
                })
                .collect(),
        }
    }

    /// The adapter's answer to `request` (made by [`Self::request`]): one breakpoint per
    /// breakpoint of the request, in its order. Breakpoints removed since are skipped.
    pub fn answered(
        &mut self,
        request: &SetInstructionBreakpointsArguments,
        answer: Result<&[Breakpoint], &str>,
    ) {
        match answer {
            Ok(breakpoints) => {
                for (asked, breakpoint) in request.breakpoints.iter().zip(breakpoints) {
                    if let Some(state) = self.breakpoints.get_mut(&asked.instruction_reference) {
                        state.verified = Some(breakpoint.verified);
                        state.message = breakpoint.message.clone();
                    }
                }
            }
            Err(error) => {
                for asked in &request.breakpoints {
                    if let Some(state) = self.breakpoints.get_mut(&asked.instruction_reference) {
                        state.verified = Some(false);
                        state.message = Some(error.to_string());
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_answer_goes_to_the_breakpoints_asked_for() {
        let mut breakpoints = InstructionBreakpoints::default();
        assert!(breakpoints.toggle("0x8021C030"));
        assert!(breakpoints.toggle("0x8021C02C"));
        let request = breakpoints.request();
        assert_eq!(request.breakpoints.len(), 2);
        // Removed before the answer came.
        assert!(!breakpoints.toggle("0x8021C02C"));
        let answer: Vec<Breakpoint> = serde_json::from_value(json!([
            { "verified": false, "message": "no" },
            { "verified": true, "message": "Set on cores 0, 1, 2" }
        ]))
        .unwrap();
        breakpoints.answered(&request, Ok(&answer));
        assert_eq!(breakpoints.get("0x8021C02C"), None);
        let state = breakpoints.get("0x8021C030").unwrap();
        assert_eq!(state.verified, Some(true));
        assert_eq!(state.message.as_deref(), Some("Set on cores 0, 1, 2"));

        breakpoints.answered(&breakpoints.request(), Err("probe gone"));
        assert_eq!(breakpoints.get("0x8021C030").unwrap().verified, Some(false));
    }

    #[test]
    fn settings_go_with_the_request_and_stay_after_the_answer() {
        let mut breakpoints = InstructionBreakpoints::default();
        let settings = BreakpointSettings::new(" v == 3 ", "% 4", "");
        assert_eq!(settings.log_message, None);
        assert_eq!(settings.summary(), "if v == 3 · hit % 4");
        breakpoints.set("0x8021C028", settings.clone());
        let request = breakpoints.request();
        assert_eq!(request.breakpoints[0].condition.as_deref(), Some("v == 3"));
        assert_eq!(request.breakpoints[0].hit_condition.as_deref(), Some("% 4"));
        let answer: Vec<Breakpoint> =
            serde_json::from_value(json!([{ "verified": true }])).unwrap();
        breakpoints.answered(&request, Ok(&answer));
        assert_eq!(breakpoints.get("0x8021C028").unwrap().settings, settings);
        assert!(BreakpointSettings::new("", " ", "").is_plain());
    }
}
