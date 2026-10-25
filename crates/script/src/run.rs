//! Runs a [`Script`] against the probe-rs DAP server.
//!
//! The first failed expectation ends the run (exit code 1) unless `keep_going` is set; any other
//! failure ends it at once (exit code 2): a mistake the parser could not see, the connection,
//! a time out. The session is ended with `disconnect` in every case, so that the server takes
//! the next one.

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use aurix_dap_client::{DapClient, Error as ClientError, Event};
use aurix_debug_model::breakpoints::{BreakpointSettings, InstructionBreakpoints};
use aurix_debug_model::memory::{decode_base64, encode_base64, writable_ram};
use aurix_debug_model::protocol::{
    LiveWatch, LiveWatchArguments, RTT_CHANNEL_CONFIG_EVENT, RTT_DATA_EVENT, RTT_PROMPT_EVENT,
    ReplCommand, RttChannelConfig, RttData, RttPrompt, RttWindowOpened,
};
use aurix_debug_model::rtt::{ChannelKey, RttChannels, SENT_MARK};
use aurix_debug_model::source_breakpoints::SourceBreakpoints;
use serde_json::{Value, json};

use crate::compare::Op;
use crate::script::{Access, Command, Cores, Line, Script, Step};

/// The server when the script does not name one.
pub const DEFAULT_SERVER: &str = "127.0.0.1:50000";
/// How long one request may take.
const TIMEOUT: Duration = Duration::from_secs(10);
/// How long `attach` may take (probe-rs reads the program and the debug information).
const ATTACH_TIMEOUT: Duration = Duration::from_secs(60);
/// How often `expect live` reads.
const LIVE_INTERVAL: Duration = Duration::from_millis(500);
/// How long an RTT command waits for the down channel to be announced.
const PROMPT_WAIT: Duration = Duration::from_secs(5);
/// How long `attach` tries to reach a server that is still ending the previous session.
const SERVER_BUSY: Duration = Duration::from_secs(10);
/// How long the end of a run waits for the server to close the connection.
const CLOSE_WAIT: Duration = Duration::from_secs(3);

#[derive(Clone, Debug, Default)]
pub struct Options {
    /// The server, unless the script names one.
    pub server: Option<String>,
    /// Run on after a failed expectation.
    pub keep_going: bool,
    /// Print the server's console messages too.
    pub verbose: bool,
    /// Set from outside (Ctrl+C): the run ends at the next chance.
    pub interrupted: Arc<AtomicBool>,
}

/// What one expectation found.
#[derive(Clone, Debug, PartialEq)]
pub struct CaseResult {
    pub line: usize,
    pub text: String,
    /// `None` when it passed, else what was found.
    pub failure: Option<String>,
    pub time: Duration,
}

/// Why the run ended early.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunError {
    /// The line, `None` for the end of the run (disconnect).
    pub line: Option<usize>,
    pub message: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Outcome {
    pub cases: Vec<CaseResult>,
    pub error: Option<RunError>,
    pub time: Duration,
}

impl Outcome {
    /// 0 passed, 1 an expectation failed, 2 the run could not go through.
    pub fn exit_code(&self) -> i32 {
        if self.error.is_some() {
            2
        } else if self.cases.iter().any(|c| c.failure.is_some()) {
            1
        } else {
            0
        }
    }
}

/// How a command ended, when not well.
enum Fail {
    /// An expectation did not hold: what was found.
    Expectation(String),
    /// Anything else.
    Error(String),
}

impl From<ClientError> for Fail {
    fn from(error: ClientError) -> Self {
        Fail::Error(error.to_string())
    }
}

/// A stop the server reported.
#[derive(Clone, Debug)]
struct Stop {
    thread: u64,
    reason: String,
    description: String,
}

struct Session<'a> {
    base_dir: PathBuf,
    options: &'a Options,
    out: &'a mut dyn Write,
    server: String,
    client: Option<DapClient>,
    threads: Vec<u64>,
    /// Stops not waited for yet, since the last run command.
    stops: VecDeque<Stop>,
    stopped: HashSet<u64>,
    last_stopped: Option<u64>,
    source_breakpoints: SourceBreakpoints,
    instruction_breakpoints: InstructionBreakpoints,
    data_breakpoints: Vec<Value>,
    rtt: RttChannels,
    /// Per RTT channel, the index (counting dropped lines) of the next line `rtt expect` reads.
    rtt_read: HashMap<ChannelKey, usize>,
}

