//! The JUnit XML report of a run: the script is a `testsuite`, each expectation that ran a
//! `testcase` (one in a loop, each time: `#2`, `#3`, …), and so is each `fail` that ran.
//! Expectations the run did not reach are `skipped`; an error that ended the run is the
//! suite's `errors` and `system-err`.

use std::collections::HashMap;
use std::fmt::Write;

use crate::run::Outcome;
use crate::script::Script;

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            // Control characters are not allowed in XML 1.0.
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

/// The report of `outcome`, the run of `script` named `name`.
pub fn report(name: &str, script: &Script, outcome: &Outcome) -> String {
    let not_reached: Vec<_> = script
        .expectations()
        .filter(|line| !outcome.cases.iter().any(|c| c.line == line.number))
        .collect();
    let failures = outcome.cases.iter().filter(|c| c.failure.is_some()).count();
    let errors = usize::from(outcome.error.is_some());
    let mut xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    let name = escape(name);
    writeln!(
        xml,
        "<testsuites><testsuite name=\"{name}\" tests=\"{}\" failures=\"{failures}\" errors=\"{errors}\" skipped=\"{}\" time=\"{:.3}\">",
        outcome.cases.len() + not_reached.len(),
        not_reached.len(),
        outcome.time.as_secs_f64()
    )
    .ok();
    let mut runs: HashMap<usize, usize> = HashMap::new();
    for case in &outcome.cases {
        let run = runs.entry(case.line).or_insert(0);
        *run += 1;
        let repeated = if *run > 1 {
            format!(" #{run}")
        } else {
            String::new()
        };
        write!(
            xml,
            "  <testcase classname=\"{name}\" name=\"line {}: {}{repeated}\" time=\"{:.3}\"",
            case.line,
            escape(&case.text),
            case.time.as_secs_f64()
        )
        .ok();
        match &case.failure {
            Some(failure) => {
                writeln!(
                    xml,
                    "><failure message=\"{}\"/></testcase>",
                    escape(failure)
                )
                .ok();
            }
            None => xml.push_str("/>\n"),
        }
    }
    for line in not_reached {
        writeln!(
            xml,
            "  <testcase classname=\"{name}\" name=\"line {}: {}\" time=\"0.000\"><skipped/></testcase>",
            line.number,
            escape(&line.text)
        )
        .ok();
    }
    if let Some(error) = &outcome.error {
        let at = error
            .line
            .map(|line| format!("line {line}: "))
            .unwrap_or_default();
        writeln!(
            xml,
            "  <system-err>{}</system-err>",
            escape(&format!("{at}{}", error.message))
        )
        .ok();
    }
    xml.push_str("</testsuite></testsuites>\n");
    xml
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::{CaseResult, RunError};
    use std::time::Duration;

    #[test]
    fn a_check_in_a_loop_is_a_case_each_time() {
        let script = Script::parse(
            "let i = 0\nrepeat 2\ncheck i < 1\nlet i = i + 1\nend\n",
            &HashMap::new(),
        )
        .unwrap();
        let case = |failure: Option<&str>| CaseResult {
            line: 3,
            text: "check i < 1".into(),
            failure: failure.map(str::to_string),
            time: Duration::ZERO,
        };
        let outcome = Outcome {
            cases: vec![case(None), case(Some("does not hold: i = 1"))],
            error: None,
            time: Duration::from_secs(1),
        };
        let xml = report("loop.aurix", &script, &outcome);
        assert!(xml.contains("tests=\"2\" failures=\"1\" errors=\"0\" skipped=\"0\""));
        assert!(xml.contains("name=\"line 3: check i &lt; 1 #2\""));
    }

    #[test]
    fn one_testcase_per_expectation() {
        let script = Script::parse(
            "attach a.json\nexpect k == 3\nexpect v < 4\nexpect live g_ticks > 0\n",
            &HashMap::new(),
        )
        .unwrap();
        let outcome = Outcome {
            cases: vec![
                CaseResult {
                    line: 2,
                    text: "expect k == 3".into(),
                    failure: None,
                    time: Duration::from_millis(12),
                },
                CaseResult {
                    line: 3,
                    text: "expect v < 4".into(),
                    failure: Some("expected v < 4, found <7>".into()),
                    time: Duration::from_millis(9),
                },
            ],
            error: Some(RunError {
                line: Some(4),
                message: "the server closed the connection".into(),
            }),
            time: Duration::from_secs(1),
        };
        let xml = report("demo_flash.aurix", &script, &outcome);
        assert_eq!(xml.matches("<testcase ").count(), 3);
        assert!(xml.contains("tests=\"3\" failures=\"1\" errors=\"1\" skipped=\"1\""));
        assert!(xml.contains("found &lt;7&gt;"));
        assert!(xml.contains("<skipped/>"));
        assert!(xml.contains("<system-err>line 4: the server"));
    }
}
