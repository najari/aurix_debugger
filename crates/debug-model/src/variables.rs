//! The scopes and variables of a stack frame, as a tree read on demand.
//!
//! The adapter numbers scopes and variables (`variablesReference`) anew at every stop, so what
//! is expanded is kept by name path (scope name, then variable names): a stop shows the same
//! nodes expanded and asks for their children again. Values are compared with the previous
//! stop's to mark what changed: those of a function's scopes (locals, arguments, registers,
//! which the adapter unwinds per frame) with the same function's on the same thread, those of
//! the other scopes (statics, peripherals) with the same thread's. The scopes that are not
//! expensive (the locals) are expanded the first time they are seen.

use std::collections::{BTreeSet, HashMap, HashSet};

use dap_types::{Scope, ScopePresentationHint, ScopesArguments, Variable, VariablesArguments};

/// A value as it was read: by context (thread, and function for a function's scopes) and key.
type ValueKey = (String, Vec<String>);

#[derive(Clone, Debug)]
struct Frame {
    id: u64,
    thread: String,
    function: String,
}

/// How deep the tree goes at most.
pub const MAX_DEPTH: usize = 16;

/// One shown row of the tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub depth: usize,
    /// The name path of the node, scope name first.
    pub key: Vec<String>,
    pub name: String,
    /// Empty for a scope.
    pub value: String,
    pub type_: Option<String>,
    /// The adapter's reference of the children, 0 without children.
    pub reference: u64,
    pub expanded: bool,
    /// Expanded, children asked for but not answered.
    pub loading: bool,
    /// The value differs from the previous stop's.
    pub changed: bool,
    /// Where the value is, for a memory view (`0x7000bfe4@0`).
    pub memory_reference: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Variables {
    /// The frame shown.
    frame: Option<Frame>,
    scopes: Vec<Scope>,
    children: HashMap<u64, Vec<Variable>>,
    pending: HashSet<u64>,
    errors: HashMap<u64, String>,
    expanded: BTreeSet<Vec<String>>,
    /// Scope names expanded once already (or collapsed by the user since).
    seen_scopes: HashSet<String>,
    /// The values read in this stop.
    values: HashMap<ValueKey, String>,
    /// The values of the previous stop.
    previous: HashMap<ValueKey, String>,
}

impl Variables {
    /// A new stop: the values shown so far become the previous ones.
    pub fn stopped(&mut self) {
        let values = std::mem::take(&mut self.values);
        if !values.is_empty() {
            self.previous = values;
        }
        self.clear();
    }

    /// Nothing shown (the core runs, or no frame is selected). What is expanded is kept.
    pub fn clear(&mut self) {
        self.frame = None;
        self.scopes.clear();
        self.children.clear();
        self.pending.clear();
        self.errors.clear();
    }

    /// Shows frame `id` of `thread`, in `function`: the request for its scopes, for
    /// [`Self::set_scopes`].
    pub fn set_frame(
        &mut self,
        id: u64,
        thread: impl Into<String>,
        function: impl Into<String>,
    ) -> ScopesArguments {
        self.clear();
        self.frame = Some(Frame {
            id,
            thread: thread.into(),
            function: function.into(),
        });
        ScopesArguments { frame_id: id }
    }

    pub fn frame_id(&self) -> Option<u64> {
        self.frame.as_ref().map(|frame| frame.id)
    }

    /// What the values of `scope` are compared within.
    fn context(&self, scope: &Scope) -> Option<String> {
        let frame = self.frame.as_ref()?;
        let per_function = matches!(
            scope.presentation_hint,
            Some(
                ScopePresentationHint::Locals
                    | ScopePresentationHint::Arguments
                    | ScopePresentationHint::Registers
                    | ScopePresentationHint::ReturnValue
            )
        );
        Some(if per_function {
            format!("{}\u{1f}{}", frame.thread, frame.function)
        } else {
            frame.thread.clone()
        })
    }

    /// The scopes of frame `id`; returns the requests for the children of the expanded scopes.
    pub fn set_scopes(&mut self, id: u64, scopes: Vec<Scope>) -> Vec<VariablesArguments> {
        if self.frame_id() != Some(id) {
            return Vec::new();
        }
        for scope in &scopes {
            if self.seen_scopes.insert(scope.name.clone()) && !scope.expensive {
                self.expanded.insert(vec![scope.name.clone()]);
            }
        }
        self.scopes = scopes;
        let wanted = self
            .scopes
            .iter()
            .filter(|scope| self.expanded.contains(&vec![scope.name.clone()]))
            .map(|scope| scope.variables_reference)
            .collect::<Vec<_>>();
        wanted
            .into_iter()
            .filter_map(|reference| self.ask(reference))
            .collect()
    }

