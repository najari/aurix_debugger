//! The command file: one command a line, `#` starts a comment, times are `500ms` or `5s`.
//! `${NAME}` is replaced by the value given with `--set NAME=value`, else by the environment
//! variable, else by the default in `${NAME:-default}`.
//!
//! Script variables: `let NAME = <expression>` or `let NAME = read …` (a value of the target)
//! gives `NAME` a value while the script runs. Expressions name them as they are (`n + 1`);
//! other commands take their value with `${NAME}`, put in when the line runs. `if`/`else`/`end`,
//! `while <condition>`/`end` and `repeat <count>`/`end` are the branches and loops; `check
//! <condition>` is an expectation on script values, `fail <message>` a failed one, and `print
//! <text>` writes a line.
//!
//! The whole file is read before anything runs: a mistake anywhere ends the run before the
//! server is asked anything. A line with a script variable is read again when it runs.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::time::Duration;

use aurix_debug_model::breakpoints::BreakpointSettings;
use aurix_debug_model::memory::{parse_bytes, parse_location};

use crate::compare::{Op, split_comparison};
use crate::expr;

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
    /// `trace expect [core] <id> <measure> <op> <value> [within <time>]`: the software trace.
    TraceExpect {
        core: Option<u64>,
        /// The id's name (from `<attach>.trace`) or number; none for `dropped`.
        id: Option<String>,
        measure: TraceMeasure,
        op: Op,
        /// A count, or nanoseconds.
        value: u64,
        within: Duration,
    },
    /// `trace clear`: the trace read so far is forgotten.
    TraceClear,
    /// `trace save <file>`: the events kept as CSV (relative to the script).
    TraceSave(String),
    /// `let <name> = …`.
    Let {
        name: String,
        value: LetValue,
    },
    /// `if <condition>`: on to the `else` or `end` when it does not hold.
    If(String),
    Else,
    /// `while <condition>`: past the `end` when it does not hold.
    While(String),
    /// `repeat <count>`: the lines up to the `end`, that many times.
    Repeat(String),
    /// The end of an `if`, `while` or `repeat`.
    End,
    /// An expectation on script values.
    Check(String),
    Print(String),
    /// A failed expectation, with its message.
    Fail(String),
}

/// What `trace expect` compares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TraceMeasure {
    /// The records of the id that are no span ends.
    Count,
    /// The records the target dropped.
    Dropped,
    /// The spans of the id, `BEGIN` to `END`.
    Span(TraceStat),
    /// From one record of the id to the next.
    Interval(TraceStat),
    /// The share of the time in the id's spans, in hundredths of a percent.
    Load,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TraceStat {
    Min,
    Max,
    Average,
    Last,
}

impl TraceMeasure {
    /// Whether the value is a time.
    pub fn is_time(self) -> bool {
        matches!(self, TraceMeasure::Span(_) | TraceMeasure::Interval(_))
    }
}

/// `750ns`, `2.5us`, `2.5µs`, `10ms`, `1.5s`: nanoseconds.
pub fn parse_time_ns(text: &str) -> Result<u64, String> {
    let bad = || format!("`{text}` is not a time: write 750ns, 2.5us, 10ms or 1s");
    let (number, scale) = [
        ("ns", 1.0),
        ("us", 1e3),
        ("µs", 1e3),
        ("ms", 1e6),
        ("s", 1e9),
    ]
    .iter()
    .find_map(|(unit, scale)| text.strip_suffix(unit).map(|n| (n, *scale)))
    .ok_or_else(bad)?;
    let value: f64 = number.parse().map_err(|_| bad())?;
    if !(value.is_finite() && value >= 0.0) {
        return Err(bad());
    }
    Ok((value * scale).round() as u64)
}

