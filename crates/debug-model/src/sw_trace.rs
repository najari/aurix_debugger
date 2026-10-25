//! The software trace of a program that writes `aurix_trace.h` records to an RTT up channel in
//! binary (`BinaryLE`): when each event happened on each core, how often an id comes and how
//! long its spans take (a `BEGIN` to the next `END` of the same id on the same core).
//!
//! A record is 8 bytes, little endian: the STM0 `TIM0` count (u32), the id (u16, bit 15 for the
//! end of a span) and an argument (u16). The id [`ID_DROPPED`] says how many records the target
//! dropped (its argument), because its buffer was full. probe-rs sends the bytes of a binary
//! channel as text, `0x` and two hex digits each ([`binary_bytes`]).

use std::collections::{BTreeMap, HashMap, VecDeque};

/// The bytes of a record.
pub const RECORD_SIZE: usize = 8;
/// The end of a span.
pub const END_BIT: u16 = 0x8000;
/// A record that counts the records dropped before it.
pub const ID_DROPPED: u16 = 0x7FFF;
/// The STM clock of the TC27x as the demos run it.
pub const DEFAULT_TICK_HZ: u64 = 100_000_000;
/// How many events are kept (for the list and the chart): some seconds of a busy core.
pub const KEEP: usize = 20_000;

/// The bytes of a binary RTT channel's data as probe-rs sends them: `0x0a0x00…`.
pub fn binary_bytes(text: &str) -> Result<Vec<u8>, String> {
    let text: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    let mut out = Vec::with_capacity(text.len() / 4);
    let mut rest = text.as_str();
    while !rest.is_empty() {
        let digits = rest
            .strip_prefix("0x")
            .and_then(|r| r.get(..2).map(|d| (d, &r[2..])));
        let Some((digits, after)) = digits else {
            return Err(format!(
                "`{}` is not 0x and two hex digits",
                &rest[..rest.len().min(8)]
            ));
        };
        out.push(
            u8::from_str_radix(digits, 16).map_err(|_| format!("`0x{digits}` is not a byte"))?,
        );
        rest = after;
    }
    Ok(out)
}

/// The names of the ids: `id = name` a line (`0x` hex or decimal), `#` comments.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Names {
    names: HashMap<u16, String>,
}

impl Names {
    pub fn parse(text: &str) -> Result<Names, String> {
        let mut names = HashMap::new();
        for (ix, line) in text.lines().enumerate() {
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let (id, name) = line
                .split_once('=')
                .ok_or_else(|| format!("line {}: write `id = name`", ix + 1))?;
            let id = id.trim();
            let value = match id.strip_prefix("0x").or_else(|| id.strip_prefix("0X")) {
                Some(hex) => u16::from_str_radix(hex, 16),
                None => id.parse(),
            }
            .map_err(|_| format!("line {}: `{id}` is not an id (0 to 0x7FFE)", ix + 1))?;
            if value & END_BIT != 0 || value == ID_DROPPED {
                return Err(format!(
                    "line {}: `{id}` is not an id (0 to 0x7FFE)",
                    ix + 1
                ));
            }
            let name = name.trim();
            if name.is_empty() {
                return Err(format!("line {}: a name after `=`", ix + 1));
            }
            names.insert(value, name.to_string());
        }
        Ok(Names { names })
    }

    /// The name of `id`, else the id as a number.
    pub fn name(&self, id: u16) -> String {
        if id == ID_DROPPED {
            return "(dropped)".into();
        }
        self.names
            .get(&id)
            .cloned()
            .unwrap_or_else(|| format!("id {id}"))
    }

    /// The id of `name` (or of `id 7`, or a number).
    pub fn id(&self, name: &str) -> Option<u16> {
        if let Some((id, _)) = self.names.iter().find(|(_, n)| n.as_str() == name) {
            return Some(*id);
        }
        let number = name.strip_prefix("id ").unwrap_or(name).trim();
        match number.strip_prefix("0x") {
            Some(hex) => u16::from_str_radix(hex, 16).ok(),
            None => number.parse().ok(),
        }
    }
}

