//! Sets one breakpoint with a condition, hit count or log message, and prints what it does.
//!
//! ```text
//! cargo run --example break_when -- <attach.json> <seconds> <where> [option=value]...
//!   where     <file>:<line>, or data:<variable>[:write|read|readWrite]
//!   if=       the condition, e.g. "v == 3"
//!   hit=      the hit count: N, >= N, > N or % N
//!   log=      a log message, e.g. "v = {v}" (the core does not stop)
//!   show=     names to read at each stop, comma separated, e.g. v,k
//!   stops=    end after this many stops (default 10)
//!   logs=     end after this many log lines
//!   running=  a global variable that changes while a core runs (e.g. g_ticks): at the end,
//!             tells for each core whether it still runs
//!   from=     <file>:<line> to stop at first, before the breakpoint is set, so that its hits
//!             are counted from a known place
//! ```
//!
//! At each stop, the stopped core (and with all-stop the cores halted with it) is continued.
//! The breakpoint is removed at the end and the target is left halted.

mod common;

use std::collections::HashMap;
use std::time::{Duration, Instant};

use aurix_dap_client::DapClient;
use aurix_debug_model::protocol::{LiveWatch, LiveWatchArguments};
use serde_json::json;

#[derive(Default)]
struct Options {
    condition: Option<String>,
    hit: Option<String>,
    log: Option<String>,
    show: Vec<String>,
    stops: usize,
    logs: Option<usize>,
    running: Option<String>,
    from: Option<(String, u64)>,
}

enum Where {
    Line { path: String, line: u64 },
    Data { name: String, access: String },
}

