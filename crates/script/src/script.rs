//! The command file: one command a line, `#` starts a comment, times are `500ms` or `5s`.
//! `${NAME}` is replaced by the value given with `--set NAME=value`, else by the environment
//! variable, else by the default in `${NAME:-default}`.
//!
//! The whole file is read before anything runs: a mistake anywhere ends the run before the
//! server is asked anything.

use std::collections::HashMap;
use std::fmt;
use std::time::Duration;

use aurix_debug_model::breakpoints::BreakpointSettings;
use aurix_debug_model::memory::{parse_bytes, parse_location};

use crate::compare::{Op, split_comparison};

/// How long a `wait` or `expect … within` waits when the script does not say.
pub const DEFAULT_WAIT: Duration = Duration::from_secs(10);

/// Which cores a `continue` or `pause` is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cores {
    /// The core that stopped last; every core when none has.
    Last,
    One(u64),
    All,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Over,
    Into,
    Out,
    Instruction,
}

/// What a data breakpoint catches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Read,
    Write,
    ReadWrite,
}

impl Access {
    /// The DAP `accessType`.
    pub fn dap(self) -> &'static str {
        match self {
            Access::Read => "read",
            Access::Write => "write",
            Access::ReadWrite => "readWrite",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Server(String),
    /// The attach arguments file, as written (relative to the script's folder).
    Attach(String),
    Break {
        path: String,
        line: u64,
        settings: BreakpointSettings,
    },
    /// Removes the source breakpoint of a `break` before it.
    Unbreak {
        path: String,
        line: u64,
    },
    /// An instruction breakpoint: the address and the core suffix as written.
    BreakAt(String),
    Watch {
        name: String,
        access: Access,
        settings: BreakpointSettings,
    },
    Continue(Cores),
    Pause(Cores),
    Step(Step, Option<u64>),
    WaitStopped {
        core: Option<u64>,
        reason: Option<String>,
        within: Duration,
    },
    Expect {
        name: String,
        op: Op,
        value: String,
    },
    ExpectLive {
        name: String,
        core: Option<u64>,
        op: Op,
        value: String,
        within: Duration,
    },
    ExpectMemory {
        address: u64,
        core: Option<u64>,
        bytes: Vec<u8>,
    },
    Write {
        address: u64,
        core: Option<u64>,
        bytes: Vec<u8>,
    },
    RttSend {
        core: u64,
        channel: u32,
        text: String,
    },
    RttExpect {
        core: u64,
        channel: u32,
        pattern: String,
        within: Duration,
    },
    Sleep(Duration),
    Disconnect,
}

impl Command {
    /// Whether the command checks something (a test case of the JUnit report).
    pub fn is_expectation(&self) -> bool {
        matches!(
            self,
            Command::Expect { .. }
                | Command::ExpectLive { .. }
                | Command::ExpectMemory { .. }
                | Command::RttExpect { .. }
        )
    }
}

/// One command and where it is.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub number: usize,
    /// The line as written, without its comment, after `${…}` replacement.
    pub text: String,
    pub command: Command,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Script {
    pub lines: Vec<Line>,
}

/// A mistake in the script.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    pub line: usize,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for ParseError {}

impl Script {
    /// Reads a whole script; `vars` are the `--set` values.
    pub fn parse(text: &str, vars: &HashMap<String, String>) -> Result<Script, ParseError> {
        let mut lines = Vec::new();
        for (ix, raw) in text.lines().enumerate() {
            let number = ix + 1;
            let error = |message: String| ParseError {
                line: number,
                message,
            };
            let text = substitute(strip_comment(raw), vars).map_err(error)?;
            let text = text.trim();
            if text.is_empty() {
                continue;
            }
            let command = parse_command(text).map_err(error)?;
            lines.push(Line {
                number,
                text: text.to_string(),
                command,
            });
        }
        let attach = lines
            .iter()
            .position(|l| matches!(l.command, Command::Attach(_)));
        for line in &lines {
            let before_attach = attach.is_none_or(|at| lines[at].number > line.number);
            let needs_session = !matches!(line.command, Command::Server(_) | Command::Sleep(_));
            if needs_session && !matches!(line.command, Command::Attach(_)) && before_attach {
                return Err(ParseError {
                    line: line.number,
                    message: format!("`{}` comes before `attach`", line.text),
                });
            }
            if matches!(line.command, Command::Server(_)) && !before_attach {
                return Err(ParseError {
                    line: line.number,
                    message: "`server` comes after `attach`".into(),
                });
            }
        }
        Ok(Script { lines })
    }