/// What a record says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Begin,
    End,
    /// A record that is no span (`AURIX_TRACE(id, arg)` of an id never ended).
    Event,
    /// [`ID_DROPPED`]: the argument records were dropped.
    Dropped,
}

/// One record, read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Event {
    /// The core of the channel, in a session of several cores.
    pub core: Option<u64>,
    /// STM ticks, counted on past each round of the 32-bit counter.
    pub time: u64,
    pub id: u16,
    pub end: bool,
    pub arg: u16,
    /// The length of the span this `END` closes, in ticks.
    pub span: Option<u64>,
}

/// Minimum, sum, maximum and count of some lengths in ticks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Spread {
    pub count: u64,
    pub min: u64,
    pub max: u64,
    pub sum: u64,
    pub last: u64,
}

impl Spread {
    fn add(&mut self, value: u64) {
        if self.count == 0 || value < self.min {
            self.min = value;
        }
        self.max = self.max.max(value);
        self.sum += value;
        self.count += 1;
        self.last = value;
    }

    pub fn average(&self) -> Option<u64> {
        (self.count > 0).then(|| self.sum / self.count)
    }
}

/// What is known of one id on one core.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IdStats {
    /// Its records that are no `END`.
    pub count: u64,
    /// From one such record to the next.
    pub interval: Spread,
    /// The spans, `BEGIN` to `END`.
    pub span: Spread,
    last_start: Option<u64>,
    open: Option<u64>,
    /// The ticks in its spans since the core's last drop (for the load).
    busy: u64,
}

/// What the trace says of one named id ([`SwTrace::timing_of`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Timing {
    pub core: Option<u64>,
    /// The share of the time in its spans, 0 to 1.
    pub load: Option<f64>,
    /// The last span, in ticks.
    pub last_span: Option<u64>,
    /// The records that began a span or were one.
    pub runs: u64,
}

/// One line of the statistics.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatRow {
    pub core: Option<u64>,
    pub id: u16,
    pub name: String,
    pub stats: IdStats,
}

#[derive(Clone, Debug, Default)]
struct CoreState {
    /// Bytes of a record not complete yet.
    partial: Vec<u8>,
    last_raw: Option<u32>,
    high: u64,
}

/// The software trace of a session.
#[derive(Clone, Debug)]
pub struct SwTrace {
    tick_hz: u64,
    names: Names,
    cores: HashMap<Option<u64>, CoreState>,
    events: VecDeque<Event>,
    stats: BTreeMap<(Option<u64>, u16), IdStats>,
    /// Records the target dropped, per core.
    dropped: BTreeMap<Option<u64>, u64>,
    total: u64,
    first: Option<u64>,
    /// Per core, the time the load is measured from (the first record, or the last drop) and
    /// the last record's.
    period: BTreeMap<Option<u64>, (u64, u64)>,
}

impl Default for SwTrace {
    fn default() -> Self {
        Self::new(Names::default())
    }
}

impl SwTrace {
    pub fn new(names: Names) -> Self {
        Self {
            tick_hz: DEFAULT_TICK_HZ,
            names,
            cores: HashMap::new(),
            events: VecDeque::new(),
            stats: BTreeMap::new(),
            dropped: BTreeMap::new(),
            total: 0,
            first: None,
            period: BTreeMap::new(),
        }
    }

    /// The share of the time `id` spent in its spans on `core`, 0 to 1, since the core's first
    /// record or last drop (nested spans count in each: an ISR inside a task counts for both).
    /// `None` before two records, or for an id that was never ended.
    pub fn load(&self, core: Option<u64>, id: u16) -> Option<f64> {
        let stats = self.stats.get(&(core, id))?;
        let (from, to) = *self.period.get(&core)?;
        let mut busy = stats.busy;
        // A span begun and not ended counts up to now.
        if let Some(begin) = stats.open
            && stats.span.count > 0
        {
            busy += to - begin.max(from);
        }
        (to > from && stats.span.count > 0).then(|| busy as f64 / (to - from) as f64)
    }