    /// The children of `reference`; returns the requests for the children of the expanded
    /// ones among them.
    pub fn set_children(
        &mut self,
        reference: u64,
        variables: Vec<Variable>,
    ) -> Vec<VariablesArguments> {
        if !self.pending.remove(&reference) {
            return Vec::new();
        }
        self.children.insert(reference, variables);
        let mut wanted = Vec::new();
        for (row, context) in self.walk() {
            if let Some(context) = context
                && row.depth > 0
            {
                self.values
                    .insert((context, row.key.clone()), row.value.clone());
            }
            if row.expanded && row.reference != 0 && !self.children.contains_key(&row.reference) {
                wanted.push(row.reference);
            }
        }
        wanted
            .into_iter()
            .filter_map(|reference| self.ask(reference))
            .collect()
    }

    /// The adapter could not give the children of `reference`.
    pub fn failed(&mut self, reference: u64, error: impl Into<String>) {
        if self.pending.remove(&reference) {
            self.errors.insert(reference, error.into());
        }
    }

    pub fn error(&self, reference: u64) -> Option<&str> {
        self.errors.get(&reference).map(String::as_str)
    }

    /// Expands or collapses the node `key` (whose children are `reference`); returns the
    /// request for its children if they are needed.
    pub fn toggle(&mut self, key: &[String], reference: u64) -> Option<VariablesArguments> {
        if reference == 0 {
            return None;
        }
        if self.expanded.remove(key) {
            return None;
        }
        self.expanded.insert(key.to_vec());
        self.errors.remove(&reference);
        if self.children.contains_key(&reference) {
            None
        } else {
            self.ask(reference)
        }
    }

    fn ask(&mut self, reference: u64) -> Option<VariablesArguments> {
        if reference == 0 || self.children.contains_key(&reference) {
            return None;
        }
        self.pending
            .insert(reference)
            .then_some(VariablesArguments {
                variables_reference: reference,
                filter: None,
                start: None,
                count: None,
                format: None,
            })
    }

    /// The rows shown: the scopes, and the children of what is expanded.
    pub fn rows(&self) -> Vec<Row> {
        self.walk().into_iter().map(|(row, _)| row).collect()
    }

    /// The rows shown, with the context their values are compared within.
    fn walk(&self) -> Vec<(Row, Option<String>)> {
        let mut rows = Vec::new();
        for scope in &self.scopes {
            let context = self.context(scope);
            let key = vec![scope.name.clone()];
            let expanded = scope.variables_reference != 0 && self.expanded.contains(&key);
            rows.push((
                Row {
                    depth: 0,
                    name: scope.name.clone(),
                    value: String::new(),
                    type_: None,
                    reference: scope.variables_reference,
                    expanded,
                    loading: expanded && self.pending.contains(&scope.variables_reference),
                    changed: false,
                    memory_reference: None,
                    key: key.clone(),
                },
                context.clone(),
            ));
            if expanded {
                self.add_children(&mut rows, &context, scope.variables_reference, &key, 1);
            }
        }
        rows
    }