    /// The expectations, in order.
    pub fn expectations(&self) -> impl Iterator<Item = &Line> {
        self.lines.iter().filter(|l| l.command.is_expectation())
    }
}

/// The line without its comment: `#` at the start or after a space, outside double quotes.
fn strip_comment(line: &str) -> &str {
    let mut quoted = false;
    let mut previous = ' ';
    for (at, c) in line.char_indices() {
        match c {
            '"' => quoted = !quoted,
            '#' if !quoted && previous.is_whitespace() => return &line[..at],
            _ => {}
        }
        previous = c;
    }
    line
}

/// Replaces each `${NAME}` and `${NAME:-default}`.
fn substitute(line: &str, vars: &HashMap<String, String>) -> Result<String, String> {
    let mut out = String::new();
    let mut rest = line;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let end = after
            .find('}')
            .ok_or_else(|| format!("`${{` without its `}}` in `{line}`"))?;
        let inner = &after[..end];
        let (name, default) = match inner.split_once(":-") {
            Some((name, default)) => (name, Some(default)),
            None => (inner, None),
        };
        let value = vars
            .get(name)
            .cloned()
            .or_else(|| std::env::var(name).ok())
            .or_else(|| default.map(str::to_string))
            .ok_or_else(|| {
                format!(
                    "`${{{name}}}` has no value: give it with --set {name}=… or the environment"
                )
            })?;
        out.push_str(&value);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

/// `500ms`, `5s`, `1.5s`.
pub fn parse_duration(text: &str) -> Result<Duration, String> {
    let bad = || format!("`{text}` is not a time: write 500ms or 5s");
    let (number, unit) = if let Some(number) = text.strip_suffix("ms") {
        (number, 0.001)
    } else if let Some(number) = text.strip_suffix('s') {
        (number, 1.0)
    } else {
        return Err(bad());
    };
    let value: f64 = number.parse().map_err(|_| bad())?;
    if !(value.is_finite() && value >= 0.0) {
        return Err(bad());
    }
    Ok(Duration::from_secs_f64(value * unit))
}

fn parse_core(text: &str) -> Result<u64, String> {
    text.parse()
        .map_err(|_| format!("`{text}` is not a core number"))
}

/// A trailing `within <time>`: the text before it and the time.
fn take_within(text: &str) -> Result<(&str, Duration), String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.len() >= 2 && words[words.len() - 2] == "within" {
        let at = text.rfind("within").unwrap_or(text.len());
        return Ok((
            text[..at].trim_end(),
            parse_duration(words[words.len() - 1])?,
        ));
    }
    Ok((text, DEFAULT_WAIT))
}

/// The `if …`, `hit …` and `log …` parts after a breakpoint's place.
fn breakpoint_settings(words: &[&str]) -> Result<BreakpointSettings, String> {
    let mut fields: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut current: Option<&str> = None;
    for word in words {
        match *word {
            "if" | "hit" | "log" => {
                if fields.contains_key(word) {
                    return Err(format!("`{word}` twice"));
                }
                fields.insert(word, Vec::new());
                current = Some(word);
            }
            word => match current {
                Some(field) => fields.entry(field).or_default().push(word),
                None => {
                    return Err(format!(
                        "`{word}`: after the place come `if <condition>`, `hit <count>`, `log <message>`"
                    ));
                }
            },
        }
    }
    let field = |name: &str| -> Result<String, String> {
        match fields.get(name) {
            Some(words) if words.is_empty() => Err(format!("`{name}` needs something after it")),
            Some(words) => Ok(words.join(" ")),
            None => Ok(String::new()),
        }
    };
    Ok(BreakpointSettings::new(
        &field("if")?,
        &field("hit")?,
        &field("log")?,
    ))
}

/// A double-quoted string at the start of `text`: its content and what follows.
fn quoted(text: &str) -> Result<(String, &str), String> {
    let rest = text
        .strip_prefix('"')
        .ok_or_else(|| format!("`{text}` should start with a double quote"))?;
    let mut out = String::new();
    let mut chars = rest.char_indices();
    while let Some((at, c)) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some((_, c)) => {
                    if c != '"' && c != '\\' {
                        out.push('\\');
                    }
                    out.push(c);
                }
                None => break,
            },
            '"' => return Ok((out, &rest[at + 1..])),
            c => out.push(c),
        }
    }
    Err(format!("`{text}` has no closing double quote"))
}