/// The words after `trace expect`.
fn trace_expect(text: &str) -> Result<Command, String> {
    let (rest, within) = take_within(text)?;
    let words: Vec<&str> = rest.split_whitespace().collect();
    // A number first is the core, unless a measure follows it: then it is the id.
    let measure_next = words
        .get(1)
        .is_some_and(|w| matches!(*w, "count" | "span" | "interval" | "load"));
    let (core, words) = match words.first() {
        Some(word) if word.parse::<u64>().is_ok() && !measure_next => {
            (Some(parse_core(word)?), &words[1..])
        }
        _ => (None, &words[..]),
    };
    let usage = "`trace expect` is `trace expect [core] <id> count|span <stat>|interval <stat>|load <op> <value>` \
                 or `trace expect [core] dropped <op> <count>`, stat min, max, avg or last";
    let stat = |word: Option<&&str>| match word.copied() {
        Some("min") => Ok(TraceStat::Min),
        Some("max") => Ok(TraceStat::Max),
        Some("avg") => Ok(TraceStat::Average),
        Some("last") => Ok(TraceStat::Last),
        _ => Err(usage.to_string()),
    };
    let (id, measure, tail) = match words {
        ["dropped", tail @ ..] => (None, TraceMeasure::Dropped, tail),
        [id, "count", tail @ ..] => (Some(id.to_string()), TraceMeasure::Count, tail),
        [id, "span", s, tail @ ..] => (
            Some(id.to_string()),
            TraceMeasure::Span(stat(Some(s))?),
            tail,
        ),
        [id, "load", tail @ ..] => (Some(id.to_string()), TraceMeasure::Load, tail),
        [id, "interval", s, tail @ ..] => (
            Some(id.to_string()),
            TraceMeasure::Interval(stat(Some(s))?),
            tail,
        ),
        _ => return Err(usage.into()),
    };
    let [op, value] = tail else {
        return Err(usage.into());
    };
    let op = Op::parse(op).ok_or_else(|| format!("`{op}` is not ==, !=, <, <=, > or >="))?;
    let value = if measure.is_time() {
        parse_time_ns(value)?
    } else if measure == TraceMeasure::Load {
        let percent: f64 = value
            .trim_end_matches('%')
            .parse()
            .map_err(|_| format!("`{value}` is not a percentage, such as 30% or 0.5%"))?;
        (percent * 100.0).round() as u64
    } else {
        value
            .parse()
            .map_err(|_| format!("`{value}` is not a count"))?
    };
    Ok(Command::TraceExpect {
        core,
        id,
        measure,
        op,
        value,
        within,
    })
}

/// Where a `let` takes its value.
#[derive(Clone, Debug, PartialEq)]
pub enum LetValue {
    /// An expression of script values.
    Expr(String),
    /// `read <expression>`: evaluated in the halted core's top frame.
    Read(String),
    /// `read live <variable>[@core]`: read while the core runs.
    Live { name: String, core: Option<u64> },
    /// `read memory <address>[@core] <1|2|4|8>`: an unsigned little-endian integer.
    Memory {
        address: u64,
        core: Option<u64>,
        size: usize,
    },
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
                | Command::TraceExpect { .. }
                | Command::Check(_)
        )
    }

    /// Whether the command opens or closes a block (`if`, `else`, `while`, `repeat`, `end`).
    pub fn is_block(&self) -> bool {
        matches!(
            self,
            Command::If(_) | Command::Else | Command::While(_) | Command::Repeat(_) | Command::End
        )
    }

    /// The expression the command evaluates, if it has one.
    fn expression(&self) -> Option<&str> {
        match self {
            Command::Let {
                value: LetValue::Expr(e),
                ..
            }
            | Command::If(e)
            | Command::While(e)
            | Command::Repeat(e)
            | Command::Check(e) => Some(e),
            _ => None,
        }
    }

    /// Whether the command needs the session (comes after `attach`).
    fn needs_session(&self) -> bool {
        !matches!(
            self,
            Command::Server(_)
                | Command::Sleep(_)
                | Command::Let {
                    value: LetValue::Expr(_),
                    ..
                }
                | Command::If(_)
                | Command::Else
                | Command::While(_)
                | Command::Repeat(_)
                | Command::End
                | Command::Check(_)
                | Command::Print(_)
                | Command::Fail(_)
        )
    }
}

