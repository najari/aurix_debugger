//! A small DAP client over TCP, for a debug adapter that listens on a port (probe-rs
//! `dap-server --port`).
//!
//! A reader thread takes the adapter's messages: a response goes to the request waiting for it,
//! an event to [`DapClient::next_event`]. Requests block the calling thread until their response or
//! a timeout; no async runtime is involved, so any UI (a GPUI background task, a CLI) can use
//! the client. [`DapClient::send`] writes a request at once and leaves the wait for later, so
//! that a UI sends its requests in the order it makes them and waits elsewhere. Requests from
//! the adapter to the client (`runInTerminal`, `startDebugging`) are answered as not supported.
//!
//! A message that cannot be read (a broken frame) closes the connection: what follows it cannot
//! be found.

use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use dap_types::requests::Request;
use serde_json::{Value, json};

/// An event of the adapter.
#[derive(Clone, Debug)]
pub struct Event {
    pub event: String,
    pub body: Value,
}

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    /// The adapter did not answer in time.
    Timeout(String),
    /// The adapter answered that the request failed.
    Failed {
        command: String,
        message: String,
    },
    /// The answer did not have the expected shape.
    Decode {
        command: String,
        error: serde_json::Error,
    },
    /// The connection is closed.
    Closed,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io(error) => write!(f, "{error}"),
            Error::Timeout(command) => write!(f, "no answer to {command}"),
            Error::Failed { command, message } => write!(f, "{command} failed: {message}"),
            Error::Decode { command, error } => {
                write!(f, "unexpected answer to {command}: {error}")
            }
            Error::Closed => write!(f, "the connection to the adapter is closed"),
        }
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Error::Io(error)
    }
}

type Pending = Arc<Mutex<HashMap<i64, Sender<Value>>>>;

pub struct DapClient {
    writer: Arc<Mutex<TcpStream>>,
    seq: Arc<AtomicI64>,
    pending: Pending,
    /// Set by the reader thread when the connection closes, under the lock of `pending`: a
    /// request sent after that fails at once instead of waiting for an answer that cannot come.
    closed: Arc<AtomicBool>,
    /// Behind a lock so that the client can be shared between threads (a UI's tasks).
    events: Mutex<Receiver<Event>>,
    _reader: JoinHandle<()>,
}

/// Writes one message with its `Content-Length` header.
fn write_message(writer: &Mutex<TcpStream>, message: &Value) -> io::Result<()> {
    let body = serde_json::to_vec(message)?;
    let mut stream = writer.lock().unwrap_or_else(|e| e.into_inner());
    stream.write_all(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes())?;
    stream.write_all(&body)?;
    stream.flush()
}

/// Reads one message, `None` at the end of the stream.
fn read_message(reader: &mut impl BufRead) -> io::Result<Option<Value>> {
    let mut length = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some(value) = line.strip_prefix("Content-Length:") {
            length = value.trim().parse::<usize>().ok();
        }
    }
    let Some(length) = length else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "a message without Content-Length",
        ));
    };
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    Ok(Some(serde_json::from_slice(&body)?))
}

/// Why a request failed: the error of the response's body (its format with the variables put
/// in), else its short `message`. probe-rs answers every failure with the message `cancelled`
/// and the reason in the error.
fn failure_message(response: &Value) -> String {
    if let Some(format) = response
        .pointer("/body/error/format")
        .and_then(Value::as_str)
    {
        let mut text = format.to_string();
        if let Some(variables) = response
            .pointer("/body/error/variables")
            .and_then(Value::as_object)
        {
            for (name, value) in variables {
                if let Some(value) = value.as_str() {
                    text = text.replace(&format!("{{{name}}}"), value);
                }
            }
        }
        if !text.is_empty() {
            return text;
        }
    }
    response
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("no message")
        .to_string()
}