fn parse_where(text: &str) -> Result<Where, String> {
    if let Some(rest) = text.strip_prefix("data:") {
        let (name, access) = rest.split_once(':').unwrap_or((rest, "write"));
        return Ok(Where::Data {
            name: name.to_string(),
            access: access.to_string(),
        });
    }
    let (path, line) = text
        .rsplit_once(':')
        .ok_or_else(|| format!("`{text}` is not <file>:<line>"))?;
    Ok(Where::Line {
        path: std::fs::canonicalize(path)
            .map(|p| {
                p.display()
                    .to_string()
                    .trim_start_matches(r"\\?\")
                    .to_string()
            })
            .map_err(|error| format!("{path}: {error}"))?,
        line: line
            .parse()
            .map_err(|_| format!("`{line}` is not a line"))?,
    })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let usage = "usage: break_when <attach.json> <seconds> <file:line | data:variable> [if=..] [hit=..] [log=..] [show=a,b] [stops=N] [logs=N] [running=global]";
    let attach = args.next().ok_or(usage)?;
    let seconds: u64 = args.next().ok_or(usage)?.parse()?;
    let place = parse_where(&args.next().ok_or(usage)?)?;
    let mut options = Options {
        stops: 10,
        ..Options::default()
    };
    for arg in args {
        let (key, value) = arg.split_once('=').ok_or(usage)?;
        match key {
            "if" => options.condition = Some(value.to_string()),
            "hit" => options.hit = Some(value.to_string()),
            "log" => options.log = Some(value.to_string()),
            "show" => options.show = value.split(',').map(str::to_string).collect(),
            "stops" => options.stops = value.parse()?,
            "logs" => options.logs = Some(value.parse()?),
            "running" => options.running = Some(value.to_string()),
            "from" => match parse_where(value)? {
                Where::Line { path, line } => options.from = Some((path, line)),
                Where::Data { .. } => return Err(usage.into()),
            },
            _ => return Err(usage.into()),
        }
    }

    let client = common::start(&attach)?;
    let threads = common::threads(&client)?;
    let start = Instant::now();
    let from = match &options.from {
        Some((path, line)) => Some(stop_at(&client, path, *line)?),
        None => None,
    };
    set(&client, &place, &options, &threads)?;
    if let Some(thread) = from {
        client.request_raw("continue", json!({ "threadId": thread }), common::TIMEOUT)?;
    }
    let set_at = Instant::now();
    let at_set = options
        .running
        .as_ref()
        .map(|global| read(&client, &threads, global));

    let deadline = set_at + Duration::from_secs(seconds);
    let mut stops = 0;
    let mut logs = 0;
    let mut last_log: HashMap<u64, Instant> = HashMap::new();
    let mut log_gaps: Vec<Duration> = Vec::new();
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        if stops >= options.stops || options.logs.is_some_and(|n| logs >= n) {
            break;
        }
        let Some(event) = client.next_event(left) else {
            break;
        };
        let at = set_at.elapsed().as_secs_f64();
        match event.event.as_str() {
            "stopped" => {
                let thread = event.body["threadId"].as_u64().unwrap_or(0);
                let reason = event.body["reason"].as_str().unwrap_or("?");
                let description = event.body["description"].as_str().unwrap_or("");
                if reason == "external" {
                    println!("{at:8.3}s  core {thread} halted with the others");
                } else {
                    stops += 1;
                    let shown = show(&client, thread, &options.show);
                    println!(
                        "{at:8.3}s  stop {stops}: core {thread} {reason} {shown}  ({description})"
                    );
                }
                client
                    .request_raw("continue", json!({ "threadId": thread }), common::TIMEOUT)
                    .ok();
            }
            "output" => {
                let text = event.body["output"].as_str().unwrap_or("").trim_end();
                // A line of the log message starts with its text before the first name.
                let prefix = options
                    .log
                    .as_deref()
                    .map(|log| log.split('{').next().unwrap_or(""));
                if let Some(prefix) = prefix
                    && text.starts_with(prefix)
                {
                    logs += 1;
                    let thread = event.body["threadId"].as_u64().unwrap_or(0);
                    let now = Instant::now();
                    if let Some(before) = last_log.insert(thread, now) {
                        log_gaps.push(now - before);
                    }
                    if logs <= 5 || logs % 100 == 0 {
                        println!("{at:8.3}s  log {logs}: {text}");
                    }
                } else if !text.is_empty() {
                    println!("{at:8.3}s  console: {text}");
                }
            }
            _ => {}
        }
    }

    let elapsed = set_at.elapsed();
    println!(
        "\n{stops} stop(s), {logs} log line(s) in {:.2}s (attach {:.2}s)",
        elapsed.as_secs_f64(),
        (set_at - start).as_secs_f64()
    );
    if !log_gaps.is_empty() {
        log_gaps.sort();
        let ms = |d: Duration| d.as_secs_f64() * 1000.0;
        println!(
            "time between two log lines of one core: min {:.1} ms, median {:.1} ms, max {:.1} ms",
            ms(log_gaps[0]),
            ms(log_gaps[log_gaps.len() / 2]),
            ms(log_gaps[log_gaps.len() - 1])
        );
    }

    let before_clear = options
        .running
        .as_ref()
        .map(|global| read(&client, &threads, global));
    clear(&client, &place);
    if let Some(global) = &options.running {
        // Let the last breakpoint run go, then look whether each core moves on.
        let quiet = Instant::now() + Duration::from_millis(300);
        while let Some(left) = quiet.checked_duration_since(Instant::now()) {
            let Some(event) = client.next_event(left) else {
                break;
            };
            if matches!(event.event.as_str(), "stopped" | "continued" | "output") {
                println!("after removal: {} {}", event.event, event.body);
            }
        }
        let first = read(&client, &threads, global);
        std::thread::sleep(Duration::from_millis(500));
        let second = read(&client, &threads, global);
        for (id, name) in &threads {
            let (a, b) = (first.get(id), second.get(id));
            let state = if a.is_some() && a != b {
                "runs"
            } else {
                "does NOT run"
            };
            let since = at_set.as_ref().and_then(|values| values.get(id));
            let until = before_clear.as_ref().and_then(|values| values.get(id));
            println!(
                "{name}: {global} {since:?} at the set, {until:?} before the removal, {a:?} -> {b:?} after, {state}"
            );
        }
    }
    common::stop(&client);
    Ok(())
}

fn set(
    client: &DapClient,
    place: &Where,
    options: &Options,
    threads: &[(u64, String)],
) -> Result<(), Box<dyn std::error::Error>> {
    match place {
        Where::Line { path, line } => {
            let mut breakpoint = json!({ "line": line });
            if let Some(condition) = &options.condition {
                breakpoint["condition"] = json!(condition);
            }
            if let Some(hit) = &options.hit {
                breakpoint["hitCondition"] = json!(hit);
            }
            if let Some(log) = &options.log {
                breakpoint["logMessage"] = json!(log);
            }
            let answer = client.request_raw(
                "setBreakpoints",
                json!({ "source": { "path": path }, "breakpoints": [breakpoint] }),
                common::TIMEOUT,
            )?;
            println!("setBreakpoints: {}", answer["breakpoints"]);
        }
        Where::Data { name, access } => {
            // The data ID of a variable needs a frame: halt the first core for it.
            let (thread, _) = threads.first().ok_or("no thread")?;
            client.request_raw("pause", json!({ "threadId": thread }), common::TIMEOUT)?;
            wait_stopped(client, *thread);
            let frame = top_frame(client, *thread).ok_or("no stack frame")?;
            let info = client.request_raw(
                "dataBreakpointInfo",
                json!({ "name": name, "frameId": frame }),
                common::TIMEOUT,
            )?;
            let data_id = info["dataId"].as_str().ok_or("no data ID")?.to_string();
            let mut breakpoint = json!({ "dataId": data_id, "accessType": access });
            if let Some(condition) = &options.condition {
                breakpoint["condition"] = json!(condition);
            }
            if let Some(hit) = &options.hit {
                breakpoint["hitCondition"] = json!(hit);
            }
            let answer = client.request_raw(
                "setDataBreakpoints",
                json!({ "breakpoints": [breakpoint] }),
                common::TIMEOUT,
            )?;
            println!("setDataBreakpoints: {}", answer["breakpoints"]);
            client.request_raw("continue", json!({ "threadId": thread }), common::TIMEOUT)?;
        }
    }
    Ok(())
}

