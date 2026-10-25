//! The local variables of every frame of a call stack, for the call stack view (TRACE32's
//! `Frame.view /Locals`): per frame the variables of its Locals scope, the first level only
//! (a struct shows as the adapter shows its value). The adapter unwinds each frame's
//! registers, so an outer frame's locals are its own, as they were at the call.
//!
//! A stop asks for the scopes of each frame ([`FrameLocals::set_frames`]), then for the
//! variables of its Locals scope ([`FrameLocals::scopes`]). A value that differs from the one
//! the same local of the same function had at the stop before is marked.

use std::collections::HashMap;

use dap_types::{Scope, ScopePresentationHint, ScopesArguments, Variable, VariablesArguments};

/// How many frames have their locals read.
pub const MAX_FRAMES: usize = 16;

/// One local of a frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Local {
    pub name: String,
    pub value: String,
    /// The value differs from the stop before's (same function, same name).
    pub changed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum State {
    /// The scopes are asked for.
    Scopes,
    /// The Locals scope's variables are asked for.
    Variables(u64),
    Done(Vec<Local>),
    Failed(String),
}

#[derive(Clone, Debug)]
struct Entry {
    id: u64,
    function: String,
    state: State,
}

/// What the locals of a frame are, for the view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shown<'a> {
    Loading,
    Locals(&'a [Local]),
    Failed(&'a str),
}

#[derive(Clone, Debug, Default)]
pub struct FrameLocals {
    frames: Vec<Entry>,
    /// The values of the stop before, by function and name.
    previous: HashMap<(String, String), String>,
}

impl FrameLocals {
    /// A new stop with these frames (id and function, innermost first): the requests for their
    /// scopes, at most [`MAX_FRAMES`]. The values read so far become the previous ones.
    pub fn set_frames(&mut self, frames: &[(u64, String)]) -> Vec<ScopesArguments> {
        self.keep_values();
        self.frames = frames
            .iter()
            .take(MAX_FRAMES)
            .map(|(id, function)| Entry {
                id: *id,
                function: function.clone(),
                state: State::Scopes,
            })
            .collect();
        self.frames
            .iter()
            .map(|entry| ScopesArguments { frame_id: entry.id })
            .collect()
    }

    /// Nothing shown (the core runs). The values read stay the previous ones of the next stop.
    pub fn clear(&mut self) {
        self.keep_values();
        self.frames.clear();
    }

    /// The values read become the previous ones of their function; those of the functions not
    /// in this stop stay, so a value is compared with the last stop its function was in.
    fn keep_values(&mut self) {
        for entry in &self.frames {
            if let State::Done(locals) = &entry.state {
                for local in locals {
                    self.previous.insert(
                        (entry.function.clone(), local.name.clone()),
                        local.value.clone(),
                    );
                }
            }
        }
    }

    fn entry(&mut self, frame: u64) -> Option<&mut Entry> {
        self.frames.iter_mut().find(|e| e.id == frame)
    }

    /// The scopes of `frame`: the request for its Locals scope's variables, if it has one.
    pub fn scopes(&mut self, frame: u64, scopes: &[Scope]) -> Option<VariablesArguments> {
        let entry = self.entry(frame)?;
        let locals = scopes
            .iter()
            .find(|s| s.presentation_hint == Some(ScopePresentationHint::Locals));
        match locals {
            Some(scope) if scope.variables_reference > 0 => {
                entry.state = State::Variables(scope.variables_reference);
                Some(VariablesArguments {
                    variables_reference: scope.variables_reference,
                    filter: None,
                    start: None,
                    count: None,
                    format: None,
                })
            }
            _ => {
                entry.state = State::Done(Vec::new());
                None
            }
        }
    }

    /// The variables of the Locals scope of `frame`.
    pub fn variables(&mut self, frame: u64, variables: Vec<Variable>) {
        let previous = self.previous.clone();
        let Some(entry) = self.entry(frame) else {
            return;
        };
        let function = entry.function.clone();
        entry.state = State::Done(
            variables
                .into_iter()
                .map(|v| {
                    let changed = previous
                        .get(&(function.clone(), v.name.clone()))
                        .is_some_and(|before| *before != v.value);
                    Local {
                        name: v.name,
                        value: v.value,
                        changed,
                    }
                })
                .collect(),
        );
    }

    /// A request for `frame` failed.
    pub fn failed(&mut self, frame: u64, error: impl Into<String>) {
        if let Some(entry) = self.entry(frame) {
            entry.state = State::Failed(error.into());
        }
    }

    /// What frame `frame` shows; `None` for a frame not read (past [`MAX_FRAMES`]).
    pub fn shown(&self, frame: u64) -> Option<Shown<'_>> {
        let entry = self.frames.iter().find(|e| e.id == frame)?;
        Some(match &entry.state {
            State::Scopes | State::Variables(_) => Shown::Loading,
            State::Done(locals) => Shown::Locals(locals),
            State::Failed(error) => Shown::Failed(error),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope(name: &str, hint: Option<ScopePresentationHint>, reference: u64) -> Scope {
        Scope {
            name: name.into(),
            presentation_hint: hint,
            variables_reference: reference,
            named_variables: None,
            indexed_variables: None,
            expensive: false,
            source: None,
            line: None,
            column: None,
            end_line: None,
            end_column: None,
        }
    }

    fn variable(name: &str, value: &str) -> Variable {
        Variable {
            name: name.into(),
            value: value.into(),
            type_: None,
            presentation_hint: None,
            evaluate_name: None,
            variables_reference: 0,
            named_variables: None,
            indexed_variables: None,
            memory_reference: None,
            declaration_location_reference: None,
            value_location_reference: None,
        }
    }

    #[test]
    fn locals_per_frame_and_what_changed() {
        let mut locals = FrameLocals::default();
        let asked = locals.set_frames(&[(1, "scale".into()), (2, "accumulate".into())]);
        assert_eq!(asked.len(), 2);
        assert_eq!(locals.shown(1), Some(Shown::Loading));
        let request = locals
            .scopes(
                2,
                &[
                    scope("Static", None, 5),
                    scope("Locals", Some(ScopePresentationHint::Locals), 7),
                ],
            )
            .unwrap();
        assert_eq!(request.variables_reference, 7);
        locals.variables(2, vec![variable("i", "0"), variable("sum", "0")]);
        // A frame without a Locals scope shows none.
        assert!(locals.scopes(1, &[scope("Registers", None, 9)]).is_none());
        assert_eq!(locals.shown(1), Some(Shown::Locals(&[])));
        // A stop without accumulate, then one with it: sum changed since it was last seen.
        locals.set_frames(&[(5, "main".into())]);
        locals.scopes(5, &[]);
        locals.set_frames(&[(11, "accumulate".into())]);
        locals.scopes(
            11,
            &[scope("Locals", Some(ScopePresentationHint::Locals), 3)],
        );
        locals.variables(11, vec![variable("i", "0"), variable("sum", "4")]);
        let Some(Shown::Locals(shown)) = locals.shown(11) else {
            panic!()
        };
        assert!(!shown[0].changed);
        assert!(shown[1].changed);
        locals.failed(11, "gone");
        assert_eq!(locals.shown(11), Some(Shown::Failed("gone")));
        assert_eq!(locals.shown(99), None);
    }
}
