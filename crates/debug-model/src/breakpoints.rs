//! Breakpoints on instructions. `setInstructionBreakpoints` replaces the adapter's set, so every
//! change sends all of them; the answer tells, per breakpoint, whether the adapter set it.

use std::collections::BTreeMap;

use dap_types::{Breakpoint, InstructionBreakpoint, SetInstructionBreakpointsArguments};

/// What the adapter answered for one breakpoint.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InstructionBreakpointState {
    /// Whether the adapter set the breakpoint (`None` until it answered).
    pub verified: Option<bool>,
    pub message: Option<String>,
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
                .keys()
                .map(|reference| InstructionBreakpoint {
                    instruction_reference: reference.clone(),
                    offset: None,
                    condition: None,
                    hit_condition: None,
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
}
