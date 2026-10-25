//! The debugger window: a session with the probe-rs DAP server and its views.
//!
//! Requests run on GPUI's background executor (the DAP client blocks until the answer) and
//! their answers come back to the window's entity. The adapter's events are taken every
//! [`EVENT_POLL`]. The state and rules of the views (disassembly window, instruction
//! breakpoints, RTT channels) are the shared `aurix-debug-model` crate's.

use std::ops::Range;
use std::sync::Arc;
use std::time::{Duration, Instant};

use aurix_dap_client::{DapClient, Event};
use aurix_debug_model::breakpoints::InstructionBreakpoints;
use aurix_debug_model::disassembly::{BEFORE, COUNT, DisassemblyWindow, MORE, Side, is_filler};
use aurix_debug_model::protocol::{
    DisassembleSigned, RTT_CHANNEL_CONFIG_EVENT, RTT_DATA_EVENT, RTT_PROMPT_EVENT,
    RttChannelConfig, RttData, RttPrompt, RttWindowOpened,
};
use aurix_debug_model::reference::parse_address;
use aurix_debug_model::rtt::RttChannels;
use aurix_debug_model::source_breakpoints::SourceBreakpoints;
use aurix_debug_model::variables::{Row, Variables};
use dap_types::requests::{
    Request, Scopes, SetBreakpoints, SetInstructionBreakpoints, StackTrace, Threads,
    Variables as VariablesRequest,
};
use dap_types::{StackFrame, StackTraceArguments, ThreadsArgument, VariablesArguments};
use gpui::{
    AnyElement, ClickEvent, Context, Hsla, ScrollStrategy, SharedString, UniformListScrollHandle,
    Window, div, prelude::*, px, rgb, uniform_list,
};
use serde_json::{Value, json};

use crate::source::{SourceFile, SourceFiles};

mod breakpoint_edit;
mod cache;
mod connect;
mod gtm;
mod memory;
mod os;
mod panels;
mod splitters;

use crate::layout::{Drag, Layout, Splitter};

use crate::text_input::{Submitted, TextInput};
use aurix_debug_model::live_watch::LiveWatch;
use aurix_debug_model::memory::MemoryView;
use gpui::Entity;
use panels::BottomTab;

/// How long a request may take.
const TIMEOUT: Duration = Duration::from_secs(10);
/// How often the adapter's events are taken.
const EVENT_POLL: Duration = Duration::from_millis(30);
/// The console keeps at most this many lines.
const MAX_CONSOLE_LINES: usize = 5_000;
/// The same console line twice within this is shown once.
const REPEAT_WINDOW: Duration = Duration::from_millis(500);
/// What the source view shows while no frame is selected.
const NOT_STOPPED: &str = "Not stopped";

