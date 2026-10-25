//! The JUnit XML report of a run: the script is a `testsuite`, each expectation a `testcase`.
//! Expectations the run did not reach are `skipped`; an error that ended the run is the
//! suite's `errors` and `system-err`.

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
    let expectations: Vec<_> = script.expectations().collect();
    let failures = outcome.cases.iter().filter(|c| c.failure.is_some()).count();
    let skipped = expectations.len().saturating_sub(outcome.cases.len());
    let errors = usize::from(outcome.error.is_some());
    let mut xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    let name = escape(name);
    writeln!(
        xml,
        "<testsuites><testsuite name=\"{name}\" tests=\"{}\" failures=\"{failures}\" errors=\"{errors}\" skipped=\"{skipped}\" time=\"{:.3}\">",
        expectations.len(),
        outcome.time.as_secs_f64()
    )
    .ok();
    for line in expectations {
        let case = outcome.cases.iter().find(|c| c.line == line.number);
        let time = case.map_or(0.0, |c| c.time.as_secs_f64());
        write!(
            xml,
            "  <testcase classname=\"{name}\" name=\"line {}: {}\" time=\"{time:.3}\"",
            line.number,
            escape(&line.text)
        )
        .ok();
        match case {
            Some(case) => match &case.failure {
                Some(failure) => {
                    writeln!(
                        xml,
                        "><failure message=\"{}\"/></testcase>",
                        escape(failure)
                    )
                    .ok();
                }
                None => xml.push_str("/>\n"),
            },
            None => xml.push_str("><skipped/></testcase>\n"),
        }
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
    use std::collections::HashMap;
    use std::time::Duration;

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
