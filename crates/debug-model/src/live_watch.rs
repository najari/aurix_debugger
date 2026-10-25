//! Live Watch: global variables read while the target runs, with the probe-rs `liveWatch`
//! request. In a session of several cores (one thread per core) the expressions are read on
//! every core; a read still under way is not asked again (a slow probe must not pile up
//! requests), and a value that changed since the last read is marked.

use std::collections::{HashMap, HashSet};

use crate::protocol::{LiveWatchArguments, LiveWatchValue};

/// How often a client reads the expressions, by default.
pub const INTERVAL_MS: u64 = 500;

/// What the expressions are read on: the session, or one core of it.
#[derive(Clone, Debug, Default)]
pub struct Target {
    /// The core (thread id) in a session of several cores.
    pub core: Option<u64>,
    pub label: String,
    pub values: HashMap<String, LiveWatchValue>,
    /// The expressions whose value changed at the last read.
    pub changed: HashSet<String>,
    pub error: Option<String>,
    /// A read is under way.
    pub busy: bool,
}

#[derive(Clone, Debug, Default)]
pub struct LiveWatch {
    expressions: Vec<String>,
    targets: Vec<Target>,
    paused: bool,
}

impl LiveWatch {
    pub fn new(expressions: Vec<String>) -> Self {
        Self {
            expressions,
            ..Default::default()
        }
    }

    pub fn expressions(&self) -> &[String] {
        &self.expressions
    }

    pub fn targets(&self) -> &[Target] {
        &self.targets
    }

    pub fn paused(&self) -> bool {
        self.paused
    }

    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
    }

    /// Adds an expression (trimmed); returns whether it is new.
    pub fn add(&mut self, expression: &str) -> bool {
        let expression = expression.trim();
        if expression.is_empty() || self.expressions.iter().any(|e| e == expression) {
            return false;
        }
        self.expressions.push(expression.to_string());
        true
    }

    pub fn remove(&mut self, expression: &str) {
        self.expressions.retain(|e| e != expression);
    }

    /// The targets for the session's threads (id, name): one per thread when there are
    /// several (each thread is a core), else the session. Targets that stay keep their values.
    pub fn set_threads(&mut self, threads: &[(u64, String)]) {
        let wanted: Vec<(Option<u64>, String)> = if threads.len() > 1 {
            threads
                .iter()
                .map(|(id, name)| (Some(*id), name.clone()))
                .collect()
        } else {
            vec![(None, String::new())]
        };
        let mut old = std::mem::take(&mut self.targets);
        self.targets = wanted
            .into_iter()
            .map(
                |(core, label)| match old.iter().position(|target| target.core == core) {
                    Some(ix) => {
                        let mut target = old.swap_remove(ix);
                        target.label = label;
                        target
                    }
                    None => Target {
                        core,
                        label,
                        ..Default::default()
                    },
                },
            )
            .collect();
    }

    /// The reads to send now, one per target that is not reading already (marked as reading).
    /// None while paused or without expressions.
    pub fn take_requests(&mut self) -> Vec<LiveWatchArguments> {
        if self.paused || self.expressions.is_empty() {
            return Vec::new();
        }
        if self.targets.is_empty() {
            self.set_threads(&[]);
        }
        let expressions = self.expressions.clone();
        self.targets
            .iter_mut()
            .filter(|target| !target.busy)
            .map(|target| {
                target.busy = true;
                LiveWatchArguments {
                    expressions: expressions.clone(),
                    core: target.core,
                }
            })
            .collect()
    }

    /// The answer to the read of `core`'s target.
    pub fn answered(&mut self, core: Option<u64>, answer: Result<Vec<LiveWatchValue>, String>) {
        let Some(target) = self.targets.iter_mut().find(|t| t.core == core) else {
            return;
        };
        target.busy = false;
        match answer {
            Ok(values) => {
                let values: HashMap<String, LiveWatchValue> = values
                    .into_iter()
                    .map(|value| (value.expression.clone(), value))
                    .collect();
                target.changed = values
                    .iter()
                    .filter(|(expression, value)| {
                        target
                            .values
                            .get(*expression)
                            .is_some_and(|old| old.value != value.value)
                    })
                    .map(|(expression, _)| expression.clone())
                    .collect();
                target.values = values;
                target.error = None;
            }
            Err(error) => target.error = Some(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(expression: &str, value: &str) -> LiveWatchValue {
        LiveWatchValue {
            expression: expression.into(),
            value: Some(value.into()),
            type_: None,
            error: None,
        }
    }

    #[test]
    fn every_core_is_read_once_at_a_time_and_changes_are_marked() {
        let mut watch = LiveWatch::new(vec![]);
        assert!(watch.take_requests().is_empty());
        assert!(watch.add(" g_ticks "));
        assert!(!watch.add("g_ticks"));
        watch.set_threads(&[(0, "0-TC275".into()), (1, "1-TC275".into())]);

        let requests = watch.take_requests();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[1].core, Some(1));
        // Still reading: nothing is asked again.
        assert!(watch.take_requests().is_empty());

        watch.answered(Some(0), Ok(vec![value("g_ticks", "1")]));
        watch.answered(Some(1), Err("probe gone".into()));
        assert_eq!(watch.take_requests().len(), 2);
        watch.answered(Some(0), Ok(vec![value("g_ticks", "2")]));
        let core0 = &watch.targets()[0];
        assert!(core0.changed.contains("g_ticks"));
        assert_eq!(watch.targets()[1].error.as_deref(), Some("probe gone"));

        watch.set_paused(true);
        assert!(watch.take_requests().is_empty());
    }

    #[test]
    fn one_thread_reads_the_session() {
        let mut watch = LiveWatch::new(vec!["g_ticks".into()]);
        watch.set_threads(&[(0, "0-TC275".into())]);
        let requests = watch.take_requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].core, None);
    }
}