mod color {
    pub const BACKGROUND: u32 = 0x1e1e1e;
    pub const PANEL: u32 = 0x252526;
    pub const BORDER: u32 = 0x3c3c3c;
    pub const TEXT: u32 = 0xd4d4d4;
    pub const MUTED: u32 = 0x8a8a8a;
    pub const ACCENT: u32 = 0x569cd6;
    pub const SELECTED: u32 = 0x37373d;
    pub const PC_LINE: u32 = 0x4b4b18;
    pub const PC_MARK: u32 = 0xffcc00;
    pub const BREAKPOINT: u32 = 0xe51400;
    pub const BUTTON: u32 = 0x3a3d41;
    pub const BUTTON_HOVER: u32 = 0x45494e;
    pub const CHANGED: u32 = 0xf48771;
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ThreadState {
    Unknown,
    Running,
    Stopped(String),
}

#[derive(Clone, Debug)]
struct ThreadRow {
    id: i64,
    name: String,
    state: ThreadState,
}

pub struct Debugger {
    server: String,
    attach: Option<String>,
    client: Option<Arc<DapClient>>,
    status: SharedString,
    threads: Vec<ThreadRow>,
    selected_thread: Option<i64>,
    frames: Vec<StackFrame>,
    selected_frame: usize,
    disassembly: DisassemblyWindow,
    disassembly_scroll: UniformListScrollHandle,
    /// Scroll the disassembly to the program counter at the next render.
    scroll_to_pc: bool,
    breakpoints: InstructionBreakpoints,
    source_files: SourceFiles,
    /// The source file of the selected frame, or why there is none.
    source: Result<Arc<SourceFile>, String>,
    source_scroll: UniformListScrollHandle,
    /// Scroll the source to the selected frame's line at the next render.
    scroll_to_line: bool,
    source_breakpoints: SourceBreakpoints,
    /// The edit row of a source breakpoint (condition, hit count, log message).
    breakpoint_edit: breakpoint_edit::BreakpointEdit,
    breakpoint_editing: bool,
    variables: Variables,
    rtt: RttChannels,
    rtt_scroll: UniformListScrollHandle,
    /// Where a line for the shown RTT tab's down channel is typed.
    rtt_input: Entity<TextInput>,
    /// Why the last line was not sent.
    rtt_error: Option<String>,
    live_watch: LiveWatch,
    /// Where a Live Watch expression is typed.
    watch_input: Entity<TextInput>,
    /// Where the Live Watch expressions are kept: next to the attach file.
    watch_file: Option<std::path::PathBuf>,
    memory: MemoryView,
    /// Where an address for the memory tab is typed.
    memory_input: Entity<TextInput>,
    memory_input_error: Option<String>,
    memory_paused: bool,
    /// The address asked for, marked in the memory tab; writes go there.
    memory_focus: Option<u64>,
    /// Where bytes to write at `memory_focus` are typed.
    memory_write_input: Entity<TextInput>,
    /// What the last write did, or why it was refused.
    memory_write_note: Option<Result<String, String>>,
    /// The OS objects of the ORTI file, and their values.
    os: Option<aurix_debug_model::orti::OsView>,
    os_file: Option<std::path::PathBuf>,
    /// Why the ORTI file could not be read.
    os_note: Option<String>,
    /// A read of the OS attributes is under way.
    os_busy: bool,
    /// The GTM's MCS channels, for a chip whose GTM is known.
    gtm: Option<aurix_debug_model::gtm::GtmView>,
    /// A read of the MCS channels is under way.
    gtm_busy: bool,
    /// The cores' caches, for a chip whose caches are known.
    cache: Option<aurix_debug_model::cache::CacheView>,
    /// A read of the cache registers is under way.
    cache_busy: bool,
    bottom: BottomTab,
    console: Vec<SharedString>,
    /// When the last console line came.
    last_log: Option<Instant>,
    console_scroll: UniformListScrollHandle,
    /// Counts the sessions: the loops and the answers of an ended session stop there.
    session: u64,
    connecting: bool,
    /// The attach files used before, newest first.
    recent: Vec<String>,
    server_input: Entity<TextInput>,
    connect_error: Option<String>,
    layout: Layout,
    /// A splitter being dragged.
    drag: Option<Drag>,
}

impl Debugger {
    pub fn new(attach: Option<String>, server: String, cx: &mut Context<Self>) -> Self {
        let server_input = cx.new(|cx| {
            let mut input = TextInput::new("Server, e.g. 127.0.0.1:50000", cx);
            input.set_text(server.clone(), cx);
            input
        });
        let mut this = Self {
            server,
            attach: None,
            client: None,
            status: "Not connected".into(),
            threads: Vec::new(),
            selected_thread: None,
            frames: Vec::new(),
            selected_frame: 0,
            disassembly: DisassemblyWindow::default(),
            disassembly_scroll: UniformListScrollHandle::new(),
            scroll_to_pc: false,
            breakpoints: InstructionBreakpoints::default(),
            source_files: SourceFiles::default(),
            source: Err(NOT_STOPPED.into()),
            source_scroll: UniformListScrollHandle::new(),
            scroll_to_line: false,
            source_breakpoints: SourceBreakpoints::default(),
            breakpoint_edit: breakpoint_edit::BreakpointEdit::new(cx),
            breakpoint_editing: false,
            variables: Variables::default(),
            rtt: RttChannels::default(),
            rtt_scroll: UniformListScrollHandle::new(),
            rtt_input: cx.new(|cx| TextInput::new("A line for the target, Enter", cx)),
            rtt_error: None,
            live_watch: LiveWatch::default(),
            watch_input: cx
                .new(|cx| TextInput::new("Add an expression (a global variable), Enter", cx)),
            watch_file: None,
            memory: MemoryView::default(),
            memory_input: cx.new(|cx| TextInput::new("Address, e.g. 0x7000A000@1", cx)),
            memory_input_error: None,
            memory_paused: false,
            memory_focus: None,
            memory_write_input: cx.new(|cx| TextInput::new("Hex bytes, e.g. 2A 00", cx)),
            memory_write_note: None,
            os: None,
            os_file: None,
            os_note: None,
            os_busy: false,
            gtm: None,
            gtm_busy: false,
            cache: None,
            cache_busy: false,
            bottom: BottomTab::Console,
            console: Vec::new(),
            last_log: None,
            console_scroll: UniformListScrollHandle::new(),
            session: 0,
            connecting: false,
            recent: crate::recent::load(),
            server_input,
            connect_error: None,
            layout: crate::layout::load(),
            drag: None,
        };
        cx.subscribe(&this.watch_input, |this, _, Submitted(expression), cx| {
            this.add_watch(expression);
            cx.notify();
        })
        .detach();
        cx.subscribe(&this.memory_input, |this, _, Submitted(text), cx| {
            this.memory_submitted(text);
            cx.notify();
        })
        .detach();
        cx.subscribe(&this.rtt_input, |this, _, Submitted(text), cx| {
            this.rtt_send(text, cx);
            cx.notify();
        })
        .detach();
        cx.subscribe(&this.memory_write_input, |this, _, Submitted(text), cx| {
            this.memory_write(text, cx);
            cx.notify();
        })
        .detach();
        if let Some(attach) = attach {
            this.start(attach, cx);
        }
        this
    }

    /// Ends the session (the window closes), so that the server takes the next one. The target
    /// is left as it is.
    pub fn disconnect(&mut self) {
        if let Some(client) = self.client.take() {
            client
                .request_raw(
                    "disconnect",
                    json!({ "terminateDebuggee": false }),
                    Duration::from_secs(2),
                )
                .ok();
        }
    }

    fn log(&mut self, line: impl Into<SharedString>) {
        let line = line.into();
        // A stop's description often comes as an output event as well, at once; the same stop
        // again later is a line of its own.
        let now = Instant::now();
        let repeated = self.console.last() == Some(&line)
            && self
                .last_log
                .is_some_and(|last| now.duration_since(last) < REPEAT_WINDOW);
        self.last_log = Some(now);
        if repeated {
            return;
        }
        self.push_console(line);
    }

    /// Adds a line to the console as it is (program output: each line counts).
    fn push_console(&mut self, line: SharedString) {
        self.console.push(line);
        if self.console.len() > MAX_CONSOLE_LINES {
            self.console.drain(..self.console.len() - MAX_CONSOLE_LINES);
        }
        self.console_scroll
            .scroll_to_item(self.console.len().saturating_sub(1), ScrollStrategy::Bottom);
    }

