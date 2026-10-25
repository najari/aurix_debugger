//! The runner against a fake adapter on a local port: one core that stops at a breakpoint when
//! continued, a few variables, memory, a live global and an RTT channel that answers.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use aurix_debug_model::memory::encode_base64;
use aurix_script::run::{Options, Outcome, run};
use aurix_script::script::Script;
use serde_json::{Value, json};

fn read_message(reader: &mut impl BufRead) -> Option<Value> {
    let mut length = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).ok()? == 0 {
            return None;
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some(value) = line.strip_prefix("Content-Length:") {
            length = value.trim().parse::<usize>().ok();
        }
    }
    let mut body = vec![0; length?];
    reader.read_exact(&mut body).ok()?;
    serde_json::from_slice(&body).ok()
}

fn write_message(stream: &Mutex<TcpStream>, message: &Value) {
    let body = serde_json::to_vec(message).unwrap();
    let mut stream = stream.lock().unwrap();
    let _ = stream.write_all(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes());
    let _ = stream.write_all(&body);
}

/// What the fake adapter saw.
#[derive(Default)]
struct Seen {
    commands: Vec<String>,
    disconnects: usize,
}

struct Fake {
    address: String,
    seen: Arc<Mutex<Seen>>,
}

/// A fake adapter; `close_after` closes the connection after that many requests.
fn fake(close_after: Option<usize>) -> Fake {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let seen = Arc::new(Mutex::new(Seen::default()));
    let record = seen.clone();
    std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let writer = Mutex::new(stream.try_clone().unwrap());
        let mut reader = BufReader::new(stream);
        let seq = AtomicUsize::new(1000);
        let ticks = AtomicUsize::new(0);
        let event = |name: &str, body: Value| {
            write_message(
                &writer,
                &json!({ "seq": seq.fetch_add(1, Ordering::SeqCst), "type": "event",
                         "event": name, "body": body }),
            );
        };
        let mut count = 0;
        while let Some(request) = read_message(&mut reader) {
            count += 1;
            if close_after.is_some_and(|n| count > n) {
                return;
            }
            let command = request["command"].as_str().unwrap_or_default().to_string();
            let arguments = request["arguments"].clone();
            record.lock().unwrap().commands.push(command.clone());
            let (success, body) = match command.as_str() {
                "threads" => (true, json!({ "threads": [{ "id": 0, "name": "0-TC275" }] })),
                "setBreakpoints" => {
                    let breakpoints: Vec<Value> = arguments["breakpoints"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|bp| {
                            let bad = bp["condition"].as_str().is_some_and(|c| c.ends_with("=="));
                            if bad {
                                json!({ "verified": false, "message": "Breakpoint not set: `v ==` needs a value after ==" })
                            } else {
                                json!({ "verified": true, "line": bp["line"], "instructionReference": "0x8021C028" })
                            }
                        })
                        .collect();
                    (true, json!({ "breakpoints": breakpoints }))
                }
                "stackTrace" => (
                    true,
                    json!({ "stackFrames": [{ "id": 7, "name": "scale", "line": 23, "column": 1 }] }),
                ),
                "evaluate" if arguments["context"] == "repl" => {
                    let expression = arguments["expression"].as_str().unwrap_or_default();
                    let text = expression.strip_prefix("rtt write 0 ").unwrap_or_default();
                    (
                        true,
                        json!({ "result": "", "variablesReference": 0, "_echo": text }),
                    )
                }
                "evaluate" => {
                    let values = HashMap::from([("k", "3"), ("v", "2"), ("s", "4")]);
                    let name = arguments["expression"].as_str().unwrap_or_default();
                    let result = values
                        .get(name)
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| format!("<invalid expression {name:?}>"));
                    (true, json!({ "result": result, "variablesReference": 0 }))
                }
                "readMemory" => (
                    true,
                    json!({ "address": "0x7000A004", "data": encode_base64(&[0x2e, 0, 0, 0]) }),
                ),
                "liveWatch" => {
                    let value = ticks.fetch_add(40, Ordering::SeqCst);
                    (
                        true,
                        json!({ "values": [{ "expression": "g_ticks", "value": value.to_string() }] }),
                    )
                }
                "pause" => (false, json!({})),
                "disconnect" => {
                    record.lock().unwrap().disconnects += 1;
                    (true, json!({}))
                }
                _ => (true, json!({})),
            };
            let mut response = json!({ "seq": seq.fetch_add(1, Ordering::SeqCst), "type": "response",
                "request_seq": request["seq"], "command": command, "success": success, "body": body });
            if !success {
                response["message"] = json!("not now");
            }
            write_message(&writer, &response);
            match command.as_str() {
                "configurationDone" => {
                    event(
                        "probe-rs-rtt-channel-config",
                        json!({ "channelNumber": 0, "channelName": "Terminal", "dataFormat": "String" }),
                    );
                    event(
                        "probe-rs-create-prompt",
                        json!({ "promptKind": "rtt", "promptName": "Input", "promptHandle": 0 }),
                    );
                }
                "continue" => event(
                    "stopped",
                    json!({ "reason": "breakpoint", "threadId": 0,
                    "description": "Stopped at a hardware breakpoint", "allThreadsStopped": true }),
                ),
                "next" => event(
                    "stopped",
                    json!({ "reason": "step", "threadId": 0, "allThreadsStopped": true }),
                ),
                "evaluate" if arguments["context"] == "repl" => {
                    let text = arguments["expression"]
                        .as_str()
                        .unwrap_or_default()
                        .trim_start_matches("rtt write 0 ");
                    event(
                        "probe-rs-rtt-data",
                        json!({ "channelNumber": 0, "data": format!("rtt_demo got: {text}\n") }),
                    );
                }
                "disconnect" => return,
                _ => {}
            }
        }
    });
    Fake { address, seen }
}