/// One command and where it is.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub number: usize,
    /// The line as written, without its comment, after `${…}` replacement (a script variable's
    /// `${…}` stays until the line runs).
    pub text: String,
    pub command: Command,
    /// Whether the line has a script variable's `${…}`: it is read again when it runs.
    pub deferred: bool,
    /// Where a block line goes on: for `if`, the line after its `else`, else its `end`, when
    /// the condition does not hold; for `else`, its `end`; for `while` and `repeat`, the line
    /// after the `end`, when done; for the `end` of a loop, the loop's first line. An index of
    /// [`Script::lines`].
    pub jump: Option<usize>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Script {
    pub lines: Vec<Line>,
    /// The `--set` values: the first values of the script variables too.
    pub vars: HashMap<String, String>,
    /// The script variables: the names `let` gives a value.
    pub names: HashSet<String>,
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
        // The script variables, first: their `${…}` waits until the line runs.
        let names: HashSet<String> = text
            .lines()
            .filter_map(|raw| let_target(strip_comment(raw)))
            .collect();
        let mut lines = Vec::new();
        for (ix, raw) in text.lines().enumerate() {
            let number = ix + 1;
            let error = |message: String| ParseError {
                line: number,
                message,
            };
            let (text, deferred) = substitute(
                strip_comment(raw),
                &|name| vars.get(name).cloned().or_else(|| std::env::var(name).ok()),
                &|name| names.contains(name),
            )
            .map_err(error)?;
            let text = text.trim();
            if text.is_empty() {
                continue;
            }
            // A line with a script variable is read with 0 in its place, to find its mistakes.
            let command = if deferred {
                let (placeholder, _) =
                    substitute(text, &|_| Some("0".into()), &|_| false).map_err(error)?;
                parse_command(&placeholder)
            } else {
                parse_command(text)
            }
            .map_err(error)?;
            if let Some(expression) = command.expression() {
                let parsed = expr::parse(expression).map_err(error)?;
                if let Some(name) = parsed.names().into_iter().find(|name| {
                    !(names.contains(*name) || vars.contains_key(*name))
                        && !matches!(*name, "true" | "false")
                }) {
                    return Err(error(format!(
                        "`{name}` is not a script variable: give it a value with `let {name} = …` or --set {name}=…"
                    )));
                }
            }
            lines.push(Line {
                number,
                text: text.to_string(),
                command,
                deferred,
                jump: None,
            });
        }
        let attach = lines
            .iter()
            .position(|l| matches!(l.command, Command::Attach(_)));
        for line in &lines {
            let before_attach = attach.is_none_or(|at| lines[at].number > line.number);
            if line.command.needs_session()
                && !matches!(line.command, Command::Attach(_))
                && before_attach
            {
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
        match_blocks(&mut lines)?;
        Ok(Script {
            lines,
            vars: vars.clone(),
            names,
        })
    }

    /// The expectations, in order.
    pub fn expectations(&self) -> impl Iterator<Item = &Line> {
        self.lines.iter().filter(|l| l.command.is_expectation())
    }

    /// A line as it runs now, `vars` holding the script variables: its text and its command,
    /// read again when it has a script variable.
    pub fn expand(
        &self,
        line: &Line,
        vars: &HashMap<String, String>,
    ) -> Result<(String, Command), String> {
        if !line.deferred {
            return Ok((line.text.clone(), line.command.clone()));
        }
        let (text, _) = substitute(
            &line.text,
            &|name| vars.get(name).cloned().or_else(|| std::env::var(name).ok()),
            &|_| false,
        )?;
        let command = parse_command(&text)?;
        // The blocks were matched when the script was read.
        if std::mem::discriminant(&command) != std::mem::discriminant(&line.command) {
            return Err(format!("`{text}` became another command"));
        }
        Ok((text, command))
    }
}

/// The name a `let` line gives a value.
fn let_target(line: &str) -> Option<String> {
    let rest = line.trim().strip_prefix("let")?;
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let (name, _) = rest.split_once('=')?;
    let name = name.trim();
    is_name(name).then(|| name.to_string())
}

/// A variable name: a letter or `_`, then letters, digits and `_`.
fn is_name(text: &str) -> bool {
    let mut chars = text.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Sets the [`Line::jump`] of the block lines, and finds the blocks left open or closed twice.
fn match_blocks(lines: &mut [Line]) -> Result<(), ParseError> {
    enum Open {
        If { at: usize, otherwise: Option<usize> },
        Loop { at: usize },
    }
    let mut open: Vec<Open> = Vec::new();
    for at in 0..lines.len() {
        let error = |message: &str| ParseError {
            line: lines[at].number,
            message: message.into(),
        };
        match lines[at].command {
            Command::If(_) => open.push(Open::If {
                at,
                otherwise: None,
            }),
            Command::While(_) | Command::Repeat(_) => open.push(Open::Loop { at }),
            Command::Else => match open.last_mut() {
                Some(Open::If { otherwise, .. }) if otherwise.is_none() => *otherwise = Some(at),
                Some(Open::If { .. }) => return Err(error("a second `else` for one `if`")),
                _ => return Err(error("`else` without its `if`")),
            },
            Command::End => match open.pop() {
                Some(Open::If {
                    at: start,
                    otherwise,
                }) => match otherwise {
                    Some(otherwise) => {
                        lines[start].jump = Some(otherwise + 1);
                        lines[otherwise].jump = Some(at);
                    }
                    None => lines[start].jump = Some(at),
                },
                Some(Open::Loop { at: start }) => {
                    lines[start].jump = Some(at + 1);
                    lines[at].jump = Some(start);
                }
                None => return Err(error("`end` without its `if`, `while` or `repeat`")),
            },
            _ => {}
        }
    }
    match open.pop() {
        None => Ok(()),
        Some(Open::If { at, .. } | Open::Loop { at }) => Err(ParseError {
            line: lines[at].number,
            message: format!("`{}` without its `end`", lines[at].text),
        }),
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

/// Replaces each `${NAME}` and `${NAME:-default}` by what `lookup` gives, else the default;
/// leaves those `keep` names as they are. Also tells whether it left one.
fn substitute(
    line: &str,
    lookup: &dyn Fn(&str) -> Option<String>,
    keep: &dyn Fn(&str) -> bool,
) -> Result<(String, bool), String> {
    let mut out = String::new();
    let mut kept = false;
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
        if keep(name) {
            out.push_str(&rest[start..start + 2 + end + 1]);
            kept = true;
            rest = &after[end + 1..];
            continue;
        }
        let value = lookup(name)
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
    Ok((out, kept))
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

/// What comes after `let <name> =`.
fn let_value(text: &str) -> Result<LetValue, String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    match words[..] {
        ["read", "live", location] => {
            let (name, core) = match location.split_once('@') {
                Some((name, core)) => (name.to_string(), Some(parse_core(core)?)),
                None => (location.to_string(), None),
            };
            Ok(LetValue::Live { name, core })
        }
        ["read", "live", ..] => Err("`read live` takes one <variable>[@core]".into()),
        ["read", "memory", location, size] => {
            let (address, core) = parse_location(location)?;
            let size = match size {
                "1" => 1,
                "2" => 2,
                "4" => 4,
                "8" => 8,
                _ => return Err(format!("`{size}`: `read memory` reads 1, 2, 4 or 8 bytes")),
            };
            Ok(LetValue::Memory {
                address,
                core,
                size,
            })
        }
        ["read", "memory", ..] => {
            Err("`read memory` is `read memory <address>[@core] <1|2|4|8>`".into())
        }
        ["read"] => Err("`read` needs what to read".into()),
        ["read", ..] => Ok(LetValue::Read(
            text.strip_prefix("read").unwrap_or(text).trim().to_string(),
        )),
        _ => {
            expr::parse(text)?;
            Ok(LetValue::Expr(text.to_string()))
        }
    }
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
        "trace" => match args.first() {
            Some(&"expect") => trace_expect(after(2)),
            Some(&"clear") => {
                no_more(1)?;
                Ok(Command::TraceClear)
            }
            Some(&"save") => {
                let file = after(2);
                if file.is_empty() {
                    return Err("`trace save` needs a file".into());
                }
                Ok(Command::TraceSave(file.trim_matches('"').to_string()))
            }
            _ => Err("`trace` is `trace expect …`, `trace clear` or `trace save <file>`".into()),
        },
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
        "let" => {
            let (name, value) = after(1)
                .split_once('=')
                .ok_or("`let` is `let <name> = <value>`")?;
            let name = name.trim();
            if !is_name(name) {
                return Err(format!(
                    "`{name}` is not a variable name (letters, digits and _)"
                ));
            }
            Ok(Command::Let {
                name: name.to_string(),
                value: let_value(value.trim())?,
            })
        }
        "if" | "while" | "repeat" | "check" => {
            let expression = after(1);
            if expression.is_empty() {
                return Err(format!(
                    "`{keyword}` needs {}",
                    if keyword == "repeat" {
                        "a count"
                    } else {
                        "a condition"
                    }
                ));
            }
            expr::parse(expression)?;
            let expression = expression.to_string();
            Ok(match keyword {
                "if" => Command::If(expression),
                "while" => Command::While(expression),
                "repeat" => Command::Repeat(expression),
                _ => Command::Check(expression),
            })
        }
        "else" | "end" => {
            no_more(0)?;
            Ok(if keyword == "else" {
                Command::Else
            } else {
                Command::End
            })
        }
        "print" | "fail" => {
            let text = after(1);
            let text = if text.starts_with('"') {
                let (text, rest) = quoted(text)?;
                if !rest.trim().is_empty() {
                    return Err(format!("`{}` after the text", rest.trim()));
                }
                text
            } else {
                text.to_string()
            };
            Ok(if keyword == "print" {
                Command::Print(text)
            } else if text.is_empty() {
                Command::Fail("fail".into())
            } else {
                Command::Fail(text)
            })
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
    fn blocks_jump() {
        let script = parse(
            "let n = 0\nrepeat 3\nif n % 2 == 0\nlet n = n + 1\nelse\nlet n = n + 2\nend\nend\n\
             while n < 10\nlet n = n * 2\nend\ncheck n == 10\n",
        )
        .unwrap();
        let jumps: Vec<_> = script.lines.iter().map(|l| l.jump).collect();
        assert_eq!(
            jumps,
            [
                None,
                Some(8), // repeat: past its end
                Some(5), // if: after the else
                None,
                Some(6), // else: its end
                None,
                None,     // the end of the if
                Some(1),  // the end of the repeat: back to it
                Some(11), // while: past its end
                None,
                Some(8), // back to the while
                None,
            ]
        );
        assert!(script.names.contains("n"));
        assert_eq!(script.expectations().count(), 1);
    }

    #[test]
    fn block_mistakes() {
        assert_eq!(parse("if 1\nlet a = 1\n").unwrap_err().line, 1);
        assert_eq!(parse("let a = 1\nend\n").unwrap_err().line, 2);
        assert_eq!(parse("repeat 2\nelse\nend\n").unwrap_err().line, 2);
        assert_eq!(parse("if 1\nelse\nelse\nend\n").unwrap_err().line, 3);
        assert_eq!(parse("while\nend\n").unwrap_err().line, 1);
        assert_eq!(parse("if 1 +\nend\n").unwrap_err().line, 1);
        // A name nothing gives a value.
        let error = parse("check m == 1\n").unwrap_err();
        assert!(error.message.contains("let m"), "{error}");
        // Reading the target needs the session.
        assert_eq!(
            parse("let a = read k\nattach a.json\n").unwrap_err().line,
            1
        );
    }

    #[test]
    fn let_values() {
        assert_eq!(
            one("let a = read p->state"),
            Command::Let {
                name: "a".into(),
                value: LetValue::Read("p->state".into())
            }
        );
        assert_eq!(
            one("let t = read live g_ticks@1"),
            Command::Let {
                name: "t".into(),
                value: LetValue::Live {
                    name: "g_ticks".into(),
                    core: Some(1)
                }
            }
        );
        assert_eq!(
            one("let w = read memory 0x7000A000@0 4"),
            Command::Let {
                name: "w".into(),
                value: LetValue::Memory {
                    address: 0x7000_a000,
                    core: Some(0),
                    size: 4
                }
            }
        );
        assert!(parse("attach a.json\nlet w = read memory 0x7000A000 3").is_err());
        assert!(parse("attach a.json\nlet 2w = 1").is_err());
        assert_eq!(
            one("print \"n = 3\" # done"),
            Command::Print("n = 3".into())
        );
        assert_eq!(one("fail"), Command::Fail("fail".into()));
    }

    #[test]
    fn script_variables_wait_for_their_line() {
        let vars = HashMap::from([("BASE".to_string(), "0x7000A000".to_string())]);
        let script = Script::parse(
            "attach a.json\nlet n = 2\nexpect memory ${BASE}@${n} 2A\nlet x = ${n:-5} + 1\n",
            &vars,
        )
        .unwrap();
        let line = &script.lines[2];
        assert!(line.deferred);
        assert_eq!(line.text, "expect memory 0x7000A000@${n} 2A");
        let now = HashMap::from([("n".to_string(), "1".to_string())]);
        let (text, command) = script.expand(line, &now).unwrap();
        assert_eq!(text, "expect memory 0x7000A000@1 2A");
        assert_eq!(
            command,
            Command::ExpectMemory {
                address: 0x7000_a000,
                core: Some(1),
                bytes: vec![0x2a]
            }
        );
        // A default when the variable has no value yet.
        let (text, _) = script.expand(&script.lines[3], &HashMap::new()).unwrap();
        assert_eq!(text, "let x = 5 + 1");
        // Mistakes in such a line are found before the run too.
        assert!(
            Script::parse("let n = 1\nattach a.json\nexpect memory 0x0@${n}\n", &vars).is_err()
        );
    }

    #[test]
    fn trace_expectations() {
        assert_eq!(
            one("trace expect 0 timer_isr count >= 50 within 1s"),
            Command::TraceExpect {
                core: Some(0),
                id: Some("timer_isr".into()),
                measure: TraceMeasure::Count,
                op: Op::Ge,
                value: 50,
                within: Duration::from_secs(1)
            }
        );
        assert_eq!(
            one("trace expect TaskA span max <= 450ms"),
            Command::TraceExpect {
                core: None,
                id: Some("TaskA".into()),
                measure: TraceMeasure::Span(TraceStat::Max),
                op: Op::Le,
                value: 450_000_000,
                within: DEFAULT_WAIT
            }
        );
        assert!(matches!(
            one("trace expect 10 interval avg > 9.5ms"),
            Command::TraceExpect {
                core: None,
                measure: TraceMeasure::Interval(TraceStat::Average),
                value: 9_500_000,
                ..
            }
        ));
        assert!(matches!(
            one("trace expect dropped == 0"),
            Command::TraceExpect {
                id: None,
                measure: TraceMeasure::Dropped,
                value: 0,
                ..
            }
        ));
        assert_eq!(one("trace clear"), Command::TraceClear);
        assert!(matches!(
            one("trace expect 0 TaskA load >= 33.3%"),
            Command::TraceExpect {
                core: Some(0),
                measure: TraceMeasure::Load,
                value: 3330,
                ..
            }
        ));
        assert_eq!(
            one("trace save \"out/trace.csv\""),
            Command::TraceSave("out/trace.csv".into())
        );
        assert_eq!(parse_time_ns("2.75us").unwrap(), 2_750);
        assert_eq!(parse_time_ns("2.75µs").unwrap(), 2_750);
        for bad in [
            "trace expect isr span 3ms",
            "trace expect isr span max <= 3",
            "trace expect isr count >= x",
            "trace expect isr",
            "trace look",
        ] {
            assert!(parse(&format!("attach a.json\n{bad}")).is_err(), "{bad}");
        }
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