    /// Connects, attaches and ends the configuration, then reads the threads and takes the
    /// adapter's events.
    fn connect(&mut self, cx: &mut Context<Self>) {
        let Some(attach) = self.attach.clone() else {
            return;
        };
        let server = self.server.clone();
        let session = self.session;
        self.connecting = true;
        self.connect_error = None;
        self.status = format!("Connecting to {server}…").into();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { start_session(&server, &attach) })
                .await;
            this.update(cx, |this, cx| {
                if this.session != session {
                    // Given up meanwhile: end the session that came too late.
                    if let Ok(client) = result {
                        end_in_background(client, cx);
                    }
                    return;
                }
                this.connecting = false;
                match result {
                    Ok(client) => {
                        this.status = format!("Attached ({})", this.server).into();
                        this.client = Some(client);
                        this.take_events(cx);
                        this.read_threads(cx);
                        this.poll_live_watch(cx);
                        this.resend_breakpoints(cx);
                    }
                    Err(error) => {
                        this.status = "Not attached".into();
                        this.connect_error = Some(error.clone());
                        this.log(format!("error: {error}"));
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Sends request `R` on the background executor; `done` gets the answer on the window's
    /// entity.
    fn request<R>(
        &self,
        arguments: R::Arguments,
        cx: &mut Context<Self>,
        done: impl FnOnce(&mut Self, Result<R::Response, String>, &mut Context<Self>) + 'static,
    ) where
        R: Request + 'static,
    {
        let Some(client) = self.client.clone() else {
            return;
        };
        let session = self.session;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    client
                        .request::<R>(arguments, TIMEOUT)
                        .map_err(|error| error.to_string())
                })
                .await;
            this.update(cx, |this, cx| {
                // The answer of an ended session is not about what is shown now.
                if this.session == session {
                    done(this, result, cx);
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// Sends a request without a typed answer (execution control); a failure goes to the
    /// console.
    fn request_raw(&self, command: &'static str, arguments: Value, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { client.request_raw(command, arguments, TIMEOUT) })
                .await;
            if let Err(error) = result {
                this.update(cx, |this, cx| {
                    this.log(format!("{command}: {error}"));
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    fn take_events(&mut self, cx: &mut Context<Self>) {
        let session = self.session;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(EVENT_POLL).await;
                let alive = this
                    .update(cx, |this, cx| {
                        let Some(client) = this.client.clone().filter(|_| this.session == session)
                        else {
                            return false;
                        };
                        let mut any = false;
                        while let Some(event) = client.try_event() {
                            this.handle_event(event, cx);
                            any = true;
                        }
                        if any {
                            cx.notify();
                        }
                        true
                    })
                    .unwrap_or(false);
                if !alive {
                    break;
                }
            }
        })
        .detach();
    }

    fn handle_event(&mut self, event: Event, cx: &mut Context<Self>) {
        let body = &event.body;
        match event.event.as_str() {
            "stopped" => {
                let reason = body
                    .get("reason")
                    .and_then(Value::as_str)
                    .unwrap_or("stopped")
                    .to_string();
                let thread = body.get("threadId").and_then(Value::as_i64);
                if body.get("allThreadsStopped").and_then(Value::as_bool) == Some(true) {
                    for row in &mut self.threads {
                        if !matches!(row.state, ThreadState::Stopped(_)) {
                            row.state = ThreadState::Stopped("stopped".into());
                        }
                    }
                }
                if let Some(thread) = thread {
                    self.set_state(thread, ThreadState::Stopped(reason.clone()));
                    let keep_focus =
                        body.get("preserveFocusHint").and_then(Value::as_bool) == Some(true);
                    if !keep_focus || self.selected_thread.is_none() {
                        self.selected_thread = Some(thread);
                    }
                }
                if let Some(description) = body.get("description").and_then(Value::as_str) {
                    self.log(description.to_string());
                }
                self.read_threads(cx);
                if thread.is_some() && thread == self.selected_thread {
                    self.variables.stopped();
                    self.read_stack(cx);
                }
            }
            "continued" => {
                if body.get("allThreadsContinued").and_then(Value::as_bool) == Some(true) {
                    for row in &mut self.threads {
                        row.state = ThreadState::Running;
                    }
                } else if let Some(thread) = body.get("threadId").and_then(Value::as_i64) {
                    self.set_state(thread, ThreadState::Running);
                }
                if self
                    .selected_thread
                    .is_some_and(|id| self.state(id) == Some(&ThreadState::Running))
                {
                    self.clear_stack();
                }
            }
            "output" => {
                // Program output (probe-rs sends log point lines so) is not folded.
                let program = body.get("category").and_then(Value::as_str) == Some("stdout");
                if let Some(output) = body.get("output").and_then(Value::as_str) {
                    for line in output.lines() {
                        if program {
                            self.push_console(line.to_string().into());
                        } else {
                            self.log(line.to_string());
                        }
                    }
                }
            }
            "probe-rs-show-message" => {
                if let Some(message) = body.get("message").and_then(Value::as_str) {
                    self.log(format!("message: {message}"));
                }
            }
            RTT_CHANNEL_CONFIG_EVENT => {
                if let Ok(config) = serde_json::from_value::<RttChannelConfig>(event.body) {
                    let open = self.rtt.announced(config);
                    let key = (open.core, open.channel_number);
                    if let Some(channel) = self.rtt.get(key) {
                        self.log(format!("RTT [{}] opened", channel.label()));
                    }
                    self.request::<RttWindowOpened>(open, cx, |this, result, _| {
                        if let Err(error) = result {
                            this.log(format!("rttWindowOpened: {error}"));
                        }
                    });
                }
            }
            RTT_PROMPT_EVENT => {
                if let Ok(prompt) = serde_json::from_value::<RttPrompt>(event.body) {
                    let core = prompt
                        .core
                        .map(|core| format!("core {core} · "))
                        .unwrap_or_default();
                    self.log(format!(
                        "RTT input [{core}{}: {}] opened",
                        prompt.prompt_handle, prompt.prompt_name
                    ));
                    self.rtt.prompted(prompt);
                }
            }
            RTT_DATA_EVENT => {
                if let Ok(data) = serde_json::from_value::<RttData>(event.body) {
                    let key = (data.core, data.channel_number);
                    if self.rtt.received(data) > 0 && self.bottom == BottomTab::Rtt(key) {
                        self.scroll_rtt_to_end();
                    }
                }
            }
            "terminated" | "exited" => {
                self.log(format!("session {}", event.event));
                self.end_session(&format!("Session ended ({})", event.event), cx);
            }
            _ => {}
        }
    }

    fn state(&self, thread: i64) -> Option<&ThreadState> {
        self.threads
            .iter()
            .find(|t| t.id == thread)
            .map(|t| &t.state)
    }

    fn set_state(&mut self, thread: i64, state: ThreadState) {
        match self.threads.iter_mut().find(|t| t.id == thread) {
            Some(row) => row.state = state,
            None => self.threads.push(ThreadRow {
                id: thread,
                name: format!("thread {thread}"),
                state,
            }),
        }
    }

    fn read_threads(&mut self, cx: &mut Context<Self>) {
        self.request::<Threads>(ThreadsArgument {}, cx, |this, result, cx| {
            let threads = match result {
                Ok(response) => response.threads,
                Err(error) => {
                    this.log(format!("threads: {error}"));
                    return;
                }
            };
            let old = std::mem::take(&mut this.threads);
            this.threads = threads
                .into_iter()
                .map(|thread| ThreadRow {
                    state: old
                        .iter()
                        .find(|row| row.id == thread.id)
                        .map(|row| row.state.clone())
                        .unwrap_or(ThreadState::Unknown),
                    id: thread.id,
                    name: thread.name,
                })
                .collect();
            let cores = this
                .threads
                .iter()
                .filter_map(|row| Some((u64::try_from(row.id).ok()?, row.name.clone())))
                .collect::<Vec<_>>();
            this.live_watch.set_threads(&cores);
            if this.selected_thread.is_none()
                && let Some(first) = this.threads.first()
            {
                this.selected_thread = Some(first.id);
                this.read_stack(cx);
            }
        });
    }

    fn select_thread(&mut self, thread: i64, cx: &mut Context<Self>) {
        self.selected_thread = Some(thread);
        self.read_stack(cx);
    }

    /// Forgets the call stack, source position, disassembly and variables shown (the selected
    /// thread runs or changed).
    fn clear_stack(&mut self) {
        self.frames.clear();
        self.selected_frame = 0;
        self.disassembly = DisassemblyWindow::default();
        self.source = Err(NOT_STOPPED.into());
        self.variables.clear();
    }

    /// Reads the call stack of the selected thread. A running core has none (it would answer
    /// with an error); a thread of unknown state is asked.
    fn read_stack(&mut self, cx: &mut Context<Self>) {
        self.clear_stack();
        let Some(thread) = self.selected_thread else {
            return;
        };
        if self.state(thread) == Some(&ThreadState::Running) {
            return;
        }
        let arguments = StackTraceArguments {
            thread_id: thread,
            start_frame: None,
            levels: Some(32),
            format: None,
        };
        let asked_unknown = self.state(thread) == Some(&ThreadState::Unknown);
        self.request::<StackTrace>(arguments, cx, move |this, result, cx| {
            if this.selected_thread != Some(thread) {
                return;
            }
            match result {
                Ok(response) => {
                    if !response.stack_frames.is_empty()
                        && this.state(thread) == Some(&ThreadState::Unknown)
                    {
                        this.set_state(thread, ThreadState::Stopped("halted".into()));
                    }
                    this.frames = response.stack_frames;
                    this.select_frame(0, cx);
                }
                Err(error) => {
                    this.clear_stack();
                    match this.state(thread) {
                        // Asked to find out: a running core has no stack.
                        Some(ThreadState::Unknown) => this.set_state(thread, ThreadState::Running),
                        // It started running meanwhile, or was asked to find out.
                        Some(ThreadState::Running) => {}
                        _ if asked_unknown => {}
                        _ => this.log(format!("stackTrace: {error}")),
                    }
                }
            }
        });
    }

    fn select_frame(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.selected_frame = ix;
        self.show_source(ix);
        self.read_scopes(ix, cx);
        let Some(reference) = self
            .frames
            .get(ix)
            .and_then(|frame| frame.instruction_pointer_reference.clone())
        else {
            self.disassembly = DisassemblyWindow::default();
            return;
        };
        self.disassembly = DisassemblyWindow::new(reference.clone());
        let request = self.disassembly.first_request(BEFORE, COUNT);
        self.request::<DisassembleSigned>(request, cx, move |this, result, _| {
            if this.disassembly.reference() != reference {
                return;
            }
            match result {
                Ok(response) => {
                    this.disassembly.set_first(BEFORE, response.instructions);
                    this.scroll_to_pc = true;
                }
                Err(error) => this.log(format!("disassemble: {error}")),
            }
        });
    }

    /// Shows the source file of frame `ix` around its line.
    fn show_source(&mut self, ix: usize) {
        let path = self
            .frames
            .get(ix)
            .and_then(|frame| frame.source.as_ref())
            .and_then(|source| source.path.clone());
        self.source = match path {
            Some(path) => self.source_files.get(&path),
            None => Err("No source for this frame".into()),
        };
        self.scroll_to_line = true;
    }

    /// The line of the selected frame, if it is in the source file shown.
    fn frame_line(&self) -> Option<u64> {
        let frame = self.frames.get(self.selected_frame)?;
        let path = frame.source.as_ref()?.path.as_deref()?;
        let shown = self.source.as_ref().ok()?;
        (shown.path == path && frame.line > 0).then_some(frame.line)
    }

    /// Reads the scopes of frame `ix`, then the variables of the expanded nodes.
    fn read_scopes(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(frame) = self.frames.get(ix) else {
            self.variables.clear();
            return;
        };
        let id = frame.id;
        let function = frame.name.clone();
        let thread = self.selected_thread.unwrap_or(0).to_string();
        let request = self.variables.set_frame(id, thread, function);
        self.request::<Scopes>(request, cx, move |this, result, cx| match result {
            Ok(response) => {
                let requests = this.variables.set_scopes(id, response.scopes);
                this.read_variables(requests, cx);
            }
            Err(error) => {
                if this.variables.frame_id() == Some(id) {
                    this.log(format!("scopes: {error}"));
                }
            }
        });
    }

    fn read_variables(&mut self, requests: Vec<VariablesArguments>, cx: &mut Context<Self>) {
        for request in requests {
            let reference = request.variables_reference;
            self.request::<VariablesRequest>(request, cx, move |this, result, cx| match result {
                Ok(response) => {
                    let requests = this.variables.set_children(reference, response.variables);
                    this.read_variables(requests, cx);
                }
                Err(error) => this.variables.failed(reference, error),
            });
        }
    }

    fn toggle_variable(&mut self, row: &Row, cx: &mut Context<Self>) {
        if let Some(request) = self.variables.toggle(&row.key, row.reference) {
            self.read_variables(vec![request], cx);
        }
    }

    fn toggle_source_breakpoint(&mut self, path: String, line: u64, cx: &mut Context<Self>) {
        self.source_breakpoints.toggle(&path, line);
        self.send_source_breakpoints(path, cx);
    }

    /// Sends all the breakpoints of `path` (`setBreakpoints` replaces the file's set).
    fn send_source_breakpoints(&mut self, path: String, cx: &mut Context<Self>) {
        let request = self.source_breakpoints.request(&path);
        let asked = request.clone();
        self.request::<SetBreakpoints>(request, cx, move |this, result, _| match result {
            Ok(response) => {
                for (breakpoint, line) in response
                    .breakpoints
                    .iter()
                    .zip(asked.lines.iter().flatten())
                {
                    if !breakpoint.verified {
                        let message = breakpoint.message.as_deref().unwrap_or("not set");
                        this.log(format!("breakpoint at line {line}: {message}"));
                    }
                }
                this.source_breakpoints
                    .answered(&asked, Ok(&response.breakpoints));
            }
            Err(error) => {
                this.log(format!("setBreakpoints: {error}"));
                this.source_breakpoints.answered(&asked, Err(&error));
            }
        });
    }

    fn read_more(&mut self, side: Side, cx: &mut Context<Self>) {
        let Some(request) = self.disassembly.more_request(side, MORE) else {
            return;
        };
        let reference = self.disassembly.reference().to_string();
        self.request::<DisassembleSigned>(request, cx, move |this, result, _| {
            if this.disassembly.reference() != reference {
                return;
            }
            match result {
                Ok(response) => {
                    this.disassembly.add_more(side, response.instructions);
                }
                Err(error) => this.log(format!("disassemble: {error}")),
            }
        });
    }

    fn toggle_breakpoint(&mut self, address: String, cx: &mut Context<Self>) {
        self.breakpoints.toggle(address);
        let request = self.breakpoints.request();
        let asked = request.clone();
        self.request::<SetInstructionBreakpoints>(
            request,
            cx,
            move |this, result, _| match result {
                Ok(response) => this.breakpoints.answered(&asked, Ok(&response.breakpoints)),
                Err(error) => {
                    this.log(format!("setInstructionBreakpoints: {error}"));
                    this.breakpoints.answered(&asked, Err(&error));
                }
            },
        );
    }

    /// Execution control of the selected thread.
    fn control(&mut self, command: &'static str, extra: Value, cx: &mut Context<Self>) {
        let Some(thread) = self.selected_thread else {
            return;
        };
        let mut arguments = json!({ "threadId": thread });
        if let (Value::Object(arguments), Value::Object(extra)) = (&mut arguments, extra) {
            arguments.extend(extra);
        }
        if command != "pause" {
            self.set_state(thread, ThreadState::Running);
            self.clear_stack();
        }
        self.request_raw(command, arguments, cx);
    }
}

/// Connects to the server and starts a session: `initialize`, `attach` with the arguments in
/// the file `attach`, `configurationDone`.
fn start_session(server: &str, attach: &str) -> Result<Arc<DapClient>, String> {
    let text = std::fs::read_to_string(attach).map_err(|error| format!("{attach}: {error}"))?;
    let arguments: Value =
        serde_json::from_str(&text).map_err(|error| format!("{attach}: {error}"))?;
    let client = DapClient::connect(server).map_err(|error| format!("{server}: {error}"))?;
    client
        .request_raw(
            "initialize",
            json!({
                "clientID": "aurix-debugger", "clientName": "AURIX Debugger",
                "adapterID": "probe-rs", "linesStartAt1": true, "columnsStartAt1": true,
                "pathFormat": "path", "supportsMemoryReferences": true,
            }),
            TIMEOUT,
        )
        .map_err(|error| error.to_string())?;
    client
        .request_raw("attach", arguments, Duration::from_secs(60))
        .map_err(|error| error.to_string())?;
    client
        .request_raw("configurationDone", json!({}), TIMEOUT)
        .map_err(|error| error.to_string())?;
    Ok(Arc::new(client))
}

/// Ends a session without waiting for it on the window's thread.
fn end_in_background(client: Arc<DapClient>, cx: &mut Context<Debugger>) {
    cx.background_executor()
        .spawn(async move {
            client
                .request_raw(
                    "disconnect",
                    json!({ "terminateDebuggee": false }),
                    Duration::from_secs(2),
                )
                .ok();
        })
        .detach();
}

fn hsla(color: u32) -> Hsla {
    rgb(color).into()
}

fn panel_title(title: &'static str) -> impl IntoElement {
    div()
        .px_2()
        .py_1()
        .text_xs()
        .text_color(hsla(color::MUTED))
        .child(title)
}

fn button(
    id: &'static str,
    label: &'static str,
    enabled: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .px_2()
        .py_0p5()
        .rounded_sm()
        .text_sm()
        .bg(hsla(color::BUTTON))
        .text_color(hsla(if enabled { color::TEXT } else { color::MUTED }))
        .when(enabled, |this| {
            this.cursor_pointer()
                .hover(|style| style.bg(hsla(color::BUTTON_HOVER)))
                .on_click(on_click)
        })
        .child(label)
}

impl Debugger {
    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.selected_thread.and_then(|id| self.state(id));
        let stopped = matches!(state, Some(ThreadState::Stopped(_)));
        // A thread of unknown state may run: pause is offered.
        let running = state.is_some() && !stopped;
        div()
            .flex()
            .items_center()
            .gap_1()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(hsla(color::BORDER))
            .bg(hsla(color::PANEL))
            .child(button(
                "continue",
                "Continue",
                stopped,
                cx.listener(|this, _, _, cx| this.control("continue", json!({}), cx)),
            ))
            .child(button(
                "pause",
                "Pause",
                running,
                cx.listener(|this, _, _, cx| this.control("pause", json!({}), cx)),
            ))
            .child(button(
                "step-over",
                "Step Over",
                stopped,
                cx.listener(|this, _, _, cx| this.control("next", json!({}), cx)),
            ))
            .child(button(
                "step-instruction",
                "Step Instruction",
                stopped,
                cx.listener(|this, _, _, cx| {
                    this.control("next", json!({ "granularity": "instruction" }), cx)
                }),
            ))
            .child(button(
                "step-into-instruction",
                "Step Into Instruction",
                stopped,
                cx.listener(|this, _, _, cx| {
                    this.control("stepIn", json!({ "granularity": "instruction" }), cx)
                }),
            ))
            .child(div().flex_1())
            .child(
                div()
                    .text_sm()
                    .text_color(hsla(color::MUTED))
                    .child(self.status.clone()),
            )
            .when_some(self.attach.clone(), |this, attach| {
                this.child(
                    div()
                        .text_sm()
                        .text_color(hsla(color::MUTED))
                        .child(crate::recent::file_name(&attach).to_string()),
                )
            })
            .child(button(
                "disconnect",
                "Disconnect",
                self.client.is_some() || self.connecting,
                cx.listener(|this, _, _, cx| {
                    this.end_session("Disconnected", cx);
                    cx.notify();
                }),
            ))
    }

    fn render_threads(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .child(panel_title("THREADS"))
            .children(self.threads.iter().map(|thread| {
                let id = thread.id;
                let selected = self.selected_thread == Some(id);
                let state = match &thread.state {
                    ThreadState::Unknown => "?".to_string(),
                    ThreadState::Running => "running".to_string(),
                    ThreadState::Stopped(reason) => format!("stopped: {reason}"),
                };
                div()
                    .id(SharedString::from(format!("thread-{id}")))
                    .flex()
                    .gap_2()
                    .px_2()
                    .text_sm()
                    .cursor_pointer()
                    .when(selected, |this| this.bg(hsla(color::SELECTED)))
                    .hover(|style| style.bg(hsla(color::SELECTED)))
                    .on_click(cx.listener(move |this, _, _, cx| this.select_thread(id, cx)))
                    .child(
                        div()
                            .text_color(hsla(color::TEXT))
                            .child(thread.name.clone()),
                    )
                    .child(div().text_color(hsla(color::MUTED)).child(state))
            }))
    }

    fn render_stack(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .child(panel_title("CALL STACK"))
            .when(self.frames.is_empty(), |this| {
                this.child(
                    div()
                        .px_2()
                        .text_sm()
                        .text_color(hsla(color::MUTED))
                        .child("Not stopped"),
                )
            })
            .children(self.frames.iter().enumerate().map(|(ix, frame)| {
                let selected = ix == self.selected_frame;
                let location = match &frame.source {
                    Some(source) => {
                        format!("{}:{}", source.name.clone().unwrap_or_default(), frame.line)
                    }
                    None => frame
                        .instruction_pointer_reference
                        .clone()
                        .unwrap_or_default(),
                };
                div()
                    .id(SharedString::from(format!("frame-{ix}")))
                    .flex()
                    .flex_col()
                    .px_2()
                    .cursor_pointer()
                    .when(selected, |this| this.bg(hsla(color::SELECTED)))
                    .hover(|style| style.bg(hsla(color::SELECTED)))
                    .on_click(cx.listener(move |this, _, _, cx| this.select_frame(ix, cx)))
                    .child(
                        div()
                            .text_sm()
                            .text_color(hsla(color::TEXT))
                            .child(frame.name.clone()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(hsla(color::MUTED))
                            .child(location),
                    )
            }))
    }

    fn render_instructions(
        &mut self,
        range: Range<usize>,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let pc_index = self.disassembly.pc_index();
        range
            .filter_map(|ix| {
                let instruction = self.disassembly.instructions().get(ix)?.clone();
                let at_pc = pc_index == Some(ix);
                let filler = is_filler(&instruction);
                let breakpoint = self.breakpoints.get(&instruction.address).cloned();
                let line = self
                    .disassembly
                    .line_start(ix)
                    .map(|(file, line)| format!("{}:{line}", file.unwrap_or("line")));
                let gutter_address = instruction.address.clone();
                let source_breakpoint = parse_address(&instruction.address)
                    .and_then(|address| self.source_breakpoints.at_address(address))
                    .map(|state| state.settings.logs());
                let mark = match breakpoint {
                    Some(state) if state.verified == Some(true) => ("●", color::BREAKPOINT),
                    Some(_) => ("○", color::MUTED),
                    // A source line's breakpoint, set at this instruction (blue: a log point).
                    None => match source_breakpoint {
                        Some(true) => ("◆", color::ACCENT),
                        Some(false) => ("◆", color::BREAKPOINT),
                        None => ("", color::MUTED),
                    },
                };
                Some(
                    div()
                        .id(SharedString::from(format!("instruction-{ix}")))
                        .h(px(20.))
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_1()
                        .font_family("Consolas")
                        .text_sm()
                        .when(at_pc, |this| this.bg(hsla(color::PC_LINE)))
                        .child(
                            div()
                                .id(SharedString::from(format!("gutter-{ix}")))
                                .w(px(16.))
                                .text_color(hsla(mark.1))
                                .when(!filler, |this| {
                                    this.cursor_pointer().on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            this.toggle_breakpoint(gutter_address.clone(), cx)
                                        },
                                    ))
                                })
                                .child(mark.0),
                        )
                        .child(
                            div()
                                .w(px(12.))
                                .text_color(hsla(color::PC_MARK))
                                .child(if at_pc { "▶" } else { "" }),
                        )
                        .child(
                            div()
                                .w(px(96.))
                                .text_color(hsla(color::MUTED))
                                .child(instruction.address.clone()),
                        )
                        .child(
                            div()
                                .w(px(110.))
                                .text_color(hsla(color::MUTED))
                                .child(instruction.instruction_bytes.clone().unwrap_or_default()),
                        )
                        .child(
                            div()
                                .w(px(260.))
                                .text_color(hsla(if filler { color::MUTED } else { color::TEXT }))
                                .child(instruction.instruction.clone()),
                        )
                        .when_some(line, |this, line| {
                            this.child(div().text_color(hsla(color::ACCENT)).child(line))
                        })
                        .into_any_element(),
                )
            })
            .collect()
    }

    fn render_disassembly(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        if self.scroll_to_pc
            && let Some(ix) = self.disassembly.pc_index()
        {
            self.scroll_to_pc = false;
            self.disassembly_scroll
                .scroll_to_item(ix, ScrollStrategy::Center);
        }
        let count = self.disassembly.instructions().len();
        let shown = count > 0;
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(panel_title("DISASSEMBLY"))
                    .child(button(
                        "earlier",
                        "▲ Earlier",
                        shown,
                        cx.listener(|this, _, _, cx| this.read_more(Side::Before, cx)),
                    ))
                    .child(button(
                        "later",
                        "▼ Later",
                        shown,
                        cx.listener(|this, _, _, cx| this.read_more(Side::After, cx)),
                    ))
                    .child(button(
                        "go-to-pc",
                        "PC",
                        shown,
                        cx.listener(|this, _, _, cx| {
                            this.scroll_to_pc = true;
                            cx.notify();
                        }),
                    ))
                    .child(
                        div()
                            .px_2()
                            .text_xs()
                            .text_color(hsla(color::MUTED))
                            .child(self.disassembly.reference().to_string()),
                    ),
            )
            .child(
                uniform_list(
                    "disassembly",
                    count,
                    cx.processor(|this, range, _window, cx| this.render_instructions(range, cx)),
                )
                .track_scroll(&self.disassembly_scroll)
                .flex_1(),
            )
    }

    fn render_source_lines(
        &mut self,
        range: Range<usize>,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let Ok(file) = self.source.clone() else {
            return Vec::new();
        };
        let frame_line = self.frame_line();
        range
            .filter_map(|ix| {
                let text = file.lines.get(ix)?.clone();
                let line = ix as u64 + 1;
                let at_frame = frame_line == Some(line);
                // ● a breakpoint, ◆ one with a condition or hit count, blue ◆ a log point;
                // hollow while the adapter has not set it.
                let mark = match self.source_breakpoints.get(&file.path, line) {
                    Some(state) => {
                        let set = state.verified == Some(true);
                        let settings = &state.settings;
                        let glyph = match (settings.is_plain(), set) {
                            (true, true) => "●",
                            (true, false) => "○",
                            (false, true) => "◆",
                            (false, false) => "◇",
                        };
                        let tint = if !set {
                            color::MUTED
                        } else if settings.logs() {
                            color::ACCENT
                        } else {
                            color::BREAKPOINT
                        };
                        (glyph, tint)
                    }
                    None => ("", color::MUTED),
                };
                let path = file.path.clone();
                let edit_path = file.path.clone();
                Some(
                    div()
                        .id(SharedString::from(format!("source-{ix}")))
                        .h(px(20.))
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_1()
                        .font_family("Consolas")
                        .text_sm()
                        .when(at_frame, |this| this.bg(hsla(color::PC_LINE)))
                        .child(
                            div()
                                .id(SharedString::from(format!("source-gutter-{ix}")))
                                .w(px(16.))
                                .cursor_pointer()
                                .text_color(hsla(mark.1))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.toggle_source_breakpoint(path.clone(), line, cx)
                                }))
                                .on_mouse_down(
                                    gpui::MouseButton::Right,
                                    cx.listener(move |this, _, window, cx| {
                                        this.open_breakpoint_edit(
                                            edit_path.clone(),
                                            line,
                                            window,
                                            cx,
                                        )
                                    }),
                                )
                                .child(mark.0),
                        )
                        .child(
                            div()
                                .w(px(40.))
                                .flex()
                                .justify_end()
                                .text_color(hsla(color::MUTED))
                                .child(line.to_string()),
                        )
                        .child(
                            div()
                                .w(px(12.))
                                .text_color(hsla(color::PC_MARK))
                                .child(if at_frame { "▶" } else { "" }),
                        )
                        .child(div().whitespace_nowrap().child(text))
                        .into_any_element(),
                )
            })
            .collect()
    }