    /// The timing of the id named `name` (an OS object of the ORTI file, say): its load, its last
    /// span and how many records began one. On several cores, the core with spans first.
    pub fn timing_of(&self, name: &str) -> Option<Timing> {
        let id = self.names.names.iter().find(|(_, n)| n.as_str() == name)?.0;
        let ((core, _), stats) = self
            .stats
            .iter()
            .filter(|((_, i), _)| i == id)
            .max_by_key(|(_, s)| s.span.count)?;
        Some(Timing {
            core: *core,
            load: self.load(*core, *id),
            last_span: (stats.span.count > 0).then_some(stats.span.last),
            runs: stats.count,
        })
    }

    /// The events kept as CSV: time since the first event (ns), core, id, name, kind, arg and
    /// the span an `end` closes (ns).
    pub fn to_csv(&self) -> String {
        let ns = |ticks: u64| (ticks as u128 * 1_000_000_000 / self.tick_hz as u128) as u64;
        let mut out = String::from("time_ns,core,id,name,kind,arg,span_ns\n");
        for event in &self.events {
            let kind = match self.kind(event) {
                Kind::Begin => "begin",
                Kind::End => "end",
                Kind::Event => "event",
                Kind::Dropped => "dropped",
            };
            let name = self.names.name(event.id).replace(['"', ','], "_");
            out.push_str(&format!(
                "{},{},{},{name},{kind},{},{}\n",
                ns(self.since_start(event.time)),
                event.core.map(|c| c.to_string()).unwrap_or_default(),
                event.id,
                event.arg,
                event.span.map(|s| ns(s).to_string()).unwrap_or_default(),
            ));
        }
        out
    }

    pub fn names(&self) -> &Names {
        &self.names
    }

    pub fn set_names(&mut self, names: Names) {
        self.names = names;
    }

    pub fn tick_hz(&self) -> u64 {
        self.tick_hz
    }

    /// The events kept, oldest first.
    pub fn events(&self) -> &VecDeque<Event> {
        &self.events
    }

    /// The records read.
    pub fn total(&self) -> u64 {
        self.total
    }

    /// The records the target dropped, per core.
    pub fn dropped(&self) -> &BTreeMap<Option<u64>, u64> {
        &self.dropped
    }

    /// The ticks since the first event read.
    pub fn since_start(&self, time: u64) -> u64 {
        time.saturating_sub(self.first.unwrap_or(time))
    }

    /// Ticks as text: `2.75 µs`, `10.00 ms`, `0.400 s`.
    pub fn format(&self, ticks: u64) -> String {
        format_ns((ticks as u128 * 1_000_000_000 / self.tick_hz as u128) as u64)
    }

    /// Forgets the events and statistics read so far. The names stay, and so do the bytes of a
    /// record not complete yet and the rounds of the counter, so that the next records read
    /// right.
    pub fn clear(&mut self) {
        self.events.clear();
        self.stats.clear();
        self.dropped.clear();
        self.total = 0;
        self.first = None;
        self.period.clear();
    }

    /// Bytes of the channel of `core`; returns how many events they made.
    pub fn received(&mut self, core: Option<u64>, bytes: &[u8]) -> usize {
        let state = self.cores.entry(core).or_default();
        state.partial.extend_from_slice(bytes);
        let whole = state.partial.len() / RECORD_SIZE * RECORD_SIZE;
        let records: Vec<[u8; RECORD_SIZE]> =
            state.partial[..whole].as_chunks::<RECORD_SIZE>().0.to_vec();
        state.partial.drain(..whole);
        for record in &records {
            self.record(core, record);
        }
        records.len()
    }

