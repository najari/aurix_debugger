//! Reads global variables of a probe-rs session while the target runs, on every core.
//!
//! ```text
//! cargo run --example live_watch -- <attach.json> <seconds> <expression>...
//! ```
//!
//! Prints a line per core and read; a value that changed since the last read has a `*`. See
//! `examples/tc275_demo_3cores.json`. The target is left halted.

mod common;

use std::time::{Duration, Instant};

use aurix_debug_model::live_watch::{INTERVAL_MS, LiveWatch};
use aurix_debug_model::protocol::LiveWatch as LiveWatchRequest;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let usage = "usage: live_watch <attach.json> <seconds> <expression>...";
    let attach = args.next().ok_or(usage)?;
    let seconds: u64 = args.next().ok_or(usage)?.parse()?;
    let mut watch = LiveWatch::new(Vec::new());
    for expression in args {
        watch.add(&expression);
    }

    let client = common::start(&attach)?;
    let deadline = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < deadline {
        watch.set_threads(&common::threads(&client)?);
        for request in watch.take_requests() {
            let core = request.core;
            let answer = client
                .request::<LiveWatchRequest>(request, common::TIMEOUT)
                .map(|response| response.values)
                .map_err(|error| error.to_string());
            watch.answered(core, answer);
        }
        for target in watch.targets() {
            let values = watch
                .expressions()
                .iter()
                .map(|expression| {
                    let shown = match target.values.get(expression) {
                        Some(value) => value.value.clone().unwrap_or_else(|| {
                            format!("<{}>", value.error.clone().unwrap_or_default())
                        }),
                        None => "…".into(),
                    };
                    let mark = if target.changed.contains(expression) {
                        "*"
                    } else {
                        ""
                    };
                    format!("{expression} = {shown}{mark}")
                })
                .collect::<Vec<_>>()
                .join(", ");
            match &target.error {
                Some(error) => println!("[{}] error: {error}", target.label),
                None => println!("[{}] {values}", target.label),
            }
        }
        std::thread::sleep(Duration::from_millis(INTERVAL_MS));
    }
    common::stop(&client);
    Ok(())
}