fn parse_command(text: &str) -> Result<Command, String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let (&keyword, args) = words.split_first().ok_or("empty line")?;
    // The text after the first `n` words.
    let after = |n: usize| -> &str {
        let mut rest = text;
        for _ in 0..n {
            rest = rest.trim_start();
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            rest = &rest[end..];
        }
        rest.trim()
    };
    let no_more = |n: usize| -> Result<(), String> {
        if args.len() > n {
            Err(format!(
                "`{keyword}` takes {n} argument(s), not `{}`",
                after(1)
            ))
        } else {
            Ok(())
        }
    };
    let cores = |args: &[&str]| -> Result<Cores, String> {
        match args {
            [] => Ok(Cores::Last),
            ["all"] => Ok(Cores::All),
            [core] => Ok(Cores::One(parse_core(core)?)),
            _ => Err(format!("`{keyword}` takes a core number or `all`")),
        }
    };
    match keyword {
        "server" => {
            no_more(1)?;
            let address = args.first().ok_or("`server` needs <host>:<port>")?;
            if !address.contains(':') {
                return Err(format!("`{address}` is not <host>:<port>"));
            }
            Ok(Command::Server(address.to_string()))
        }
        "attach" => {
            let path = after(1);
            if path.is_empty() {
                return Err("`attach` needs the attach arguments file".into());
            }
            Ok(Command::Attach(path.trim_matches('"').to_string()))
        }
        "break" | "unbreak" => {
            let place = args
                .first()
                .ok_or_else(|| format!("`{keyword}` needs <file>:<line>"))?;
            let (path, line) = place
                .rsplit_once(':')
                .ok_or_else(|| format!("`{place}` is not <file>:<line>"))?;
            let line: u64 = line
                .parse()
                .map_err(|_| format!("`{line}` is not a line number"))?;
            if path.is_empty() || line == 0 {
                return Err(format!("`{place}` is not <file>:<line>"));
            }
            if keyword == "unbreak" {
                no_more(1)?;
                return Ok(Command::Unbreak {
                    path: path.to_string(),
                    line,
                });
            }
            Ok(Command::Break {
                path: path.to_string(),
                line,
                settings: breakpoint_settings(&args[1..])?,
            })
        }
        "break-at" => {
            no_more(1)?;
            let reference = args.first().ok_or("`break-at` needs <address>[@core]")?;
            parse_location(reference)?;
            Ok(Command::BreakAt(reference.to_string()))
        }
        "watch" => {
            let name = args
                .first()
                .ok_or("`watch` needs <variable> read|write|access")?;
            let access = match args.get(1) {
                Some(&"read") => Access::Read,
                Some(&"write") => Access::Write,
                Some(&"access") => Access::ReadWrite,
                _ => return Err("`watch <variable>` needs read, write or access".into()),
            };
            Ok(Command::Watch {
                name: name.to_string(),
                access,
                settings: breakpoint_settings(&args[2..])?,
            })
        }
        "continue" => Ok(Command::Continue(cores(args)?)),
        "pause" => Ok(Command::Pause(cores(args)?)),
        "step" => {
            let kind = match args.first() {
                Some(&"over") => Step::Over,
                Some(&"into") => Step::Into,
                Some(&"out") => Step::Out,
                Some(&"instruction") => Step::Instruction,
                _ => return Err("`step` needs over, into, out or instruction".into()),
            };
            no_more(2)?;
            let core = args.get(1).map(|c| parse_core(c)).transpose()?;
            Ok(Command::Step(kind, core))
        }
        "wait" => {
            if args.first() != Some(&"stopped") {
                return Err(
                    "`wait` is `wait stopped [core] [reason <reason>] [within <time>]`".into(),
                );
            }
            let (rest, within) = take_within(after(2))?;
            let words: Vec<&str> = rest.split_whitespace().collect();
            let (core, words) = match words.first() {
                Some(word) if *word != "reason" => (Some(parse_core(word)?), &words[1..]),
                _ => (None, &words[..]),
            };
            // A reason may be more than a word (`data breakpoint`).
            let reason = match words {
                [] => None,
                ["reason", reason @ ..] if !reason.is_empty() => Some(reason.join(" ")),
                _ => {
                    return Err(
                        "`wait stopped` takes [core] [reason <reason>] [within <time>]".into(),
                    );
                }
            };
            Ok(Command::WaitStopped {
                core,
                reason,
                within,
            })
        }
        "expect" => match args.first() {
            Some(&"live") => {
                let (rest, within) = take_within(after(2))?;
                let (name, op, value) = split_comparison(rest)?;
                let (name, core) = match name.split_once('@') {
                    Some((name, core)) => (name.to_string(), Some(parse_core(core)?)),
                    None => (name, None),
                };
                Ok(Command::ExpectLive {
                    name,
                    core,
                    op,
                    value,
                    within,
                })
            }
            Some(&"memory") => {
                let location = args
                    .get(1)
                    .ok_or("`expect memory` needs <address>[@core]")?;
                let (address, core) = parse_location(location)?;
                let bytes = parse_bytes(after(3))?;
                if bytes.is_empty() {
                    return Err("`expect memory` needs the bytes, e.g. 2A 00".into());
                }
                Ok(Command::ExpectMemory {
                    address,
                    core,
                    bytes,
                })
            }
            _ => {
                let (name, op, value) = split_comparison(after(1))?;
                Ok(Command::Expect { name, op, value })
            }
        },
        "write" => {
            let location = args
                .first()
                .ok_or("`write` needs <address>[@core] <bytes>")?;
            let (address, core) = parse_location(location)?;
            let bytes = parse_bytes(after(2))?;
            if bytes.is_empty() {
                return Err("`write` needs the bytes, e.g. 2A 00".into());
            }
            Ok(Command::Write {
                address,
                core,
                bytes,
            })
        }
        "rtt" => {
            let kind = args.first().copied().unwrap_or_default();
            let core = parse_core(args.get(1).ok_or("`rtt` needs <core> <channel>")?)?;
            let channel: u32 = args
                .get(2)
                .ok_or("`rtt` needs <core> <channel>")?
                .parse()
                .map_err(|_| "the RTT channel is a number".to_string())?;
            match kind {
                "send" => {
                    let text = after(4);
                    let text = if text.starts_with('"') {
                        let (text, rest) = quoted(text)?;
                        if !rest.trim().is_empty() {
                            return Err(format!("`{}` after the text", rest.trim()));
                        }
                        text
                    } else {
                        text.to_string()
                    };
                    if text.is_empty() {
                        return Err("`rtt send` needs the text".into());
                    }
                    Ok(Command::RttSend {
                        core,
                        channel,
                        text,
                    })
                }
                "expect" => {
                    let (pattern, rest) = quoted(after(4))?;
                    regex::Regex::new(&pattern)
                        .map_err(|error| format!("`{pattern}` is not a pattern: {error}"))?;
                    let rest = rest.trim();
                    let within = match rest.split_whitespace().collect::<Vec<_>>()[..] {
                        [] => DEFAULT_WAIT,
                        ["within", time] => parse_duration(time)?,
                        _ => return Err(format!("`{rest}` after the pattern")),
                    };
                    Ok(Command::RttExpect {
                        core,
                        channel,
                        pattern,
                        within,
                    })
                }
                _ => Err("`rtt` is `rtt send …` or `rtt expect …`".into()),
            }
        }
        "sleep" => {
            no_more(1)?;
            Ok(Command::Sleep(parse_duration(
                args.first().ok_or("`sleep` needs a time")?,
            )?))
        }
        "disconnect" => {
            no_more(0)?;
            Ok(Command::Disconnect)
        }
        _ => Err(format!("`{keyword}` is not a command")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<Script, ParseError> {
        Script::parse(text, &HashMap::new())
    }

    fn one(line: &str) -> Command {
        let script = parse(&format!("attach a.json\n{line}")).unwrap();
        script.lines[1].command.clone()
    }

    #[test]
    fn the_demo_script() {
        let script = parse(
            "# T-S1\nserver 127.0.0.1:50000\nattach ../a.json   # the 3 cores\n\
             break C:\\work\\demo.c:23\ncontinue\nwait stopped 0 reason breakpoint\n\
             expect k == 3\nexpect v < 4\n",
        )
        .unwrap();
        assert_eq!(script.lines.len(), 7);
        assert_eq!(script.lines[1].number, 3);
        assert_eq!(script.lines[1].command, Command::Attach("../a.json".into()));
        assert_eq!(
            script.lines[2].command,
            Command::Break {
                path: "C:\\work\\demo.c".into(),
                line: 23,
                settings: BreakpointSettings::default()
            }
        );
        assert_eq!(
            script.lines[4].command,
            Command::WaitStopped {
                core: Some(0),
                reason: Some("breakpoint".into()),
                within: DEFAULT_WAIT
            }
        );
        assert_eq!(script.expectations().count(), 2);
    }

    #[test]
    fn breakpoints_with_conditions() {
        match one("break demo.c:23 if v > 1 && k == 3 hit % 4") {
            Command::Break { settings, .. } => {
                assert_eq!(settings.condition.as_deref(), Some("v > 1 && k == 3"));
                assert_eq!(settings.hit_condition.as_deref(), Some("% 4"));
            }
            other => panic!("{other:?}"),
        }
        match one("watch g_result write if g_result == 46") {
            Command::Watch {
                access, settings, ..
            } => {
                assert_eq!(access, Access::Write);
                assert_eq!(settings.condition.as_deref(), Some("g_result == 46"));
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(
            one("break-at 0x8021C028@1"),
            Command::BreakAt("0x8021C028@1".into())
        );
    }

    #[test]
    fn run_control_and_waits() {
        assert_eq!(one("continue"), Command::Continue(Cores::Last));
        assert_eq!(one("continue all"), Command::Continue(Cores::All));
        assert_eq!(one("pause 2"), Command::Pause(Cores::One(2)));
        assert_eq!(
            one("step instruction 1"),
            Command::Step(Step::Instruction, Some(1))
        );
        assert_eq!(
            one("wait stopped reason step within 500ms"),
            Command::WaitStopped {
                core: None,
                reason: Some("step".into()),
                within: Duration::from_millis(500)
            }
        );
        assert_eq!(
            one("sleep 1.5s"),
            Command::Sleep(Duration::from_millis(1500))
        );
        assert_eq!(
            one("wait stopped 0 reason data breakpoint within 2s"),
            Command::WaitStopped {
                core: Some(0),
                reason: Some("data breakpoint".into()),
                within: Duration::from_secs(2)
            }
        );
    }

    #[test]
    fn expectations() {
        assert_eq!(
            one("expect live g_ticks@1 > 100 within 2s"),
            Command::ExpectLive {
                name: "g_ticks".into(),
                core: Some(1),
                op: Op::Gt,
                value: "100".into(),
                within: Duration::from_secs(2)
            }
        );
        assert_eq!(
            one("expect memory 0x7000A004@0 2E 00 00 00"),
            Command::ExpectMemory {
                address: 0x7000_a004,
                core: Some(0),
                bytes: vec![0x2e, 0, 0, 0]
            }
        );
        assert_eq!(
            one("rtt expect 1 0 \"got: \\\"ping\\\"\" within 2s"),
            Command::RttExpect {
                core: 1,
                channel: 0,
                pattern: "got: \"ping\"".into(),
                within: Duration::from_secs(2)
            }
        );
        assert_eq!(
            one("rtt send 1 0 ping # a comment"),
            Command::RttSend {
                core: 1,
                channel: 0,
                text: "ping".into()
            }
        );
    }

    #[test]
    fn mistakes_name_their_line() {
        let error = parse("attach a.json\ncontinue\nexpect k ==\n").unwrap_err();
        assert_eq!(error.line, 3);
        assert_eq!(parse("attach a.json\nfly away").unwrap_err().line, 2);
        assert_eq!(parse("continue\nattach a.json").unwrap_err().line, 1);
        assert_eq!(parse("attach a.json\nserver x:1").unwrap_err().line, 2);
        assert_eq!(parse("attach a.json\nsleep 5").unwrap_err().line, 2);
        assert_eq!(
            parse("attach a.json\nbreak demo.c:23 hit")
                .unwrap_err()
                .line,
            2
        );
        assert_eq!(
            parse("attach a.json\nrtt expect 0 0 \"(\"")
                .unwrap_err()
                .line,
            2
        );
        assert_eq!(
            parse("attach a.json\nwait stopped 0 why").unwrap_err().line,
            2
        );
    }

    #[test]
    fn substitution() {
        let vars = HashMap::from([("DEMO".to_string(), "C:/demo".to_string())]);
        let script = Script::parse(
            "attach ${DEMO}/a.json\nbreak ${SRC:-C:/src}/demo.c:23\n",
            &vars,
        )
        .unwrap();
        assert_eq!(
            script.lines[0].command,
            Command::Attach("C:/demo/a.json".into())
        );
        assert!(
            matches!(&script.lines[1].command, Command::Break { path, .. } if path == "C:/src/demo.c")
        );
        let error = Script::parse("attach ${AURIX_NO_SUCH_VARIABLE_42}", &vars).unwrap_err();
        assert!(error.message.contains("--set"), "{error}");
    }

    #[test]
    fn unbreak_names_one_line() {
        let vars = HashMap::new();
        let script = Script::parse("attach a.json\nunbreak C:/src/demo.c:32\n", &vars).unwrap();
        assert_eq!(
            script.lines[1].command,
            Command::Unbreak {
                path: "C:/src/demo.c".into(),
                line: 32
            }
        );
        assert!(Script::parse("attach a.json\nunbreak C:/src/demo.c:32 if i == 0", &vars).is_err());
        assert!(Script::parse("attach a.json\nunbreak demo.c", &vars).is_err());
    }
}