    fn add_children(
        &self,
        rows: &mut Vec<(Row, Option<String>)>,
        context: &Option<String>,
        reference: u64,
        parent: &[String],
        depth: usize,
    ) {
        if depth > MAX_DEPTH {
            return;
        }
        let Some(children) = self.children.get(&reference) else {
            return;
        };
        for variable in children {
            let mut key = parent.to_vec();
            key.push(variable.name.clone());
            let expanded = variable.variables_reference != 0 && self.expanded.contains(&key);
            let changed = context.as_ref().is_some_and(|context| {
                self.previous
                    .get(&(context.clone(), key.clone()))
                    .is_some_and(|previous| *previous != variable.value)
            });
            let row = Row {
                depth,
                name: variable.name.clone(),
                value: variable.value.clone(),
                type_: variable.type_.clone(),
                reference: variable.variables_reference,
                expanded,
                loading: expanded && self.pending.contains(&variable.variables_reference),
                changed,
                memory_reference: variable.memory_reference.clone(),
                key: key.clone(),
            };
            rows.push((row, context.clone()));
            if expanded {
                self.add_children(rows, context, variable.variables_reference, &key, depth + 1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The scopes as probe-rs gives them (without Peripherals).
    fn scopes(locals: u64, registers: u64) -> Vec<Scope> {
        serde_json::from_value(json!([
            { "name": "Registers", "expensive": true, "presentationHint": "registers",
              "variablesReference": registers },
            { "name": "Variables", "expensive": false, "presentationHint": "locals",
              "variablesReference": locals },
        ]))
        .unwrap()
    }

    fn with_statics(locals: u64, statics: u64) -> Vec<Scope> {
        let mut scopes = scopes(locals, locals + 1);
        scopes.insert(
            0,
            serde_json::from_value(json!({
                "name": "Static", "expensive": true, "presentationHint": "statics",
                "variablesReference": statics,
            }))
            .unwrap(),
        );
        scopes
    }

    fn changed(tree: &Variables) -> Vec<String> {
        let rows = tree.rows();
        rows.into_iter()
            .filter(|r| r.changed)
            .map(|r| r.key.join("/"))
            .collect()
    }

    fn variables(rows: serde_json::Value) -> Vec<Variable> {
        serde_json::from_value(rows).unwrap()
    }

    fn references(requests: &[VariablesArguments]) -> Vec<u64> {
        requests.iter().map(|r| r.variables_reference).collect()
    }

    #[test]
    fn the_locals_open_and_what_is_expanded_survives_a_stop() {
        let mut tree = Variables::default();
        assert_eq!(tree.set_frame(7, "0", "scale").frame_id, 7);
        // An answer for another frame is dropped.
        assert!(tree.set_scopes(8, scopes(100, 101)).is_empty());
        assert_eq!(references(&tree.set_scopes(7, scopes(100, 101))), [100]);
        let rows = tree.rows();
        assert_eq!(rows.len(), 2);
        assert!(!rows[0].expanded && rows[1].expanded && rows[1].loading);

        let asked = tree.set_children(
            100,
            variables(json!([
                { "name": "v", "value": "1", "type": "int", "variablesReference": 0 },
                { "name": "p", "value": "{...}", "variablesReference": 102 },
            ])),
        );
        assert!(asked.is_empty());
        let rows = tree.rows();
        assert_eq!(rows.len(), 4);
        assert_eq!((rows[2].depth, rows[2].name.as_str()), (1, "v"));

        // Expanding p asks for its children; collapsing it asks nothing.
        let p = rows[3].clone();
        assert_eq!(p.key, ["Variables", "p"]);
        assert_eq!(
            references(
                &tree
                    .toggle(&p.key, p.reference)
                    .into_iter()
                    .collect::<Vec<_>>()
            ),
            [102]
        );
        tree.set_children(
            102,
            variables(json!([{ "name": "x", "value": "5", "variablesReference": 0 }])),
        );
        assert_eq!(tree.rows().len(), 5);

        // The next stop numbers everything anew; Variables and p stay expanded and are asked
        // for again, and the values that differ are marked.
        tree.stopped();
        assert!(tree.rows().is_empty());
        tree.set_frame(9, "0", "scale");
        assert_eq!(references(&tree.set_scopes(9, scopes(200, 201))), [200]);
        let asked = tree.set_children(
            200,
            variables(json!([
                { "name": "v", "value": "2", "variablesReference": 0 },
                { "name": "p", "value": "{...}", "variablesReference": 202 },
            ])),
        );
        assert_eq!(references(&asked), [202]);
        tree.set_children(
            202,
            variables(json!([{ "name": "x", "value": "5", "variablesReference": 0 }])),
        );
        assert_eq!(changed(&tree), ["Variables/v"]);
    }

    #[test]
    fn statics_compare_across_functions_and_locals_do_not() {
        let mut tree = Variables::default();
        tree.set_frame(1, "0", "scale");
        tree.set_scopes(1, with_statics(10, 12));
        tree.toggle(&["Static".to_string()], 12);
        tree.set_children(
            10,
            variables(json!([{ "name": "i", "value": "1", "variablesReference": 0 }])),
        );
        tree.set_children(
            12,
            variables(json!([{ "name": "g_ticks", "value": "7", "variablesReference": 0 }])),
        );

        // Stopped in another function: g_ticks is compared, its local i is not.
        tree.stopped();
        tree.set_frame(2, "0", "accumulate");
        assert_eq!(
            references(&tree.set_scopes(2, with_statics(20, 22))),
            [22, 20]
        );
        tree.set_children(
            20,
            variables(json!([{ "name": "i", "value": "2", "variablesReference": 0 }])),
        );
        tree.set_children(
            22,
            variables(json!([{ "name": "g_ticks", "value": "8", "variablesReference": 0 }])),
        );
        assert_eq!(changed(&tree), ["Static/g_ticks"]);

        // The same function on another core: nothing to compare with.
        tree.set_frame(3, "1", "accumulate");
        tree.set_scopes(3, with_statics(30, 32));
        tree.set_children(
            30,
            variables(json!([{ "name": "i", "value": "9", "variablesReference": 0 }])),
        );
        tree.set_children(
            32,
            variables(json!([{ "name": "g_ticks", "value": "9", "variablesReference": 0 }])),
        );
        assert!(changed(&tree).is_empty());
    }

    #[test]
    fn a_collapsed_scope_stays_collapsed_and_errors_are_kept() {
        let mut tree = Variables::default();
        tree.set_frame(1, "0", "main");
        tree.set_scopes(1, scopes(10, 11));
        let locals = vec!["Variables".to_string()];
        assert_eq!(tree.toggle(&locals, 10), None);
        tree.set_children(10, Vec::new());

        tree.stopped();
        tree.set_frame(2, "0", "main");
        assert!(tree.set_scopes(2, scopes(20, 21)).is_empty());

        let registers = vec!["Registers".to_string()];
        assert!(tree.toggle(&registers, 21).is_some());
        // Asked once only.
        assert!(tree.set_scopes(2, scopes(20, 21)).is_empty());
        tree.failed(21, "not halted");
        assert_eq!(tree.error(21), Some("not halted"));
        assert!(!tree.rows()[0].loading);
    }
}