fn attach_file() -> tempfile_path::Path {
    tempfile_path::write("{ \"request\": \"attach\", \"chip\": \"TC275\" }")
}

/// A file in the temporary folder, removed afterwards.
mod tempfile_path {
    pub struct Path(pub std::path::PathBuf);
    impl Drop for Path {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    pub fn write(text: &str) -> Path {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static N: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "aurix-run-test-{}-{}.json",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::write(&path, text).unwrap();
        Path(path)
    }
}

fn run_script(fake: &Fake, body: &str, keep_going: bool) -> (Outcome, String) {
    let attach = attach_file();
    let text = format!("attach {}\n{body}", attach.0.display());
    let script = Script::parse(&text, &HashMap::new()).unwrap();
    let options = Options {
        server: Some(fake.address.clone()),
        keep_going,
        ..Options::default()
    };
    let mut out = Vec::new();
    let outcome = run(&script, std::path::Path::new("."), &options, &mut out);
    (outcome, String::from_utf8(out).unwrap())
}

#[test]
fn a_script_that_holds_passes() {
    let fake = fake(None);
    let (outcome, out) = run_script(
        &fake,
        "break demo.c:23 if v == 3\ncontinue\nwait stopped 0 reason breakpoint\nexpect k == 3\nexpect v < 4\n\
         expect memory 0x7000A004@0 2E 00 00 00\nwrite 0x7000A040@0 DE AD\nexpect live g_ticks > 100 within 3s\n\
         step over\nwait stopped reason step within 2s\nexpect s == 4\n",
        false,
    );
    assert_eq!(outcome.exit_code(), 0, "{out}");
    assert_eq!(outcome.cases.len(), 5);
    let seen = fake.seen.lock().unwrap();
    assert_eq!(seen.disconnects, 1);
    assert!(seen.commands.contains(&"writeMemory".to_string()));
    assert!(seen.commands.contains(&"rttWindowOpened".to_string()));
}

#[test]
fn a_failed_expectation_stops_the_run_with_the_value_found() {
    let fake = fake(None);
    let (outcome, out) = run_script(
        &fake,
        "continue\nwait stopped 0\nexpect k == 4\nexpect v < 4\n",
        false,
    );
    assert_eq!(outcome.exit_code(), 1);
    assert_eq!(outcome.cases.len(), 1);
    assert!(
        out.contains("   4 FAIL  expect k == 4: expected k == 4, found 3"),
        "{out}"
    );
    assert_eq!(fake.seen.lock().unwrap().disconnects, 1);
}

#[test]
fn keep_going_runs_every_expectation() {
    let fake = fake(None);
    let (outcome, _) = run_script(
        &fake,
        "continue\nwait stopped 0\nexpect k == 4\nexpect nosuch == 1\nexpect v < 4\n",
        true,
    );
    assert_eq!(outcome.exit_code(), 1);
    let failed: Vec<usize> = outcome
        .cases
        .iter()
        .filter(|c| c.failure.is_some())
        .map(|c| c.line)
        .collect();
    assert_eq!(failed, vec![4, 5]);
    assert_eq!(outcome.cases.len(), 3);
}

#[test]
fn errors_end_the_run_with_2() {
    // A refused breakpoint.
    let fake_a = fake(None);
    let (outcome, out) = run_script(&fake_a, "break demo.c:23 if v ==\n", false);
    assert_eq!(outcome.exit_code(), 2, "{out}");
    assert!(out.contains("needs a value after =="), "{out}");
    // A stop that does not come.
    let fake_b = fake(None);
    let (outcome, _) = run_script(&fake_b, "wait stopped 0 within 300ms\n", false);
    assert_eq!(outcome.exit_code(), 2);
    // An expectation on a core that runs.
    let fake_c = fake(None);
    let (outcome, out) = run_script(&fake_c, "expect k == 3\n", false);
    assert_eq!(outcome.exit_code(), 2);
    assert!(out.contains("halted core"), "{out}");
    // RAM only.
    let fake_d = fake(None);
    let (outcome, out) = run_script(&fake_d, "write 0x8021C000 00\n", false);
    assert_eq!(outcome.exit_code(), 2);
    assert!(out.contains("flash"), "{out}");
    assert!(
        !fake_d
            .seen
            .lock()
            .unwrap()
            .commands
            .contains(&"writeMemory".to_string())
    );
}

#[test]
fn a_closed_connection_ends_the_run_at_once() {
    // initialize, attach, configurationDone, threads, then the connection closes.
    let fake = fake(Some(4));
    let start = Instant::now();
    let (outcome, out) = run_script(&fake, "wait stopped 0 within 30s\n", false);
    assert_eq!(outcome.exit_code(), 2, "{out}");
    assert!(start.elapsed() < Duration::from_secs(5), "{out}");
}

#[test]
fn rtt_lines_go_out_and_come_back() {
    let fake = fake(None);
    let (outcome, out) = run_script(
        &fake,
        "rtt send 0 0 ping\nrtt expect 0 0 \"got: ping\" within 2s\nrtt expect 0 0 \"got: ping\" within 300ms\n",
        true,
    );
    // The second expect finds no new line: the one line was read by the first.
    assert_eq!(outcome.exit_code(), 1, "{out}");
    assert!(outcome.cases[0].failure.is_none(), "{out}");
    assert!(outcome.cases[1].failure.is_some(), "{out}");
}

#[test]
fn a_flashing_attach_file_is_refused() {
    let fake = fake(None);
    let attach = tempfile_path::write(
        "{ \"request\": \"launch\", \"flashingConfig\": { \"flashingEnabled\": true } }",
    );
    let script =
        Script::parse(&format!("attach {}\n", attach.0.display()), &HashMap::new()).unwrap();
    let options = Options {
        server: Some(fake.address.clone()),
        ..Options::default()
    };
    let mut out = Vec::new();
    let outcome = run(&script, std::path::Path::new("."), &options, &mut out);
    assert_eq!(outcome.exit_code(), 2);
    assert!(fake.seen.lock().unwrap().commands.is_empty());
}