/// Runs `script`, whose relative paths are relative to `base_dir`, printing what it does.
pub fn run(script: &Script, base_dir: &Path, options: &Options, out: &mut dyn Write) -> Outcome {
    let start = Instant::now();
    let mut session = Session {
        base_dir: base_dir.to_path_buf(),
        options,
        out,
        server: options
            .server
            .clone()
            .unwrap_or_else(|| DEFAULT_SERVER.to_string()),
        client: None,
        threads: Vec::new(),
        stops: VecDeque::new(),
        stopped: HashSet::new(),
        last_stopped: None,
        source_breakpoints: SourceBreakpoints::default(),
        instruction_breakpoints: InstructionBreakpoints::default(),
        data_breakpoints: Vec::new(),
        rtt: RttChannels::default(),
        rtt_read: HashMap::new(),
    };
    let mut outcome = Outcome::default();
    for line in &script.lines {
        if options.interrupted.load(Ordering::SeqCst) {
            outcome.error = Some(RunError {
                line: Some(line.number),
                message: "interrupted".into(),
            });
            break;
        }
        let started = Instant::now();
        let result = session.execute(line);
        let time = started.elapsed();
        match result {
            Ok(note) => {
                if line.command.is_expectation() {
                    outcome.cases.push(CaseResult {
                        line: line.number,
                        text: line.text.clone(),
                        failure: None,
                        time,
                    });
                }
                let note = note.map(|n| format!("  ({n})")).unwrap_or_default();
                let took = if time >= Duration::from_millis(100) {
                    format!("  [{:.2}s]", time.as_secs_f64())
                } else {
                    String::new()
                };
                session.say(&format!(
                    "{:>4} ok    {}{note}{took}",
                    line.number, line.text
                ));
            }
            Err(Fail::Expectation(found)) => {
                session.say(&format!("{:>4} FAIL  {}: {found}", line.number, line.text));
                outcome.cases.push(CaseResult {
                    line: line.number,
                    text: line.text.clone(),
                    failure: Some(found),
                    time,
                });
                if !options.keep_going {
                    break;
                }
            }
            Err(Fail::Error(message)) => {
                session.say(&format!(
                    "{:>4} ERROR {}: {message}",
                    line.number, line.text
                ));
                outcome.error = Some(RunError {
                    line: Some(line.number),
                    message,
                });
                break;
            }
        }
    }
    if let Err(message) = session.end() {
        session.say(&format!("     ERROR disconnect: {message}"));
        outcome.error.get_or_insert(RunError {
            line: None,
            message,
        });
    }
    outcome.time = start.elapsed();
    outcome
}