    fn render_source(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        if self.scroll_to_line
            && let Some(line) = self.frame_line()
        {
            self.scroll_to_line = false;
            self.source_scroll
                .scroll_to_item(line as usize - 1, ScrollStrategy::Center);
        }
        let (title, count) = match &self.source {
            Ok(file) => (SharedString::from(file.path.clone()), file.lines.len()),
            Err(why) => (SharedString::from(why.clone()), 0),
        };
        let shown = self.frame_line().is_some();
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .border_b_1()
            .border_color(hsla(color::BORDER))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(panel_title("SOURCE"))
                    .child(button(
                        "go-to-line",
                        "PC",
                        shown,
                        cx.listener(|this, _, _, cx| {
                            this.scroll_to_line = true;
                            cx.notify();
                        }),
                    ))
                    .child(button(
                        "reload-source",
                        "Reload",
                        true,
                        cx.listener(|this, _, _, cx| {
                            this.source_files.reload();
                            this.show_source(this.selected_frame);
                            cx.notify();
                        }),
                    ))
                    .child(
                        div()
                            .px_2()
                            .text_xs()
                            .text_color(hsla(color::MUTED))
                            .child(title),
                    ),
            )
            .children(self.render_breakpoint_edit(cx))
            .child(
                uniform_list(
                    "source",
                    count,
                    cx.processor(|this, range, _window, cx| this.render_source_lines(range, cx)),
                )
                .track_scroll(&self.source_scroll)
                .flex_1(),
            )
    }

