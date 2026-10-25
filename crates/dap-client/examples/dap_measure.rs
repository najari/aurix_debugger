//! Measures what the debugger's requests cost through the probe-rs DAP server: attach, step
//! into, step over and step out (request to `stopped`), continue to a breakpoint, stackTrace,
//! scopes, variables, readMemory of 64 B and 4 KiB, and liveWatch of globals on every core.
//! Each item is measured `rounds` times (default 20) and shown as median and 95th percentile.
//!
//! ```text
//! cargo run --release --example dap_measure -- <attach.json> <file>:<line> [rounds=20] [attaches=20] [globals=g_ticks,g_result] [memory=0x70000000]
//! ```
//!
//! The breakpoint line should call a function (step into goes into it, step out comes back),
//! and be reached again soon after (continue). With the flash demo (`start_in_flash`) and
//! `tc275_demo_3cores.json`: `C:\work\aurix_tcf_demo\src\demo.c:22`. Every core that runs the
//! program stops at the breakpoint; the measurements follow the first one. The target is left
//! halted: reset it afterwards.

mod common;

use std::time::{Duration, Instant};

use aurix_dap_client::DapClient;
use aurix_debug_model::protocol::{LiveWatch, LiveWatchArguments};
use serde_json::{Value, json};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

struct Item {
    name: &'static str,
    samples: Vec<Duration>,
}

fn report(items: &mut [Item]) {
    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    for item in items {
        if item.samples.is_empty() {
            println!("{:<32} no samples", item.name);
            continue;
        }
        item.samples.sort_unstable();
        let n = item.samples.len();
        println!(
            "{:<32} n={n:<4} min {:>8.1}  median {:>8.1}  p95 {:>8.1}  max {:>8.1} ms",
            item.name,
            ms(item.samples[0]),
            ms(item.samples[n / 2]),
            ms(item.samples[(n * 95 / 100).min(n - 1)]),
            ms(item.samples[n - 1]),
        );
    }
}

/// Waits for `stopped` on `thread` (any thread for `None`); returns the thread.
fn wait_stopped(client: &DapClient, thread: Option<u64>) -> Result<u64> {
    let deadline = Instant::now() + common::TIMEOUT;
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        let Some(event) = client.next_event(left) else {
            break;
        };
        if event.event != "stopped" {
            continue;
        }
        let stopped = event.body["threadId"].as_u64();
        if thread.is_none() || stopped == thread {
            return stopped.ok_or_else(|| "stopped without a thread".into());
        }
    }
    Err(format!("no stop of thread {thread:?} within {:?}", common::TIMEOUT).into())
}

/// Times `command` on `thread` up to its `stopped` event.
fn timed_run(client: &DapClient, command: &str, thread: u64) -> Result<Duration> {
    let start = Instant::now();
    client.request_raw(command, json!({ "threadId": thread }), common::TIMEOUT)?;
    wait_stopped(client, Some(thread))?;
    Ok(start.elapsed())
}

fn timed(client: &DapClient, command: &str, arguments: Value) -> Result<(Duration, Value)> {
    let start = Instant::now();
    let answer = client.request_raw(command, arguments, common::TIMEOUT)?;
    Ok((start.elapsed(), answer))
}

/// Connects and initializes, again and again while the server sets up for the next session (a
/// connection made before it listens again is closed at once).
fn connect(server: &str) -> Result<DapClient> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let attempt = DapClient::connect(server)
            .map_err(|error| error.to_string())
            .and_then(|client| {
                client
                    .request_raw(
                        "initialize",
                        json!({ "clientID": "aurix-dap-client", "adapterID": "probe-rs",
                                "linesStartAt1": true, "columnsStartAt1": true,
                                "pathFormat": "path", "supportsMemoryReferences": true }),
                        common::TIMEOUT,
                    )
                    .map(|_| client)
                    .map_err(|error| format!("initialize: {error}"))
            });
        match attempt {
            Ok(client) => return Ok(client),
            Err(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(200)),
            Err(error) => return Err(error.into()),
        }
    }
}

/// One attach: initialize, attach (timed), configurationDone, disconnect; waits for the server
/// to close the connection.
fn timed_attach(server: &str, arguments: &Value) -> Result<Duration> {
    let client = connect(server)?;
    let start = Instant::now();
    client
        .request_raw("attach", arguments.clone(), Duration::from_secs(60))
        .map_err(|error| format!("attach: {error}"))?;
    let attach = start.elapsed();
    client
        .request_raw("configurationDone", json!({}), common::TIMEOUT)
        .map_err(|error| format!("configurationDone: {error}"))?;
    client
        .request_raw(
            "disconnect",
            json!({ "terminateDebuggee": false }),
            common::TIMEOUT,
        )
        .ok();
    // The server ends the session and listens again: wait until it closes the connection.
    let deadline = Instant::now() + Duration::from_secs(10);
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        if client.wait_event(left).is_err() {
            break;
        }
    }
    Ok(attach)
}