impl Session<'_> {
    fn say(&mut self, text: &str) {
        writeln!(self.out, "{text}").ok();
    }

    fn client(&self) -> Result<&DapClient, Fail> {
        self.client
            .as_ref()
            .ok_or_else(|| Fail::Error("no session (attach first, or after disconnect)".into()))
    }

    fn request(&self, command: &str, arguments: Value) -> Result<Value, Fail> {
        Ok(self.client()?.request_raw(command, arguments, TIMEOUT)?)
    }

    /// Takes the events that came, waiting at most `wait` for the first.
    fn pump(&mut self, wait: Duration) -> Result<(), Fail> {
        let first = match self.client()?.wait_event(wait) {
            Ok(event) => event,
            Err(ClientError::Closed) => {
                return Err(Fail::Error("the server closed the connection".into()));
            }
            Err(error) => return Err(error.into()),
        };
        let mut next = first;
        while let Some(event) = next {
            self.handle(event)?;
            next = self.client()?.try_event();
        }
        Ok(())
    }

    fn handle(&mut self, event: Event) -> Result<(), Fail> {
        let body = event.body;
        match event.event.as_str() {
            "stopped" => {
                let thread = body["threadId"].as_u64().unwrap_or(0);
                self.stopped.insert(thread);
                if body["allThreadsStopped"] == true {
                    self.stopped.extend(self.threads.iter().copied());
                }
                self.stops.push_back(Stop {
                    thread,
                    reason: body["reason"].as_str().unwrap_or_default().to_string(),
                    description: body["description"].as_str().unwrap_or_default().to_string(),
                });
            }
            "continued" => {
                if body["allThreadsContinued"] == true {
                    self.stopped.clear();
                } else if let Some(thread) = body["threadId"].as_u64() {
                    self.stopped.remove(&thread);
                }
            }
            "output" => {
                let program = body["category"] == "stdout";
                if program || self.options.verbose {
                    let text = body["output"].as_str().unwrap_or_default().to_string();
                    for line in text.lines() {
                        self.say(&format!("       | {line}"));
                    }
                }
            }
            RTT_CHANNEL_CONFIG_EVENT => {
                if let Ok(config) = serde_json::from_value::<RttChannelConfig>(body) {
                    let open = self.rtt.announced(config);
                    // The answer has no body to read.
                    self.request(
                        <RttWindowOpened as dap_types::requests::Request>::COMMAND,
                        serde_json::to_value(open).unwrap_or_default(),
                    )?;
                }
            }
            RTT_DATA_EVENT => {
                if let Ok(data) = serde_json::from_value::<RttData>(body) {
                    self.rtt.received(data);
                }
            }
            RTT_PROMPT_EVENT => {
                if let Ok(prompt) = serde_json::from_value::<RttPrompt>(body) {
                    self.rtt.prompted(prompt);
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Takes what came without waiting.
    fn drain(&mut self) -> Result<(), Fail> {
        while let Some(event) = self.client()?.try_event() {
            self.handle(event)?;
        }
        Ok(())
    }

    /// Waits `time`, taking the events meanwhile.
    fn pass_time(&mut self, time: Duration) -> Result<(), Fail> {
        let end = Instant::now() + time;
        while let Some(left) = end.checked_duration_since(Instant::now()) {
            self.check_interrupted()?;
            if self.client.is_some() {
                self.pump(left.min(Duration::from_millis(100)))?;
            } else {
                std::thread::sleep(left.min(Duration::from_millis(100)));
            }
        }
        Ok(())
    }

    fn check_interrupted(&self) -> Result<(), Fail> {
        if self.options.interrupted.load(Ordering::SeqCst) {
            Err(Fail::Error("interrupted".into()))
        } else {
            Ok(())
        }
    }

    fn path(&self, path: &str) -> PathBuf {
        let path = Path::new(path);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.base_dir.join(path)
        }
    }

    /// The core a command without one is for: the one that stopped last.
    fn stopped_core(&self, what: &str) -> Result<u64, Fail> {
        let core = self.last_stopped.ok_or_else(|| {
            Fail::Error(format!(
                "{what} needs a halted core: wait for a stop first, or name the core"
            ))
        })?;
        if !self.stopped.contains(&core) {
            return Err(Fail::Error(format!(
                "core {core} runs again since its last stop: `wait stopped` first"
            )));
        }
        Ok(core)
    }

    /// The top frame of a halted core.
    fn top_frame(&self, thread: u64) -> Result<i64, Fail> {
        let trace = self.request("stackTrace", json!({ "threadId": thread, "levels": 1 }))?;
        trace["stackFrames"]
            .get(0)
            .and_then(|frame| frame["id"].as_i64())
            .ok_or_else(|| Fail::Error(format!("core {thread} has no stack frame (is it halted?)")))
    }

    /// The RTT channel of `core` as the events name it (no core in a session of one).
    fn rtt_key(&self, core: u64, channel: u32) -> ChannelKey {
        if self.threads.len() > 1 {
            (Some(core), channel)
        } else {
            (None, channel)
        }
    }

    fn memory_reference(address: u64, core: Option<u64>) -> String {
        match core {
            Some(core) => format!("{address:#x}@{core}"),
            None => format!("{address:#x}"),
        }
    }

    fn execute(&mut self, line: &Line) -> Result<Option<String>, Fail> {
        if self.client.is_some() {
            self.drain()?;
        }
        match &line.command {
            Command::Server(server) => {
                self.server = server.clone();
                Ok(None)
            }
            Command::Attach(file) => self.attach(file),
            Command::Break {
                path,
                line,
                settings,
            } => self.set_break(path, *line, settings.clone()),
            Command::BreakAt(reference) => self.set_break_at(reference),
            Command::Watch {
                name,
                access,
                settings,
            } => self.set_watch(name, *access, settings),
            Command::Continue(cores) => self.run_cores("continue", *cores),
            Command::Pause(cores) => self.run_cores("pause", *cores),
            Command::Step(kind, core) => self.step(*kind, *core),
            Command::WaitStopped {
                core,
                reason,
                within,
            } => self.wait_stopped(*core, reason.as_deref(), *within),
            Command::Expect { name, op, value } => self.expect(name, *op, value),
            Command::ExpectLive {
                name,
                core,
                op,
                value,
                within,
            } => self.expect_live(name, *core, *op, value, *within),
            Command::ExpectMemory {
                address,
                core,
                bytes,
            } => self.expect_memory(*address, *core, bytes),
            Command::Write {
                address,
                core,
                bytes,
            } => self.write(*address, *core, bytes),
            Command::RttSend {
                core,
                channel,
                text,
            } => self.rtt_send(*core, *channel, text),
            Command::RttExpect {
                core,
                channel,
                pattern,
                within,
            } => self.rtt_expect(*core, *channel, pattern, *within),
            Command::Sleep(time) => {
                self.pass_time(*time)?;
                Ok(None)
            }
            Command::Disconnect => {
                self.end().map_err(Fail::Error)?;
                Ok(None)
            }
        }
    }

    fn attach(&mut self, file: &str) -> Result<Option<String>, Fail> {
        if self.client.is_some() {
            return Err(Fail::Error("already attached (disconnect first)".into()));
        }
        let path = self.path(file);
        let text = std::fs::read_to_string(&path)
            .map_err(|error| Fail::Error(format!("{}: {error}", path.display())))?;
        let arguments: Value = serde_json::from_str(&text)
            .map_err(|error| Fail::Error(format!("{}: {error}", path.display())))?;
        // A script never changes the flash.
        if arguments["flashingConfig"]["flashingEnabled"] == true {
            return Err(Fail::Error(
                "the attach file turns flashing on (flashingConfig.flashingEnabled): scripts do not write the flash"
                    .into(),
            ));
        }
        // The server listens anew after each session: right after one ended, a connection can be
        // refused or closed at once. Try again for a while.
        let end = Instant::now() + SERVER_BUSY;
        loop {
            let attempt = DapClient::connect(self.server.as_str()).and_then(|client| {
                client.request_raw(
                    "initialize",
                    json!({
                        "clientID": "aurix-run", "adapterID": "probe-rs", "linesStartAt1": true,
                        "columnsStartAt1": true, "pathFormat": "path", "supportsMemoryReferences": true,
                    }),
                    TIMEOUT,
                )?;
                Ok(client)
            });
            match attempt {
                Ok(client) => {
                    self.client = Some(client);
                    break;
                }
                Err(error @ (ClientError::Io(_) | ClientError::Closed)) => {
                    if Instant::now() >= end {
                        return Err(Fail::Error(format!(
                            "cannot reach {}: {error}",
                            self.server
                        )));
                    }
                    self.check_interrupted()?;
                    std::thread::sleep(Duration::from_millis(200));
                }
                Err(error) => return Err(error.into()),
            }
        }
        self.client()?
            .request_raw("attach", arguments, ATTACH_TIMEOUT)?;
        self.request("configurationDone", json!({}))?;
        let threads = self.request("threads", json!({}))?;
        self.threads = threads["threads"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|t| t["id"].as_u64())
            .collect();
        Ok(Some(format!(
            "{} core(s) on {}",
            self.threads.len(),
            self.server
        )))
    }

    fn set_break(
        &mut self,
        path: &str,
        line: u64,
        settings: BreakpointSettings,
    ) -> Result<Option<String>, Fail> {
        let resolved = self.path(path);
        // The debug information names the file by its full path.
        let path = std::fs::canonicalize(&resolved)
            .map(|p| {
                p.display()
                    .to_string()
                    .trim_start_matches(r"\\?\")
                    .to_string()
            })
            .unwrap_or_else(|_| resolved.display().to_string());
        self.source_breakpoints.set(&path, line, settings);
        let request = self.source_breakpoints.request(&path);
        let answer = self.request("setBreakpoints", serde_json::to_value(&request).unwrap())?;
        let index = request
            .breakpoints
            .iter()
            .flatten()
            .position(|bp| bp.line == line)
            .unwrap_or(0);
        let breakpoint = &answer["breakpoints"][index];
        if breakpoint["verified"] != true {
            self.source_breakpoints.toggle(&path, line);
            return Err(Fail::Error(format!(
                "not set: {}",
                breakpoint["message"].as_str().unwrap_or("no reason given")
            )));
        }
        Ok(breakpoint["instructionReference"]
            .as_str()
            .map(|address| format!("at {address}")))
    }

    fn set_break_at(&mut self, reference: &str) -> Result<Option<String>, Fail> {
        self.instruction_breakpoints
            .set(reference, BreakpointSettings::default());
        let request = self.instruction_breakpoints.request();
        let answer = self.request(
            "setInstructionBreakpoints",
            serde_json::to_value(&request).unwrap(),
        )?;
        let index = request
            .breakpoints
            .iter()
            .position(|bp| bp.instruction_reference == reference)
            .unwrap_or(0);
        let breakpoint = &answer["breakpoints"][index];
        if breakpoint["verified"] != true {
            self.instruction_breakpoints.toggle(reference);
            return Err(Fail::Error(format!(
                "not set: {}",
                breakpoint["message"].as_str().unwrap_or("no reason given")
            )));
        }
        Ok(None)
    }

    fn set_watch(
        &mut self,
        name: &str,
        access: Access,
        settings: &BreakpointSettings,
    ) -> Result<Option<String>, Fail> {
        let core = self.stopped_core("watch")?;
        let frame = self.top_frame(core)?;
        let info = self.request(
            "dataBreakpointInfo",
            json!({ "name": name, "frameId": frame }),
        )?;
        let Some(data_id) = info["dataId"].as_str() else {
            return Err(Fail::Error(
                info["description"]
                    .as_str()
                    .unwrap_or("no data ID")
                    .to_string(),
            ));
        };
        let mut breakpoint = json!({ "dataId": data_id, "accessType": access.dap() });
        if let Some(condition) = &settings.condition {
            breakpoint["condition"] = json!(condition);
        }
        if let Some(hit) = &settings.hit_condition {
            breakpoint["hitCondition"] = json!(hit);
        }
        self.data_breakpoints.push(breakpoint);
        let answer = self.request(
            "setDataBreakpoints",
            json!({ "breakpoints": self.data_breakpoints }),
        )?;
        let last = &answer["breakpoints"][self.data_breakpoints.len() - 1];
        if last["verified"] != true {
            self.data_breakpoints.pop();
            return Err(Fail::Error(format!(
                "not set: {}",
                last["message"].as_str().unwrap_or("no reason given")
            )));
        }
        Ok(Some(data_id.to_string()))
    }

    /// `continue` or `pause` of some cores.
    fn run_cores(&mut self, command: &str, cores: Cores) -> Result<Option<String>, Fail> {
        let (threads, any) = match cores {
            Cores::One(core) => (vec![core], false),
            Cores::All => (self.threads.clone(), true),
            Cores::Last => match self.last_stopped {
                Some(core) => (vec![core], false),
                None => (self.threads.clone(), true),
            },
        };
        if command == "continue" {
            // A stop from before this run is not one to wait for.
            self.stops.clear();
        }
        let mut done = Vec::new();
        for thread in threads {
            match self.request(command, json!({ "threadId": thread })) {
                Ok(_) => {
                    done.push(thread.to_string());
                    if command == "continue" {
                        self.stopped.remove(&thread);
                    }
                }
                // A core that runs already (continue) or is halted already (pause).
                Err(_) if any => {}
                Err(error) => return Err(error),
            }
        }
        Ok(Some(format!("core {}", done.join(", "))))
    }

    fn step(&mut self, kind: Step, core: Option<u64>) -> Result<Option<String>, Fail> {
        let thread = match core {
            Some(core) => core,
            None => self.stopped_core("step")?,
        };
        self.stops.clear();
        let (command, mut arguments) = match kind {
            Step::Over => ("next", json!({ "threadId": thread })),
            Step::Into => ("stepIn", json!({ "threadId": thread })),
            Step::Out => ("stepOut", json!({ "threadId": thread })),
            Step::Instruction => ("next", json!({ "threadId": thread })),
        };
        if kind == Step::Instruction {
            arguments["granularity"] = json!("instruction");
        }
        self.request(command, arguments)?;
        self.stopped.remove(&thread);
        Ok(None)
    }

    fn wait_stopped(
        &mut self,
        core: Option<u64>,
        reason: Option<&str>,
        within: Duration,
    ) -> Result<Option<String>, Fail> {
        let end = Instant::now() + within;
        loop {
            let found = self.stops.iter().position(|stop| {
                core.is_none_or(|core| stop.thread == core)
                    && reason.is_none_or(|reason| stop.reason == reason)
            });
            if let Some(at) = found {
                let stop = self.stops.remove(at).unwrap_or_else(|| unreachable!());
                self.last_stopped = Some(stop.thread);
                return Ok(Some(format!(
                    "core {}: {}",
                    stop.thread,
                    if stop.description.is_empty() {
                        &stop.reason
                    } else {
                        &stop.description
                    }
                )));
            }
            self.check_interrupted()?;
            let Some(left) = end.checked_duration_since(Instant::now()) else {
                let seen = self
                    .stops
                    .iter()
                    .map(|s| format!("core {} {}", s.thread, s.reason))
                    .collect::<Vec<_>>();
                let seen = if seen.is_empty() {
                    "no other stop".to_string()
                } else {
                    format!("stops meanwhile: {}", seen.join(", "))
                };
                return Err(Fail::Error(format!(
                    "no such stop within {:.1}s ({seen})",
                    within.as_secs_f64()
                )));
            };
            self.pump(left.min(Duration::from_millis(100)))?;
        }
    }

    fn expect(&mut self, name: &str, op: Op, value: &str) -> Result<Option<String>, Fail> {
        let core = self.stopped_core("expect")?;
        let frame = self.top_frame(core)?;
        let actual = match self.request(
            "evaluate",
            json!({ "expression": name, "frameId": frame, "context": "watch" }),
        ) {
            Ok(answer) => answer["result"].as_str().unwrap_or_default().to_string(),
            Err(Fail::Error(error)) => {
                return Err(Fail::Expectation(format!("{name} cannot be read: {error}")));
            }
            Err(other) => return Err(other),
        };
        if actual.starts_with('<') {
            return Err(Fail::Expectation(format!("{name} is {actual}")));
        }
        if op.holds(&actual, value) {
            Ok(Some(format!("{name} = {actual}")))
        } else {
            Err(Fail::Expectation(format!(
                "expected {name} {op} {value}, found {actual}"
            )))
        }
    }

    fn expect_live(
        &mut self,
        name: &str,
        core: Option<u64>,
        op: Op,
        value: &str,
        within: Duration,
    ) -> Result<Option<String>, Fail> {
        let core = core
            .or(self.last_stopped)
            .or_else(|| self.threads.first().copied())
            .ok_or_else(|| Fail::Error("no core".into()))?;
        let end = Instant::now() + within;
        loop {
            let answer = self.client()?.request::<LiveWatch>(
                LiveWatchArguments {
                    expressions: vec![name.to_string()],
                    core: Some(core),
                },
                TIMEOUT,
            )?;
            let read = answer.values.into_iter().next();
            let actual = match read {
                Some(read) => match read.value {
                    Some(value) => Ok(value),
                    None => Err(read.error.unwrap_or_else(|| "no value".into())),
                },
                None => Err("no value".into()),
            };
            match &actual {
                Ok(actual) if op.holds(actual, value) => {
                    return Ok(Some(format!("{name} = {actual} on core {core}")));
                }
                _ => {}
            }
            if Instant::now() + LIVE_INTERVAL > end {
                return Err(Fail::Expectation(match actual {
                    Ok(actual) => format!(
                        "expected {name} {op} {value} within {:.1}s, last read {actual}",
                        within.as_secs_f64()
                    ),
                    Err(error) => format!("{name} cannot be read: {error}"),
                }));
            }
            self.pass_time(LIVE_INTERVAL)?;
        }
    }

    fn expect_memory(
        &mut self,
        address: u64,
        core: Option<u64>,
        bytes: &[u8],
    ) -> Result<Option<String>, Fail> {
        let answer = self.request(
            "readMemory",
            json!({
                "memoryReference": Self::memory_reference(address, core),
                "offset": 0,
                "count": bytes.len(),
            }),
        )?;
        let read = answer["data"]
            .as_str()
            .map(decode_base64)
            .transpose()
            .map_err(Fail::Error)?
            .unwrap_or_default();
        let hex = |bytes: &[u8]| {
            bytes
                .iter()
                .map(|b| format!("{b:02X}"))
                .collect::<Vec<_>>()
                .join(" ")
        };
        if read == bytes {
            Ok(None)
        } else {
            Err(Fail::Expectation(format!(
                "expected {}, found {}",
                hex(bytes),
                if read.is_empty() {
                    "nothing readable".to_string()
                } else {
                    hex(&read)
                }
            )))
        }
    }

    fn write(
        &mut self,
        address: u64,
        core: Option<u64>,
        bytes: &[u8],
    ) -> Result<Option<String>, Fail> {
        let ram = writable_ram(address, bytes.len(), core).map_err(Fail::Error)?;
        self.request(
            "writeMemory",
            json!({
                "memoryReference": Self::memory_reference(address, core),
                "offset": 0,
                "data": encode_base64(bytes),
            }),
        )?;
        Ok(Some(format!("{} byte(s) to {ram}", bytes.len())))
    }

    fn rtt_send(&mut self, core: u64, channel: u32, text: &str) -> Result<Option<String>, Fail> {
        let key = self.rtt_key(core, channel);
        let end = Instant::now() + PROMPT_WAIT;
        while self.rtt.down_for(key).is_none() {
            if Instant::now() >= end {
                return Err(Fail::Error(format!(
                    "core {core} has no RTT down channel (is rttEnabled set, does the program have one?)"
                )));
            }
            self.pump(Duration::from_millis(100))?;
        }
        let Some(command) = self.rtt.send(key, text) else {
            return Err(Fail::Error("nothing to send".into()));
        };
        let answer = self.client()?.request::<ReplCommand>(command, TIMEOUT)?;
        if answer.result.trim().is_empty() {
            Ok(None)
        } else {
            Err(Fail::Error(answer.result))
        }
    }

    fn rtt_expect(
        &mut self,
        core: u64,
        channel: u32,
        pattern: &str,
        within: Duration,
    ) -> Result<Option<String>, Fail> {
        let regex = regex::Regex::new(pattern).map_err(|error| Fail::Error(error.to_string()))?;
        let key = self.rtt_key(core, channel);
        let end = Instant::now() + within;
        loop {
            if let Some(channel) = self.rtt.get(key) {
                let first = channel.dropped;
                let from = self.rtt_read.get(&key).copied().unwrap_or(0).max(first);
                let found = channel
                    .lines
                    .iter()
                    .enumerate()
                    .skip(from - first)
                    .find(|(_, line)| !line.starts_with(SENT_MARK) && regex.is_match(line))
                    .map(|(ix, line)| (first + ix, line.clone()));
                if let Some((index, line)) = found {
                    self.rtt_read.insert(key, index + 1);
                    return Ok(Some(line));
                }
            }
            self.check_interrupted()?;
            let Some(left) = end.checked_duration_since(Instant::now()) else {
                let last = self
                    .rtt
                    .get(key)
                    .and_then(|c| c.lines.iter().rev().find(|l| !l.starts_with(SENT_MARK)))
                    .cloned();
                return Err(Fail::Expectation(match last {
                    Some(last) => format!(
                        "no line matching within {:.1}s, last line {last:?}",
                        within.as_secs_f64()
                    ),
                    None => format!(
                        "no line on core {core} channel {channel} within {:.1}s",
                        within.as_secs_f64()
                    ),
                }));
            };
            self.pump(left.min(Duration::from_millis(100)))?;
        }
    }

    /// Ends the session, if there is one.
    fn end(&mut self) -> Result<(), String> {
        let Some(client) = self.client.take() else {
            return Ok(());
        };
        self.stops.clear();
        self.stopped.clear();
        self.last_stopped = None;
        match client.request_raw(
            "disconnect",
            json!({ "terminateDebuggee": false }),
            Duration::from_secs(5),
        ) {
            // A connection that is gone already: the server ended the session itself.
            Ok(_) | Err(ClientError::Closed | ClientError::Io(_)) => {}
            Err(error) => return Err(error.to_string()),
        }
        // The server closes the connection once the session is over; the next run then finds
        // it listening sooner.
        let end = Instant::now() + CLOSE_WAIT;
        while let Some(left) = end.checked_duration_since(Instant::now()) {
            if client.wait_event(left).is_err() {
                break;
            }
        }
        Ok(())
    }
}