    fn record(&mut self, core: Option<u64>, record: &[u8; RECORD_SIZE]) {
        let raw = u32::from_le_bytes([record[0], record[1], record[2], record[3]]);
        let word = u16::from_le_bytes([record[4], record[5]]);
        let arg = u16::from_le_bytes([record[6], record[7]]);
        let state = self.cores.entry(core).or_default();
        // The counter goes round every 2^32 ticks: a time before the last is the next round.
        if state.last_raw.is_some_and(|last| raw < last) {
            state.high += 1 << 32;
        }
        state.last_raw = Some(raw);
        let time = state.high + u64::from(raw);
        self.first.get_or_insert(time);
        self.total += 1;
        let end = word & END_BIT != 0;
        let id = word & !END_BIT;
        let mut span = None;
        let period = self.period.entry(core).or_insert((time, time));
        period.1 = time;
        if id == ID_DROPPED && !end {
            *self.dropped.entry(core).or_default() += u64::from(arg);
            // Records are missing here: no interval or span of this core reaches across, and
            // the load is measured from here on.
            *period = (time, time);
            for ((c, _), stats) in self.stats.iter_mut() {
                if *c == core {
                    stats.last_start = None;
                    stats.open = None;
                    stats.busy = 0;
                }
            }
        } else {
            let from = period.0;
            let stats = self.stats.entry((core, id)).or_default();
            if end {
                if let Some(begin) = stats.open.take() {
                    let length = time - begin;
                    stats.span.add(length);
                    stats.busy += time - begin.max(from);
                    span = Some(length);
                }
            } else {
                stats.count += 1;
                if let Some(last) = stats.last_start {
                    stats.interval.add(time - last);
                }
                stats.last_start = Some(time);
                stats.open = Some(time);
            }
        }
        self.events.push_back(Event {
            core,
            time,
            id,
            end,
            arg,
            span,
        });
        while self.events.len() > KEEP {
            self.events.pop_front();
        }
    }

    /// What `event` is.
    pub fn kind(&self, event: &Event) -> Kind {
        if event.id == ID_DROPPED {
            Kind::Dropped
        } else if event.end {
            Kind::End
        } else if self
            .stats
            .get(&(event.core, event.id))
            .is_some_and(|s| s.span.count > 0)
        {
            // An id that was ended on its core: its records begin spans.
            Kind::Begin
        } else {
            Kind::Event
        }
    }

    /// The statistics, by core and id.
    pub fn stats(&self) -> Vec<StatRow> {
        self.stats
            .iter()
            .map(|((core, id), stats)| StatRow {
                core: *core,
                id: *id,
                name: self.names.name(*id),
                stats: *stats,
            })
            .collect()
    }

    /// The statistics of one id on `core` (any core when `None` and there is one).
    pub fn stats_of(&self, core: Option<u64>, id: u16) -> Option<IdStats> {
        self.stats.get(&(core, id)).copied().or_else(|| {
            let mut all = self.stats.iter().filter(|((_, i), _)| *i == id);
            match (core, all.next(), all.next()) {
                (None, Some((_, s)), None) => Some(*s),
                _ => None,
            }
        })
    }
}

/// One row of the chart: an id on a core, its spans and single records in the window, in
/// columns (`0..columns`) merged where they touch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChartRow {
    pub core: Option<u64>,
    pub id: u16,
    pub name: String,
    /// The spans, as column ranges `start..end` (at least one column wide).
    pub segments: Vec<(u32, u32)>,
    /// The records that are no span, as columns.
    pub marks: Vec<u32>,
    /// The spans that reach into the window (whole or not).
    pub spans: usize,
}

/// The spans and records of the last `window` ticks, in `columns` columns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chart {
    /// The window, in ticks since the first event read.
    pub start: u64,
    pub end: u64,
    pub columns: u32,
    pub rows: Vec<ChartRow>,
}