impl DapClient {
    /// Connects to an adapter listening at `address`.
    pub fn connect(address: impl ToSocketAddrs) -> Result<Self, Error> {
        let stream = TcpStream::connect(address)?;
        stream.set_nodelay(true).ok();
        let writer = Arc::new(Mutex::new(stream.try_clone()?));
        let seq = Arc::new(AtomicI64::new(1));
        let pending: Pending = Arc::default();
        let closed = Arc::new(AtomicBool::new(false));
        let (event_sender, events) = mpsc::channel();

        let reader = {
            let pending = pending.clone();
            let closed = closed.clone();
            let writer = writer.clone();
            let seq = seq.clone();
            std::thread::spawn(move || {
                let mut reader = BufReader::new(stream);
                while let Ok(Some(message)) = read_message(&mut reader) {
                    match message.get("type").and_then(Value::as_str) {
                        Some("response") => {
                            let Some(request_seq) =
                                message.get("request_seq").and_then(Value::as_i64)
                            else {
                                continue;
                            };
                            let waiting = pending
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .remove(&request_seq);
                            if let Some(waiting) = waiting {
                                waiting.send(message).ok();
                            }
                        }
                        Some("event") => {
                            let event = Event {
                                event: message
                                    .get("event")
                                    .and_then(Value::as_str)
                                    .unwrap_or_default()
                                    .to_string(),
                                body: message.get("body").cloned().unwrap_or(Value::Null),
                            };
                            if event_sender.send(event).is_err() {
                                break;
                            }
                        }
                        Some("request") => {
                            let answer = json!({
                                "seq": seq.fetch_add(1, Ordering::SeqCst),
                                "type": "response",
                                "request_seq": message.get("seq").cloned().unwrap_or(Value::Null),
                                "command": message.get("command").cloned().unwrap_or(Value::Null),
                                "success": false,
                                "message": "not supported by this client",
                            });
                            write_message(&writer, &answer).ok();
                        }
                        _ => {}
                    }
                }
                // The stream ended, or a message could not be read: nothing after it can be.
                writer
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .shutdown(std::net::Shutdown::Both)
                    .ok();
                // The waiting requests see the connection close, and so do the next ones.
                let mut pending = pending.lock().unwrap_or_else(|e| e.into_inner());
                closed.store(true, Ordering::SeqCst);
                pending.clear();
            })
        };

        Ok(Self {
            writer,
            seq,
            pending,
            closed,
            events: Mutex::new(events),
            _reader: reader,
        })
    }

    /// The adapter's next event, waiting at most `timeout`. Events come in the order the adapter
    /// sent them; `None` when none came in time or the connection is closed.
    pub fn next_event(&self, timeout: Duration) -> Option<Event> {
        self.events
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .recv_timeout(timeout)
            .ok()
    }

    /// Like [`Self::next_event`], but tells a closed connection (`Err(Error::Closed)`, at once)
    /// from no event in time (`Ok(None)`).
    pub fn wait_event(&self, timeout: Duration) -> Result<Option<Event>, Error> {
        match self
            .events
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .recv_timeout(timeout)
        {
            Ok(event) => Ok(Some(event)),
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => Err(Error::Closed),
        }
    }

    /// Whether the connection to the adapter is closed (it ended, or a message could not be
    /// read). The events that came before it stay to be taken.
    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    /// The adapter's next event if one is waiting.
    pub fn try_event(&self) -> Option<Event> {
        self.events
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .try_recv()
            .ok()
    }

    /// Writes request `command` with `arguments` now; [`PendingResponse::wait`] waits for its
    /// response. Requests reach the adapter in the order they are sent.
    pub fn send_raw(&self, command: &str, arguments: Value) -> Result<PendingResponse, Error> {
        let seq = self.seq.fetch_add(1, Ordering::SeqCst);
        let (sender, receiver) = mpsc::channel();
        {
            let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
            if self.closed.load(Ordering::SeqCst) {
                return Err(Error::Closed);
            }
            pending.insert(seq, sender);
        }
        let message = json!({
            "seq": seq,
            "type": "request",
            "command": command,
            "arguments": arguments,
        });
        if let Err(error) = write_message(&self.writer, &message) {
            self.pending
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&seq);
            return Err(error.into());
        }
        Ok(PendingResponse {
            seq,
            command: command.to_string(),
            receiver,
            pending: self.pending.clone(),
        })
    }

    /// Sends `command` with `arguments` and waits for its response; returns the response body.
    pub fn request_raw(
        &self,
        command: &str,
        arguments: Value,
        timeout: Duration,
    ) -> Result<Value, Error> {
        self.send_raw(command, arguments)?.wait(timeout)
    }

    /// Writes request `R` now; [`TypedResponse::wait`] waits for its response.
    pub fn send<R: Request>(&self, arguments: R::Arguments) -> Result<TypedResponse<R>, Error> {
        let arguments = serde_json::to_value(arguments).map_err(|error| Error::Decode {
            command: R::COMMAND.to_string(),
            error,
        })?;
        Ok(TypedResponse {
            pending: self.send_raw(R::COMMAND, arguments)?,
            _request: std::marker::PhantomData,
        })
    }

    /// Sends request `R` and waits for its response.
    pub fn request<R: Request>(
        &self,
        arguments: R::Arguments,
        timeout: Duration,
    ) -> Result<R::Response, Error> {
        self.send::<R>(arguments)?.wait(timeout)
    }
}