fn main() -> Result<()> {
    let usage = "usage: dap_measure <attach.json> <file>:<line> [rounds=20] [attaches=20] [globals=a,b] [memory=0x70000000]";
    let mut args = std::env::args().skip(1);
    let attach = args.next().ok_or(usage)?;
    let place = args.next().ok_or(usage)?;
    let (path, line) = place.rsplit_once(':').ok_or(usage)?;
    let path = std::fs::canonicalize(path)?
        .display()
        .to_string()
        .trim_start_matches(r"\\?\")
        .to_string();
    let line: u64 = line.parse()?;
    let (mut rounds, mut attaches) = (20usize, 20usize);
    let mut globals = vec!["g_ticks".to_string(), "g_result".to_string()];
    let mut memory = "0x70000000".to_string();
    for arg in args {
        let (key, value) = arg.split_once('=').ok_or(usage)?;
        match key {
            "rounds" => rounds = value.parse()?,
            "attaches" => attaches = value.parse()?,
            "globals" => globals = value.split(',').map(str::to_string).collect(),
            "memory" => memory = value.to_string(),
            _ => return Err(usage.into()),
        }
    }
    let server = std::env::var("DAP_SERVER").unwrap_or_else(|_| "127.0.0.1:50000".into());
    let arguments: Value = serde_json::from_str(&std::fs::read_to_string(&attach)?)?;
    println!(
        "{attach}: probe {}, speed {}",
        arguments["probe"],
        arguments
            .get("speed")
            .map_or("not set".to_string(), |s| format!("{s} kHz"))
    );

    let mut attach_item = Item {
        name: "attach",
        samples: Vec::new(),
    };
    for _ in 0..attaches {
        attach_item.samples.push(timed_attach(&server, &arguments)?);
    }

    let client = connect(&server)?;
    client.request_raw("attach", arguments.clone(), Duration::from_secs(60))?;
    client.request_raw("configurationDone", json!({}), common::TIMEOUT)?;
    let threads = common::threads(&client)?;
    for (id, _) in &threads {
        // A core that runs already answers with an error.
        client
            .request_raw("continue", json!({ "threadId": id }), common::TIMEOUT)
            .ok();
    }
    client.request_raw(
        "setBreakpoints",
        json!({ "source": { "path": path }, "breakpoints": [{ "line": line }] }),
        common::TIMEOUT,
    )?;
    let thread = wait_stopped(&client, None)?;
    println!("stopped at {path}:{line} on thread {thread}; {rounds} rounds");

    let names = [
        "stackTrace (1 frame)",
        "scopes",
        "variables (locals)",
        "readMemory 64 B",
        "readMemory 4 KiB",
        "liveWatch, 2 globals x cores",
        "stepIn",
        "next (source line)",
        "stepOut",
        "continue -> breakpoint",
    ];
    let mut items = names
        .iter()
        .map(|&name| Item {
            name,
            samples: Vec::new(),
        })
        .collect::<Vec<_>>();
    for _ in 0..rounds {
        let (time, trace) = timed(
            &client,
            "stackTrace",
            json!({ "threadId": thread, "levels": 1 }),
        )?;
        items[0].samples.push(time);
        let frame = trace["stackFrames"][0]["id"]
            .as_i64()
            .ok_or("no stack frame")?;
        let (time, scopes) = timed(&client, "scopes", json!({ "frameId": frame }))?;
        items[1].samples.push(time);
        let locals = scopes["scopes"]
            .as_array()
            .and_then(|scopes| {
                scopes
                    .iter()
                    .find(|s| s["name"] == "Variables" || s["name"] == "Locals")
                    .or_else(|| scopes.last())
            })
            .and_then(|scope| scope["variablesReference"].as_i64())
            .ok_or("no scope")?;
        let (time, _) = timed(
            &client,
            "variables",
            json!({ "variablesReference": locals }),
        )?;
        items[2].samples.push(time);
        for (item, count) in [(3, 64), (4, 4096)] {
            let (time, answer) = timed(
                &client,
                "readMemory",
                json!({ "memoryReference": memory, "count": count }),
            )?;
            if answer["data"].as_str().is_none_or(str::is_empty) {
                return Err(format!("readMemory {count} B returned no data").into());
            }
            items[item].samples.push(time);
        }
        let start = Instant::now();
        for (id, _) in &threads {
            client.request::<LiveWatch>(
                LiveWatchArguments {
                    expressions: globals.clone(),
                    core: Some(*id),
                },
                common::TIMEOUT,
            )?;
        }
        items[5].samples.push(start.elapsed());

        items[6].samples.push(timed_run(&client, "stepIn", thread)?);
        items[7].samples.push(timed_run(&client, "next", thread)?);
        items[8]
            .samples
            .push(timed_run(&client, "stepOut", thread)?);
        items[9]
            .samples
            .push(timed_run(&client, "continue", thread)?);
    }
    client
        .request_raw(
            "setBreakpoints",
            json!({ "source": { "path": path }, "breakpoints": [] }),
            common::TIMEOUT,
        )
        .ok();
    common::stop(&client);

    report(std::slice::from_mut(&mut attach_item));
    report(&mut items);
    println!(
        "liveWatch: {} globals on {} cores",
        globals.len(),
        threads.len()
    );
    Ok(())
}