/// Stops at `path:line` with a plain breakpoint and removes it; returns the core that stopped,
/// still halted.
fn stop_at(client: &DapClient, path: &str, line: u64) -> Result<u64, Box<dyn std::error::Error>> {
    let source = json!({ "path": path });
    client.request_raw(
        "setBreakpoints",
        json!({ "source": source, "breakpoints": [{ "line": line }] }),
        common::TIMEOUT,
    )?;
    let deadline = Instant::now() + common::TIMEOUT;
    let mut thread = None;
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        match client.next_event(left) {
            Some(event) if event.event == "stopped" && event.body["reason"] != "external" => {
                thread = event.body["threadId"].as_u64();
                break;
            }
            Some(_) => {}
            None => break,
        }
    }
    let thread = thread.ok_or("no stop at the from= line")?;
    // Same file as the breakpoint to set: the new set replaces this one.
    client.request_raw(
        "setBreakpoints",
        json!({ "source": source, "breakpoints": [] }),
        common::TIMEOUT,
    )?;
    println!("stopped at {path}:{line} on core {thread}");
    Ok(thread)
}

fn clear(client: &DapClient, place: &Where) {
    let removed = match place {
        Where::Line { path, .. } => client.request_raw(
            "setBreakpoints",
            json!({ "source": { "path": path }, "breakpoints": [] }),
            common::TIMEOUT,
        ),
        Where::Data { .. } => client.request_raw(
            "setDataBreakpoints",
            json!({ "breakpoints": [] }),
            common::TIMEOUT,
        ),
    };
    if let Err(error) = removed {
        println!("removing the breakpoint failed: {error}");
    }
}

fn wait_stopped(client: &DapClient, thread: u64) {
    let deadline = Instant::now() + common::TIMEOUT;
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        match client.next_event(left) {
            Some(event)
                if event.event == "stopped" && event.body["threadId"].as_u64() == Some(thread) =>
            {
                return;
            }
            Some(_) => {}
            None => return,
        }
    }
}

fn top_frame(client: &DapClient, thread: u64) -> Option<i64> {
    let trace = client
        .request_raw(
            "stackTrace",
            json!({ "threadId": thread, "levels": 1 }),
            common::TIMEOUT,
        )
        .ok()?;
    trace["stackFrames"].get(0)?["id"].as_i64()
}

/// `name = value` for each name, in the top frame of the stopped core.
fn show(client: &DapClient, thread: u64, names: &[String]) -> String {
    if names.is_empty() {
        return String::new();
    }
    let Some(frame) = top_frame(client, thread) else {
        return "(no frame)".into();
    };
    names
        .iter()
        .map(|name| {
            let value = client
                .request_raw(
                    "evaluate",
                    json!({ "expression": name, "frameId": frame, "context": "watch" }),
                    common::TIMEOUT,
                )
                .map(|answer| answer["result"].as_str().unwrap_or("?").to_string())
                .unwrap_or_else(|error| format!("<{error}>"));
            format!("{name} = {value}")
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// The value of a global variable on each core.
fn read(client: &DapClient, threads: &[(u64, String)], global: &str) -> HashMap<u64, String> {
    let mut values = HashMap::new();
    for (id, _) in threads {
        let answer = client.request::<LiveWatch>(
            LiveWatchArguments {
                expressions: vec![global.to_string()],
                core: Some(*id),
            },
            common::TIMEOUT,
        );
        if let Some(value) = answer
            .ok()
            .and_then(|answer| answer.values.into_iter().next())
            .and_then(|value| value.value)
        {
            values.insert(*id, value);
        }
    }
    values
}