/// A request that was sent, waiting for its response.
pub struct PendingResponse {
    seq: i64,
    command: String,
    receiver: Receiver<Value>,
    pending: Pending,
}

impl PendingResponse {
    /// Waits at most `timeout` for the response; returns its body.
    pub fn wait(self, timeout: Duration) -> Result<Value, Error> {
        let response = match self.receiver.recv_timeout(timeout) {
            Ok(response) => response,
            Err(RecvTimeoutError::Timeout) => {
                self.pending
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&self.seq);
                return Err(Error::Timeout(self.command));
            }
            Err(RecvTimeoutError::Disconnected) => return Err(Error::Closed),
        };
        if response.get("success").and_then(Value::as_bool) != Some(true) {
            let message = failure_message(&response);
            return Err(Error::Failed {
                command: self.command,
                message,
            });
        }
        Ok(response.get("body").cloned().unwrap_or(Value::Null))
    }
}

/// A request `R` that was sent, waiting for its response.
pub struct TypedResponse<R> {
    pending: PendingResponse,
    _request: std::marker::PhantomData<fn() -> R>,
}

impl<R: Request> TypedResponse<R> {
    /// Waits at most `timeout` for the response.
    pub fn wait(self, timeout: Duration) -> Result<R::Response, Error> {
        let body = self.pending.wait(timeout)?;
        serde_json::from_value(body).map_err(|error| Error::Decode {
            command: R::COMMAND.to_string(),
            error,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    /// An adapter that answers `threads` and sends an event, on a local port.
    fn adapter() -> std::net::SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let writer = Mutex::new(stream.try_clone().unwrap());
            let mut reader = BufReader::new(stream);
            while let Ok(Some(request)) = read_message(&mut reader) {
                let seq = request["seq"].clone();
                let command = request["command"].as_str().unwrap().to_string();
                if command == "threads" {
                    write_message(
                        &writer,
                        &json!({ "seq": 100, "type": "event", "event": "probe-rs-rtt-data",
                                 "body": { "channelNumber": 0, "data": "hi\n" } }),
                    )
                    .unwrap();
                    write_message(
                        &writer,
                        &json!({ "seq": 101, "type": "response", "request_seq": seq,
                                 "command": command, "success": true,
                                 "body": { "threads": [{ "id": 0, "name": "0-TC275" }] } }),
                    )
                    .unwrap();
                } else {
                    write_message(
                        &writer,
                        &json!({ "seq": 102, "type": "response", "request_seq": seq,
                                 "command": command, "success": false, "message": "nope" }),
                    )
                    .unwrap();
                }
            }
        });
        address
    }

    #[test]
    fn the_client_can_be_shared_between_threads() {
        fn shared<T: Send + Sync>() {}
        shared::<DapClient>();
    }

    #[test]
    fn requests_get_their_responses_and_events_come_on_the_channel() {
        let client = DapClient::connect(adapter()).unwrap();
        let threads = client
            .request::<dap_types::requests::Threads>(
                dap_types::ThreadsArgument {},
                Duration::from_secs(5),
            )
            .unwrap();
        assert_eq!(threads.threads[0].name, "0-TC275");
        let event = client.next_event(Duration::from_secs(5)).unwrap();
        assert_eq!(event.event, "probe-rs-rtt-data");
        assert_eq!(event.body["data"], "hi\n");

        match client.request_raw("liveWatch", json!({}), Duration::from_secs(5)) {
            Err(Error::Failed { message, .. }) => assert_eq!(message, "nope"),
            other => panic!("expected a failure, got {other:?}"),
        }
    }