impl SwTrace {
    /// The chart of the last `window` ticks before the last event, `columns` columns wide. A
    /// span still open runs to the end of the window. `None` before the first event.
    pub fn chart(&self, window: u64, columns: u32) -> Option<Chart> {
        let last = self.events.back()?.time;
        let window = window.max(1);
        let columns = columns.max(1);
        let from = last.saturating_sub(window);
        let column = |time: u64| -> u32 {
            let offset = time.saturating_sub(from).min(window);
            ((offset as u128 * columns as u128) / window as u128) as u32
        };
        let mut rows: BTreeMap<(Option<u64>, u16), ChartRow> = self
            .stats
            .iter()
            .map(|((core, id), _)| {
                (
                    (*core, *id),
                    ChartRow {
                        core: *core,
                        id: *id,
                        name: self.names.name(*id),
                        segments: Vec::new(),
                        marks: Vec::new(),
                        spans: 0,
                    },
                )
            })
            .collect();
        let add_span = |row: &mut ChartRow, begin: u64, end: u64| {
            if end < from {
                return;
            }
            let a = column(begin);
            let b = column(end).max(a + 1).min(columns);
            let a = a.min(b.saturating_sub(1));
            row.spans += 1;
            match row.segments.last_mut() {
                Some(last) if a <= last.1 => last.1 = last.1.max(b),
                _ => row.segments.push((a, b)),
            }
        };
        for event in &self.events {
            if event.id == ID_DROPPED {
                continue;
            }
            let Some(row) = rows.get_mut(&(event.core, event.id)) else {
                continue;
            };
            if let Some(span) = event.span {
                add_span(row, event.time - span, event.time);
            } else if !event.end
                && event.time >= from
                && self
                    .stats
                    .get(&(event.core, event.id))
                    .is_some_and(|s| s.span.count == 0)
            {
                let c = column(event.time).min(columns - 1);
                if row.marks.last() != Some(&c) {
                    row.marks.push(c);
                }
            }
        }
        // Spans begun and not ended yet run to the end of the window.
        for ((core, id), stats) in &self.stats {
            if let (Some(begin), true) = (stats.open, stats.span.count > 0)
                && let Some(row) = rows.get_mut(&(*core, *id))
            {
                add_span(row, begin, last);
            }
        }
        Some(Chart {
            start: self.since_start(from),
            end: self.since_start(last),
            columns,
            rows: rows.into_values().collect(),
        })
    }
}