    fn render_variables(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = self.variables.rows();
        div()
            .flex()
            .flex_col()
            .child(panel_title("VARIABLES"))
            .when(rows.is_empty(), |this| {
                this.child(
                    div()
                        .px_2()
                        .text_sm()
                        .text_color(hsla(color::MUTED))
                        .child(NOT_STOPPED),
                )
            })
            .children(rows.into_iter().enumerate().map(|(ix, row)| {
                let chevron = match (row.reference != 0, row.expanded) {
                    (false, _) => "",
                    (true, false) => "▸",
                    (true, true) => "▾",
                };
                let note = if row.loading {
                    Some("…".to_string())
                } else if row.expanded {
                    self.variables.error(row.reference).map(str::to_string)
                } else {
                    None
                };
                let scope = row.depth == 0;
                let value_color = if row.changed {
                    color::CHANGED
                } else {
                    color::TEXT
                };
                let name = row.name.clone();
                let value = row.value.clone();
                let type_ = row.type_.clone();
                // A global can be read while the core runs: it can go to the Live Watch.
                let watchable = row.depth == 1 && row.key.first().is_some_and(|s| s == "Static");
                let watch_name = row.name.clone();
                let memory_reference = row.memory_reference.clone().filter(|_| !scope);
                div()
                    .id(SharedString::from(format!("variable-{ix}")))
                    .flex()
                    .gap_1()
                    .pl(px(8. + 12. * row.depth as f32))
                    .pr_2()
                    .text_sm()
                    .when(row.reference != 0, |this| {
                        this.cursor_pointer()
                            .hover(|style| style.bg(hsla(color::SELECTED)))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.toggle_variable(&row, cx);
                                cx.notify();
                            }))
                    })
                    .child(
                        div()
                            .w(px(10.))
                            .flex_none()
                            .text_color(hsla(color::MUTED))
                            .child(chevron),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_color(hsla(if scope { color::ACCENT } else { color::TEXT }))
                            .child(name),
                    )
                    .when(!scope, |this| {
                        this.child(div().text_color(hsla(color::MUTED)).child("="))
                            .child(
                                div()
                                    .font_family("Consolas")
                                    .text_color(hsla(value_color))
                                    .child(value),
                            )
                    })
                    .when_some(type_, |this, type_| {
                        this.child(div().text_xs().text_color(hsla(color::MUTED)).child(type_))
                    })
                    .when_some(note, |this, note| {
                        this.child(div().text_xs().text_color(hsla(color::MUTED)).child(note))
                    })
                    .child(div().flex_1())
                    .when_some(memory_reference, |this, reference| {
                        this.child(
                            div()
                                .id(SharedString::from(format!("memory-{ix}")))
                                .px_1()
                                .rounded_sm()
                                .text_xs()
                                .text_color(hsla(color::MUTED))
                                .cursor_pointer()
                                .hover(|style| {
                                    style
                                        .bg(hsla(color::BUTTON_HOVER))
                                        .text_color(hsla(color::TEXT))
                                })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.show_memory_at(&reference);
                                    cx.notify();
                                }))
                                .child("mem"),
                        )
                    })
                    .when(watchable, |this| {
                        let expression = watch_name.clone();
                        this.child(
                            div()
                                .id(SharedString::from(format!("watch-{ix}")))
                                .px_1()
                                .rounded_sm()
                                .text_xs()
                                .text_color(hsla(color::MUTED))
                                .cursor_pointer()
                                .hover(|style| {
                                    style
                                        .bg(hsla(color::BUTTON_HOVER))
                                        .text_color(hsla(color::TEXT))
                                })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.add_watch(&expression);
                                    cx.notify();
                                }))
                                .child("watch"),
                        )
                    })
            }))
    }
}