    #[test]
    fn a_closed_connection_ends_the_wait_for_an_event() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            // Accept and close at once.
            let _ = listener.accept().unwrap();
        });
        let client = DapClient::connect(address).unwrap();
        let start = std::time::Instant::now();
        let mut closed = false;
        while start.elapsed() < Duration::from_secs(5) {
            match client.wait_event(Duration::from_secs(5)) {
                Err(Error::Closed) => {
                    closed = true;
                    break;
                }
                Ok(_) => {}
                Err(other) => panic!("{other}"),
            }
        }
        assert!(closed);
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn a_failure_says_why() {
        // probe-rs: the reason in the error's variables, the message always "cancelled".
        let probe_rs = json!({
            "success": false, "message": "cancelled",
            "body": { "error": { "id": 0, "format": "{response_message}",
                "variables": { "response_message": "Core must be halted: The core is asleep." } } }
        });
        assert_eq!(
            failure_message(&probe_rs),
            "Core must be halted: The core is asleep."
        );
        let plain = json!({ "success": false, "message": "nope" });
        assert_eq!(failure_message(&plain), "nope");
        let formatted = json!({
            "success": false, "message": "x",
            "body": { "error": { "id": 1, "format": "no file {path}", "variables": { "path": "a.c" } } }
        });
        assert_eq!(failure_message(&formatted), "no file a.c");
    }

    #[test]
    fn requests_reach_the_adapter_in_the_order_they_are_sent() {
        // An adapter that answers each request with the order it came in.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let writer = Mutex::new(stream.try_clone().unwrap());
            let mut reader = BufReader::new(stream);
            let mut order = 0;
            while let Ok(Some(request)) = read_message(&mut reader) {
                order += 1;
                write_message(
                    &writer,
                    &json!({ "seq": 100 + order, "type": "response",
                             "request_seq": request["seq"], "command": request["command"],
                             "success": true, "body": { "order": order,
                             "lines": request["arguments"]["lines"] } }),
                )
                .unwrap();
            }
        });
        let client = DapClient::connect(address).unwrap();
        let first = client
            .send_raw("setBreakpoints", json!({ "lines": [10] }))
            .unwrap();
        let second = client
            .send_raw("setBreakpoints", json!({ "lines": [10, 20] }))
            .unwrap();
        // Waited for in the other order, from other threads: they were sent already.
        let second = std::thread::spawn(move || second.wait(Duration::from_secs(5)).unwrap());
        let first = first.wait(Duration::from_secs(5)).unwrap();
        let second = second.join().unwrap();
        assert_eq!(first["order"], 1);
        assert_eq!(first["lines"], json!([10]));
        assert_eq!(second["order"], 2);
        assert_eq!(second["lines"], json!([10, 20]));
    }

    #[test]
    fn a_message_that_cannot_be_read_closes_the_connection() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (done, adapter_done) = mpsc::channel::<()>();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            write_message(
                &Mutex::new(stream.try_clone().unwrap()),
                &json!({ "seq": 1, "type": "event", "event": "stopped", "body": {} }),
            )
            .unwrap();
            // A frame without its Content-Length; the adapter keeps the socket open.
            stream.write_all(b"Content-Lenght: 12\r\n\r\n{}").unwrap();
            adapter_done.recv_timeout(Duration::from_secs(10)).ok();
        });
        let client = DapClient::connect(address).unwrap();
        let start = std::time::Instant::now();
        while !client.is_closed() && start.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(client.is_closed());
        // The event before it stays to be taken.
        assert_eq!(client.try_event().unwrap().event, "stopped");
        assert!(matches!(
            client.request_raw("threads", json!({}), Duration::from_secs(5)),
            Err(Error::Closed)
        ));
        done.send(()).ok();
    }

    #[test]
    fn a_request_after_the_connection_closed_fails_at_once() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let _ = listener.accept().unwrap();
        });
        let client = DapClient::connect(address).unwrap();
        while client.wait_event(Duration::from_secs(5)).is_ok() {}
        // The write may still succeed: the socket closed on the other side only.
        let start = std::time::Instant::now();
        match client.request_raw("disconnect", json!({}), Duration::from_secs(5)) {
            Err(Error::Closed | Error::Io(_)) => {}
            other => panic!("expected a closed connection, got {other:?}"),
        }
        assert!(start.elapsed() < Duration::from_secs(1));
    }
}