/// Nanoseconds as text, three significant digits or so.
pub fn format_ns(ns: u64) -> String {
    let ns_f = ns as f64;
    if ns < 1_000 {
        format!("{ns} ns")
    } else if ns < 1_000_000 {
        format!("{:.2} µs", ns_f / 1e3)
    } else if ns < 1_000_000_000 {
        format!("{:.2} ms", ns_f / 1e6)
    } else {
        format!("{:.3} s", ns_f / 1e9)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(time: u32, id: u16, arg: u16) -> Vec<u8> {
        let mut out = time.to_le_bytes().to_vec();
        out.extend_from_slice(&id.to_le_bytes());
        out.extend_from_slice(&arg.to_le_bytes());
        out
    }

    fn text(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:#04x}")).collect()
    }

    #[test]
    fn bytes_as_probe_rs_sends_them() {
        assert_eq!(binary_bytes("0x0a0x000xff").unwrap(), [10, 0, 255]);
        assert_eq!(binary_bytes("").unwrap(), Vec::<u8>::new());
        assert!(binary_bytes("0x0").is_err());
        assert!(binary_bytes("hello").is_err());
    }

    #[test]
    fn names_from_a_file() {
        let names = Names::parse("# the demo\n1 = TaskA\n0x0A = timer_isr  # ISR\n\n").unwrap();
        assert_eq!(names.name(10), "timer_isr");
        assert_eq!(names.name(5), "id 5");
        assert_eq!(names.id("TaskA"), Some(1));
        assert_eq!(names.id("id 5"), Some(5));
        assert!(Names::parse("x = y").is_err());
        assert!(Names::parse("0x8001 = end").is_err());
        assert!(Names::parse("3 =").is_err());
    }

    #[test]
    fn spans_intervals_and_records_split_across_reads() {
        let mut trace = SwTrace::new(Names::parse("10 = isr\n20 = dispatch").unwrap());
        let mut bytes = Vec::new();
        for n in 0..3u32 {
            let t = 1_000 + n * 1_000_000;
            bytes.extend(record(t, 10, 0));
            bytes.extend(record(t + 275, 10 | END_BIT, 0));
        }
        bytes.extend(record(3_500_000, 20, 2));
        // In two pieces, the first ending inside a record.
        assert_eq!(trace.received(None, &bytes[..13]), 1);
        assert_eq!(trace.received(None, &bytes[13..]), 6);
        let isr = trace.stats_of(None, 10).unwrap();
        assert_eq!(isr.count, 3);
        assert_eq!((isr.span.min, isr.span.max, isr.span.count), (275, 275, 3));
        assert_eq!(isr.interval.average(), Some(1_000_000));
        assert_eq!(trace.format(isr.span.last), "2.75 µs");
        assert_eq!(trace.format(isr.interval.max), "10.00 ms");
        let dispatch = trace.stats_of(None, 20).unwrap();
        assert_eq!((dispatch.count, dispatch.span.count), (1, 0));
        let events = trace.events();
        assert_eq!(trace.kind(&events[0]), Kind::Begin);
        assert_eq!(trace.kind(&events[1]), Kind::End);
        assert_eq!(events[1].span, Some(275));
        assert_eq!(trace.kind(&events[6]), Kind::Event);
        assert_eq!(trace.since_start(events[6].time), 3_499_000);
        // Through the text probe-rs sends.
        let mut again = SwTrace::default();
        assert_eq!(
            again.received(Some(1), &binary_bytes(&text(&bytes)).unwrap()),
            7
        );
        assert!(again.stats_of(None, 10).is_some());
        assert!(again.stats_of(Some(0), 10).is_none());
    }

    #[test]
    fn the_counter_goes_round_and_drops_are_counted() {
        let mut trace = SwTrace::default();
        let mut bytes = record(u32::MAX - 99, 1, 0);
        bytes.extend(record(150, 1 | END_BIT, 0));
        bytes.extend(record(200, 1, 0));
        bytes.extend(record(300, ID_DROPPED, 42));
        bytes.extend(record(400, 1 | END_BIT, 0));
        bytes.extend(record(500, 1, 0));
        trace.received(Some(0), &bytes);
        let task = trace.stats_of(Some(0), 1).unwrap();
        // From 2^32 - 100 past the round to 150: 250 ticks.
        assert_eq!(task.span.last, 250);
        // The drop ends the span begun at 200 and the interval from 200 to 500.
        assert_eq!(task.span.count, 1);
        assert_eq!(task.interval.count, 1);
        assert_eq!(task.interval.last, 300);
        assert_eq!(trace.dropped().get(&Some(0)), Some(&42));
        assert_eq!(trace.kind(&trace.events()[3]), Kind::Dropped);
        // A clear between the halves of a record keeps the record whole.
        let next = record(200, 2, 7);
        trace.received(Some(0), &next[..3]);
        trace.clear();
        assert_eq!(trace.total(), 0);
        assert_eq!(trace.received(Some(0), &next[3..]), 1);
        assert_eq!(trace.events()[0].arg, 7);
    }

    #[test]
    fn a_chart_of_the_last_window() {
        let mut trace = SwTrace::new(Names::parse("1 = task\n10 = isr\n20 = dispatch").unwrap());
        let mut bytes = Vec::new();
        // A task from 0 to 600, ISRs of 2 ticks every 100, a dispatch at 650, a task begun at
        // 700 and not ended.
        bytes.extend(record(0, 1, 0));
        for t in (100..=500).step_by(100) {
            bytes.extend(record(t, 10, 0));
            bytes.extend(record(t + 2, 10 | END_BIT, 0));
        }
        bytes.extend(record(600, 1 | END_BIT, 0));
        bytes.extend(record(650, 20, 1));
        bytes.extend(record(700, 1, 0));
        bytes.extend(record(1000, 10, 0));
        bytes.extend(record(1002, 10 | END_BIT, 0));
        trace.received(None, &bytes);

        // The last 1000 ticks in 100 columns: a column is 10 ticks.
        let chart = trace.chart(1000, 100).unwrap();
        assert_eq!((chart.start, chart.end), (2, 1002));
        let row = |name: &str| chart.rows.iter().find(|r| r.name == name).unwrap();
        // The task from 0 (before the window, cut at its start) to 600, and the open one from
        // 700 to the end.
        assert_eq!(row("task").segments, [(0, 59), (69, 100)]);
        assert_eq!(row("task").spans, 2);
        // Six ISRs, each at least a column wide.
        assert_eq!(
            row("isr").segments,
            [(9, 10), (19, 20), (29, 30), (39, 40), (49, 50), (99, 100)]
        );
        assert_eq!(row("dispatch").marks, [64]);
        // A narrow chart merges what touches.
        let narrow = trace.chart(1000, 2).unwrap();
        let task = narrow.rows.iter().find(|r| r.name == "task").unwrap();
        assert_eq!(task.segments, [(0, 2)]);
        assert!(SwTrace::default().chart(10, 10).is_none());
    }

    #[test]
    fn load_and_csv() {
        let mut trace = SwTrace::new(Names::parse("1 = task\n10 = isr\n20 = dispatch").unwrap());
        let mut bytes = Vec::new();
        // Before a drop: a task of 900 ticks, not counted.
        bytes.extend(record(0, 1, 0));
        bytes.extend(record(900, 1 | END_BIT, 0));
        bytes.extend(record(1000, ID_DROPPED, 5));
        // From 1000 to 2000: the task 1000..1300 and 1500..2000 (open), an ISR of 10 ticks.
        bytes.extend(record(1000, 1, 0));
        bytes.extend(record(1100, 10, 0));
        bytes.extend(record(1110, 10 | END_BIT, 0));
        bytes.extend(record(1300, 1 | END_BIT, 0));
        bytes.extend(record(1500, 1, 0));
        bytes.extend(record(1600, 20, 7));
        bytes.extend(record(2000, 10, 0));
        bytes.extend(record(2000, 10 | END_BIT, 0));
        trace.received(Some(0), &bytes);
        // 300 + 500 of 1000 ticks.
        assert_eq!(trace.load(Some(0), 1), Some(0.8));
        assert_eq!(trace.load(Some(0), 10), Some(0.01));
        assert_eq!(trace.load(Some(0), 20), None);
        let task = trace.timing_of("task").unwrap();
        assert_eq!(
            (task.core, task.load, task.last_span, task.runs),
            (Some(0), Some(0.8), Some(300), 3)
        );
        assert_eq!(trace.timing_of("dispatch").unwrap().last_span, None);
        assert!(trace.timing_of("TaskZ").is_none());
        let csv = trace.to_csv();
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines[0], "time_ns,core,id,name,kind,arg,span_ns");
        // 100 MHz: a tick is 10 ns.
        assert_eq!(lines[2], "9000,0,1,task,end,0,9000");
        assert_eq!(lines[3], "10000,0,32767,(dropped),dropped,5,");
        assert_eq!(lines[9], "16000,0,20,dispatch,event,7,");
        assert_eq!(lines.len(), 1 + 11);
    }

    #[test]
    fn times_as_text() {
        assert_eq!(format_ns(750), "750 ns");
        assert_eq!(format_ns(2_750), "2.75 µs");
        assert_eq!(format_ns(400_000_000), "400.00 ms");
        assert_eq!(format_ns(1_200_000_000), "1.200 s");
    }
}