impl Render for Debugger {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.drag.is_none() {
            self.fit_layout(window);
        }
        let layout = self.layout;
        let source_height = layout.source_height(window.viewport_size().height.as_f32());
        let dragging = self.drag.map(|drag| drag.splitter.across());
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(hsla(color::BACKGROUND))
            .text_color(hsla(color::TEXT))
            .font_family("Segoe UI")
            .when(dragging == Some(true), |this| this.cursor_ew_resize())
            .when(dragging == Some(false), |this| this.cursor_ns_resize())
            .on_mouse_move(
                cx.listener(|this, event, window, cx| this.mouse_moved(event, window, cx)),
            )
            .on_mouse_up(
                gpui::MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.end_drag();
                    cx.notify();
                }),
            )
            .child(self.render_toolbar(cx))
            .when(self.client.is_none(), |this| {
                this.child(self.render_connect(cx))
            })
            .when(self.client.is_some(), |this| {
                this.child(
                    div()
                        .flex()
                        .flex_1()
                        .min_h_0()
                        .child(
                            div()
                                .id("side")
                                .w(px(layout.left))
                                .flex_none()
                                .flex()
                                .flex_col()
                                .gap_2()
                                .bg(hsla(color::PANEL))
                                .overflow_y_scroll()
                                .child(self.render_threads(cx))
                                .child(self.render_stack(cx)),
                        )
                        .child(self.splitter(Splitter::Left, cx))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .flex_1()
                                .min_w_0()
                                .child(
                                    div()
                                        .h(px(source_height))
                                        .flex_none()
                                        .flex()
                                        .flex_col()
                                        .child(self.render_source(cx)),
                                )
                                .child(self.splitter(Splitter::Source, cx))
                                .child(self.render_disassembly(cx)),
                        )
                        .child(self.splitter(Splitter::Right, cx))
                        .child(
                            div()
                                .id("variables")
                                .w(px(layout.right))
                                .flex_none()
                                .bg(hsla(color::PANEL))
                                .overflow_y_scroll()
                                .child(self.render_variables(cx)),
                        ),
                )
            })
            .child(self.splitter(Splitter::Bottom, cx))
            .child(self.render_bottom(cx))
    }
}
