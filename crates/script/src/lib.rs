//! Board tests from a command file, run against the probe-rs DAP server without a screen: the
//! `aurix-run` tool. What TRACE32 PRACTICE scripts do for repeated tests, for CI.
//!
//! ```text
//! attach ../../dap-client/examples/tc275_demo_3cores.json
//! break ${AURIX_DEMO:-C:/work/aurix_tcf_demo}/src/demo.c:23
//! continue
//! wait stopped 0 reason breakpoint
//! expect k == 3
//! ```
//!
//! - [`script`]: the commands and the parser (the whole file before anything runs).
//! - [`run`]: the run against the server, and its outcome (exit code 0, 1 or 2).
//! - [`junit`]: the JUnit XML report.
//! - [`compare`]: the comparison of a value read with the value expected.
//! - [`expr`]: the expressions of `let`, `if`, `while`, `repeat` and `check`.

pub mod compare;
pub mod expr;
pub mod junit;
pub mod run;
pub mod script;
