//! A small DAP client over TCP, for a debug adapter that listens on a port (probe-rs
//! `dap-server --port`).
//!
//! A reader thread takes the adapter's messages: a response goes to the request waiting for it,
//! an event to [`DapClient::events`]. Requests block the calling thread until their response or
//! a timeout; no async runtime is involved, so any UI (a GPUI background task, a CLI) can use
//! the client. Requests from the adapter to the client (`runInTerminal`, `startDebugging`) are
//! answered as not supported.

use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicI64, Ordering};
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
    events: Receiver<Event>,
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

impl DapClient {
    /// Connects to an adapter listening at `address`.
    pub fn connect(address: impl ToSocketAddrs) -> Result<Self, Error> {
        let stream = TcpStream::connect(address)?;
        stream.set_nodelay(true).ok();
        let writer = Arc::new(Mutex::new(stream.try_clone()?));
        let seq = Arc::new(AtomicI64::new(1));
        let pending: Pending = Arc::default();
        let (event_sender, events) = mpsc::channel();

        let reader = {
            let pending = pending.clone();
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
                // The waiting requests see the connection close.
                pending.lock().unwrap_or_else(|e| e.into_inner()).clear();
            })
        };

        Ok(Self {
            writer,
            seq,
            pending,
            events,
            _reader: reader,
        })
    }

    /// The adapter's events, in the order they came.
    pub fn events(&self) -> &Receiver<Event> {
        &self.events
    }

    /// Sends `command` with `arguments` and waits for its response; returns the response body.
    pub fn request_raw(
        &self,
        command: &str,
        arguments: Value,
        timeout: Duration,
    ) -> Result<Value, Error> {
        let seq = self.seq.fetch_add(1, Ordering::SeqCst);
        let (sender, receiver) = mpsc::channel();
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(seq, sender);
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
        let response = match receiver.recv_timeout(timeout) {
            Ok(response) => response,
            Err(RecvTimeoutError::Timeout) => {
                self.pending
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&seq);
                return Err(Error::Timeout(command.to_string()));
            }
            Err(RecvTimeoutError::Disconnected) => return Err(Error::Closed),
        };
        if response.get("success").and_then(Value::as_bool) != Some(true) {
            let message = response
                .get("message")
                .and_then(Value::as_str)
                .or_else(|| {
                    response
                        .pointer("/body/error/format")
                        .and_then(Value::as_str)
                })
                .unwrap_or("no message")
                .to_string();
            return Err(Error::Failed {
                command: command.to_string(),
                message,
            });
        }
        Ok(response.get("body").cloned().unwrap_or(Value::Null))
    }

    /// Sends request `R` and waits for its response.
    pub fn request<R: Request>(
        &self,
        arguments: R::Arguments,
        timeout: Duration,
    ) -> Result<R::Response, Error> {
        let arguments = serde_json::to_value(arguments).map_err(|error| Error::Decode {
            command: R::COMMAND.to_string(),
            error,
        })?;
        let body = self.request_raw(R::COMMAND, arguments, timeout)?;
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
    fn requests_get_their_responses_and_events_come_on_the_channel() {
        let client = DapClient::connect(adapter()).unwrap();
        let threads = client
            .request::<dap_types::requests::Threads>(
                dap_types::ThreadsArgument {},
                Duration::from_secs(5),
            )
            .unwrap();
        assert_eq!(threads.threads[0].name, "0-TC275");
        let event = client
            .events()
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        assert_eq!(event.event, "probe-rs-rtt-data");
        assert_eq!(event.body["data"], "hi\n");

        match client.request_raw("liveWatch", json!({}), Duration::from_secs(5)) {
            Err(Error::Failed { message, .. }) => assert_eq!(message, "nope"),
            other => panic!("expected a failure, got {other:?}"),
        }
    }
}
