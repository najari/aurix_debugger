//! The caches of the TriCore cores: what each core has and whether it is on, which segments it
//! caches, and the hits and misses its performance counters count. Everything is read from the
//! cores' CSFRs over the bus, while the cores run too (TC27x User's Manual V2.2, 5.8, 5.10, 5.11):
//!
//! - `PCON2`/`DCON2`: the sizes of the program cache and scratch-pad, of the data cache and
//!   scratch-pad, in KB;
//! - `PCON0.PCBYP`/`DCON0.DCBYP`: the program cache and the data cache (TC1.6P) or data read
//!   buffer (TC1.6E) bypassed;
//! - `PMA0`/`PMA1`: the segments cacheable for data and for code;
//! - `CCTRL`, `CCNT`, `ICNT`, `M1CNT`–`M3CNT`: the performance counters.
//!
//! The lines in a cache (its tags) are not shown: the tag memories can be read only with the
//! cache switched off and mapped into the address space (`MTU_MEMMAP`), which changes how the
//! program runs, and their format is not in the manual.
//!
//! A measurement sets the counters' sources (`CCTRL.M1`–`M3`) for the program cache, then for
//! the data side, for the time between two reads each, and puts `CCTRL` back: the only writes.

use crate::memory::decode_base64;
use crate::protocol::{MemoryRange, RangeData};

/// The CSFRs of a core, from its CSFR base.
pub mod csfr {
    pub const PMA0: u32 = 0x8100;
    pub const PMA1: u32 = 0x8104;
    pub const DCON2: u32 = 0x9000;
    pub const DCON0: u32 = 0x9040;
    pub const PCON2: u32 = 0x9208;
    pub const PCON0: u32 = 0x920C;
    pub const CCTRL: u32 = 0xFC00;
}

/// `PCON0.PCBYP`, `DCON0.DCBYP`.
const BYPASS: u32 = 1 << 1;
/// `CCTRL.CE`: the counters count.
const CCTRL_CE: u32 = 1 << 1;
/// `CCTRL.M1`–`M3`: the source of each multi-count register (bits 4:2, 7:5, 10:8).
const CCTRL_SOURCES: u32 = 0x7FC;

/// The TriCore of a core: the TC1.6E has a program cache and a data read buffer, the TC1.6P
/// a program cache and a write-back data cache.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoreKind {
    E,
    P,
}

impl CoreKind {
    pub fn name(self) -> &'static str {
        match self {
            CoreKind::E => "TC1.6E",
            CoreKind::P => "TC1.6P",
        }
    }

    /// What the data side has.
    pub fn data_name(self) -> &'static str {
        match self {
            CoreKind::E => "DRB",
            CoreKind::P => "DCACHE",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CacheCore {
    /// The core's number (thread of the session).
    pub core: usize,
    pub kind: CoreKind,
    /// Its CSFR base.
    pub csfr: u32,
}

impl CacheCore {
    pub fn register(&self, offset: u32) -> u32 {
        self.csfr + offset
    }
}

/// The cores of a chip whose caches are known.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Caches {
    pub cores: Vec<CacheCore>,
}

impl Caches {
    /// The TC27x: CPU0 a TC1.6E, CPU1 and CPU2 TC1.6P (CSFRs at `0xF881_0000 + 0x2_0000 * n`).
    pub fn for_chip(chip: &str) -> Option<Self> {
        chip.to_ascii_uppercase().starts_with("TC27").then(|| Self {
            cores: (0..3)
                .map(|core| CacheCore {
                    core,
                    kind: if core == 0 { CoreKind::E } else { CoreKind::P },
                    csfr: 0xF881_0000 + 0x2_0000 * core as u32,
                })
                .collect(),
        })
    }
}

/// The performance counters of a core at one read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counters {
    pub cctrl: u32,
    /// Clock cycles.
    pub ccnt: u32,
    /// Instructions.
    pub icnt: u32,
    pub m: [u32; 3],
}

impl Counters {
    /// The counts from `before` to `self` (31 bits each; bit 31 is the overflow flag).
    fn since(&self, before: &Counters) -> [u32; 5] {
        let delta = |a: u32, b: u32| a.wrapping_sub(b) & 0x7FFF_FFFF;
        [
            delta(self.ccnt, before.ccnt),
            delta(self.icnt, before.icnt),
            delta(self.m[0], before.m[0]),
            delta(self.m[1], before.m[1]),
            delta(self.m[2], before.m[2]),
        ]
    }
}

/// What one core's registers say.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CoreCaches {
    pub kind: CoreKind,
    pub pma0: u32,
    pub pma1: u32,
    pub pcon0: u32,
    pub pcon2: u32,
    pub dcon0: u32,
    pub dcon2: u32,
    pub counters: Counters,
}

impl CoreCaches {
    pub fn pcache_kb(&self) -> u32 {
        self.pcon2 & 0xFFFF
    }

    pub fn pspr_kb(&self) -> u32 {
        self.pcon2 >> 16
    }

    pub fn dcache_kb(&self) -> u32 {
        self.dcon2 & 0xFFFF
    }

    pub fn dspr_kb(&self) -> u32 {
        self.dcon2 >> 16
    }

    /// The program cache caches (not bypassed).
    pub fn pcache_on(&self) -> bool {
        self.pcache_kb() > 0 && self.pcon0 & BYPASS == 0
    }

    /// The data cache (TC1.6P) or data read buffer (TC1.6E) caches.
    pub fn data_on(&self) -> bool {
        let present = match self.kind {
            CoreKind::P => self.dcache_kb() > 0,
            CoreKind::E => true,
        };
        present && self.dcon0 & BYPASS == 0
    }

    /// Whether the core caches data accesses to `segment` (0–15). Segments A, E and F, D and the
    /// core's own segment (7 - core) never are (5.7, PMA).
    pub fn data_cacheable(&self, core: usize, segment: u32) -> bool {
        !(matches!(segment, 0xA | 0xD | 0xE | 0xF) || segment as usize + core == 7)
            && self.pma0 >> segment & 1 == 1
    }

    /// Whether the core caches code fetches from `segment`: never A, C, E, F and its own.
    pub fn code_cacheable(&self, core: usize, segment: u32) -> bool {
        !(matches!(segment, 0xA | 0xC | 0xE | 0xF) || segment as usize + core == 7)
            && self.pma1 >> segment & 1 == 1
    }

    /// The segments cacheable by `test`, as `8 9`.
    fn segments(&self, test: impl Fn(u32) -> bool) -> String {
        let list: Vec<String> = (0..16)
            .filter(|s| test(*s))
            .map(|s| format!("{s:X}"))
            .collect();
        if list.is_empty() {
            "none".into()
        } else {
            list.join(" ")
        }
    }
}

/// One row of the cache table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CacheRow {
    pub core: usize,
    pub kind: CoreKind,
    /// The program side: `PCACHE 16 KB on`, and the scratch-pad.
    pub program: String,
    /// The data side: `DCACHE 8 KB bypassed`, `DRB on`.
    pub data: String,
    /// The segments cacheable for code, for data.
    pub code_segments: String,
    pub data_segments: String,
    /// The last measurement: program cache, data side.
    pub measured: Option<(String, String)>,
}

/// Where a measurement is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    /// The program sources were written; the next read is the start.
    ProgramStart,
    ProgramEnd,
    DataStart,
    DataEnd,
}

/// A write of one CSFR word (`CCTRL`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CounterWrite {
    pub core: usize,
    pub address: u32,
    pub value: u32,
}

impl CounterWrite {
    pub fn arguments(&self) -> dap_types::WriteMemoryArguments {
        dap_types::WriteMemoryArguments {
            memory_reference: format!("{:#010x}", self.address),
            offset: None,
            allow_partial: Some(false),
            data: crate::memory::encode_base64(&self.value.to_le_bytes()),
        }
    }
}

/// The cache view: the cores' registers read again and again, and a measurement.
#[derive(Clone, Debug)]
pub struct CacheView {
    caches: Caches,
    states: Vec<Result<CoreCaches, String>>,
    error: Option<String>,
    /// Bumped by each write: a read sent before it does not count for a measurement.
    generation: u64,
    phase: Option<Phase>,
    /// `CCTRL` of each core before the measurement.
    saved: Vec<u32>,
    start: Vec<Counters>,
    program: Vec<[u32; 5]>,
    measured: Vec<Option<(String, String)>>,
    writes: Vec<CounterWrite>,
}

/// Sources of `CCTRL.M1`–`M3` (Table 5-17, 5-18): 1 the program cache (hit, miss, multi-issue
/// on a TC1.6P), 2 the data side (hit, clean miss, dirty miss; DRB hit, miss on a TC1.6E).
const PROGRAM: u32 = 1;
const DATA: u32 = 2;

fn cctrl_with(saved: u32, source: u32) -> u32 {
    (saved & !CCTRL_SOURCES) | CCTRL_CE | source << 2 | source << 5 | source << 8
}

fn percent(hits: u32, misses: u32) -> String {
    let total = u64::from(hits) + u64::from(misses);
    if total == 0 {
        "no accesses".into()
    } else {
        format!(
            "{:.2}% hits ({hits} hits, {misses} misses)",
            hits as f64 * 100.0 / total as f64
        )
    }
}

impl CacheView {
    pub fn new(caches: Caches) -> Self {
        let n = caches.cores.len();
        Self {
            caches,
            states: Vec::new(),
            error: None,
            generation: 0,
            phase: None,
            saved: Vec::new(),
            start: Vec::new(),
            program: Vec::new(),
            measured: vec![None; n],
            writes: Vec::new(),
        }
    }

    pub fn caches(&self) -> &Caches {
        &self.caches
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// The ranges to read, five per core, and the generation to hand back with the answer.
    pub fn ranges(&self) -> (Vec<MemoryRange>, u64) {
        let mut ranges = Vec::new();
        for core in &self.caches.cores {
            for (offset, count) in [
                (csfr::PMA0, 8),
                (csfr::DCON2, 4),
                (csfr::DCON0, 4),
                (csfr::PCON2, 8),
                (csfr::CCTRL, 24),
            ] {
                ranges.push(MemoryRange {
                    address: core.register(offset) as u64,
                    count,
                });
            }
        }
        (ranges, self.generation)
    }

    /// The request as a whole failed.
    pub fn failed(&mut self, error: &str) {
        self.error = Some(error.to_string());
    }

    /// The answer to [`Self::ranges`] of `generation`.
    pub fn answered(&mut self, generation: u64, answer: &[RangeData]) {
        let n = self.caches.cores.len();
        if answer.len() != 5 * n {
            self.error = Some(format!("asked {} ranges, got {}", 5 * n, answer.len()));
            return;
        }
        self.error = None;
        let words = |data: &RangeData| -> Result<Vec<u32>, String> {
            if let Some(error) = &data.error {
                return Err(error.clone());
            }
            let bytes = decode_base64(data.data.as_deref().unwrap_or(""))?;
            Ok(bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|w| u32::from_le_bytes(*w))
                .collect())
        };
        self.states = self
            .caches
            .cores
            .iter()
            .enumerate()
            .map(|(i, core)| {
                let part = &answer[5 * i..5 * i + 5];
                let read: Result<Vec<Vec<u32>>, String> = part.iter().map(words).collect();
                match read?.as_slice() {
                    [pma, dcon2, dcon0, pcon, counters]
                        if pma.len() == 2
                            && dcon2.len() == 1
                            && dcon0.len() == 1
                            && pcon.len() == 2
                            && counters.len() == 6 =>
                    {
                        Ok(CoreCaches {
                            kind: core.kind,
                            pma0: pma[0],
                            pma1: pma[1],
                            dcon2: dcon2[0],
                            dcon0: dcon0[0],
                            pcon2: pcon[0],
                            pcon0: pcon[1],
                            counters: Counters {
                                cctrl: counters[0],
                                ccnt: counters[1],
                                icnt: counters[2],
                                m: [counters[3], counters[4], counters[5]],
                            },
                        })
                    }
                    _ => Err("short read".into()),
                }
            })
            .collect();
        if generation == self.generation {
            self.advance();
        }
    }

    /// Whether a measurement runs.
    pub fn measuring(&self) -> bool {
        self.phase.is_some()
    }

    /// Starts a measurement: the program cache sources first. False when the registers were
    /// not read yet, or one runs.
    pub fn measure(&mut self) -> bool {
        if self.phase.is_some() {
            return false;
        }
        let Some(saved) = self
            .states
            .iter()
            .map(|s| s.as_ref().ok().map(|s| s.counters.cctrl))
            .collect::<Option<Vec<u32>>>()
        else {
            return false;
        };
        if saved.len() != self.caches.cores.len() {
            return false;
        }
        self.saved = saved;
        self.write_sources(Some(PROGRAM));
        self.phase = Some(Phase::ProgramStart);
        true
    }

    /// The `CCTRL` writes to send before the next read.
    pub fn take_writes(&mut self) -> Vec<CounterWrite> {
        std::mem::take(&mut self.writes)
    }

    /// Sets every core's sources (`None`: back as before).
    fn write_sources(&mut self, source: Option<u32>) {
        self.writes = self
            .caches
            .cores
            .iter()
            .zip(&self.saved)
            .map(|(core, saved)| CounterWrite {
                core: core.core,
                address: core.register(csfr::CCTRL),
                value: source.map_or(*saved, |source| cctrl_with(*saved, source)),
            })
            .collect();
        self.generation += 1;
    }

    fn counters(&self) -> Option<Vec<Counters>> {
        self.states
            .iter()
            .map(|s| s.as_ref().ok().map(|s| s.counters))
            .collect()
    }

    /// The next step after a read of the current generation.
    fn advance(&mut self) {
        let Some(phase) = self.phase else { return };
        let Some(now) = self.counters() else {
            // A core could not be read: give up, and put the sources back.
            self.write_sources(None);
            self.phase = None;
            self.error = Some("measurement stopped: a core could not be read".into());
            return;
        };
        match phase {
            Phase::ProgramStart | Phase::DataStart => {
                self.start = now;
                self.phase = Some(if phase == Phase::ProgramStart {
                    Phase::ProgramEnd
                } else {
                    Phase::DataEnd
                });
            }
            Phase::ProgramEnd => {
                self.program = now
                    .iter()
                    .zip(&self.start)
                    .map(|(now, start)| now.since(start))
                    .collect();
                self.write_sources(Some(DATA));
                self.phase = Some(Phase::DataStart);
            }
            Phase::DataEnd => {
                let data: Vec<[u32; 5]> = now
                    .iter()
                    .zip(&self.start)
                    .map(|(now, start)| now.since(start))
                    .collect();
                self.measured = self
                    .caches
                    .cores
                    .iter()
                    .zip(self.program.iter().zip(&data))
                    .map(|(core, (p, d))| {
                        let program = format!(
                            "PCACHE {} in {} cycles, {} instructions",
                            percent(p[2], p[3]),
                            p[0],
                            p[1]
                        );
                        let data = match core.kind {
                            CoreKind::P => format!(
                                "DCACHE {}, {} dirty misses",
                                percent(d[2], d[3] + d[4]),
                                d[4]
                            ),
                            CoreKind::E => format!("DRB {}", percent(d[2], d[3])),
                        };
                        Some((program, data))
                    })
                    .collect();
                self.write_sources(None);
                self.phase = None;
            }
        }
    }

    pub fn rows(&self) -> Vec<Result<CacheRow, String>> {
        self.caches
            .cores
            .iter()
            .zip(&self.states)
            .enumerate()
            .map(|(i, (core, state))| {
                let state = state.as_ref().map_err(Clone::clone)?;
                let on = |on: bool| if on { "on" } else { "bypassed" };
                Ok(CacheRow {
                    core: core.core,
                    kind: core.kind,
                    program: format!(
                        "PCACHE {} KB {}, PSPR {} KB",
                        state.pcache_kb(),
                        on(state.pcache_on()),
                        state.pspr_kb()
                    ),
                    data: match core.kind {
                        CoreKind::P => format!(
                            "DCACHE {} KB {}, DSPR {} KB",
                            state.dcache_kb(),
                            on(state.data_on()),
                            state.dspr_kb()
                        ),
                        CoreKind::E => format!(
                            "DRB 128 B {}, DSPR {} KB",
                            on(state.data_on()),
                            state.dspr_kb()
                        ),
                    },
                    code_segments: state.segments(|s| state.code_cacheable(core.core, s)),
                    data_segments: state.segments(|s| state.data_cacheable(core.core, s)),
                    measured: self.measured.get(i).cloned().flatten(),
                })
            })
            .collect()
    }

    /// What the caches mean for memory read and written over the bus at `address` (the
    /// debugger reads and writes the memory, not a core's cache): `None` when no core caches
    /// data there now.
    pub fn memory_note(&self, address: u64) -> Option<String> {
        let segment = (address >> 28) as u32 & 0xF;
        let mut write_back = Vec::new();
        let mut read_only = Vec::new();
        for (core, state) in self.caches.cores.iter().zip(&self.states) {
            let Ok(state) = state else { continue };
            if state.data_on() && state.data_cacheable(core.core, segment) {
                match core.kind {
                    CoreKind::P => write_back.push(format!("CPU{}", core.core)),
                    CoreKind::E => read_only.push(format!("CPU{}", core.core)),
                }
            }
        }
        let mut parts = Vec::new();
        if !write_back.is_empty() {
            parts.push(format!(
                "{} cache this segment in a write-back DCACHE: a value shown may be older than the core's",
                write_back.join(", ")
            ));
        }
        if !read_only.is_empty() || !write_back.is_empty() {
            let all: Vec<String> = write_back.iter().chain(&read_only).cloned().collect();
            parts.push(format!(
                "a write here is not seen by a line {} holds",
                all.join(", ")
            ));
        }
        (!parts.is_empty()).then(|| format!("Segment {segment:X}: {}", parts.join("; ")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::encode_base64;

    fn data(words: &[u32]) -> RangeData {
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        RangeData {
            address: String::new(),
            data: Some(encode_base64(&bytes)),
            error: None,
        }
    }

    /// The board's registers (flash demo): PCACHE on, DCACHE bypassed, segments 8 and 9.
    fn board(counters: [[u32; 6]; 3], dcon0: u32) -> Vec<RangeData> {
        let pcon2 = [0x0018_0008, 0x0020_0010, 0x0020_0010];
        let dcon2 = [0x0070_0000, 0x0078_0008, 0x0078_0008];
        (0..3)
            .flat_map(|i| {
                [
                    data(&[0x300, 0x300]),
                    data(&[dcon2[i]]),
                    data(&[dcon0]),
                    data(&[pcon2[i], 0]),
                    data(&counters[i]),
                ]
            })
            .collect()
    }

    fn view() -> CacheView {
        CacheView::new(Caches::for_chip("TC275").unwrap())
    }

    #[test]
    fn the_board_as_read() {
        let mut view = view();
        let (ranges, generation) = view.ranges();
        assert_eq!(ranges.len(), 15);
        assert_eq!(ranges[4].address, 0xF881_FC00);
        assert_eq!(ranges[5].address, 0xF883_8100);
        view.answered(generation, &board([[0; 6]; 3], 2));
        let rows: Vec<CacheRow> = view.rows().into_iter().map(Result::unwrap).collect();
        assert_eq!(rows[0].program, "PCACHE 8 KB on, PSPR 24 KB");
        assert_eq!(rows[0].data, "DRB 128 B bypassed, DSPR 112 KB");
        assert_eq!(rows[1].data, "DCACHE 8 KB bypassed, DSPR 120 KB");
        assert_eq!(rows[2].code_segments, "8 9");
        assert_eq!(rows[2].data_segments, "8 9");
        // Bypassed: nothing to say about the LMU.
        assert_eq!(view.memory_note(0x9000_0000), None);
    }

    #[test]
    fn segments_never_cacheable() {
        let state = CoreCaches {
            kind: CoreKind::P,
            pma0: 0xFFFF,
            pma1: 0xFFFF,
            pcon0: 0,
            pcon2: 0,
            dcon0: 0,
            dcon2: 0,
            counters: Counters::default(),
        };
        // CPU1: its own segment is 6.
        let data: Vec<u32> = (0..16).filter(|s| state.data_cacheable(1, *s)).collect();
        assert_eq!(data, [0, 1, 2, 3, 4, 5, 7, 8, 9, 0xB, 0xC]);
        let code: Vec<u32> = (0..16).filter(|s| state.code_cacheable(1, *s)).collect();
        assert_eq!(code, [0, 1, 2, 3, 4, 5, 7, 8, 9, 0xB, 0xD]);
    }

    #[test]
    fn notes_for_cached_memory() {
        let mut view = view();
        let (_, generation) = view.ranges();
        view.answered(generation, &board([[0; 6]; 3], 0));
        let note = view.memory_note(0x9000_0000).unwrap();
        assert!(note.contains("CPU1, CPU2 cache this segment"), "{note}");
        assert!(note.contains("line CPU1, CPU2, CPU0 holds"), "{note}");
        // The local DSPR of CPU0 is no core's cacheable segment... and segment 7 is CPU0's own.
        assert_eq!(view.memory_note(0x7000_0000), None);
        assert_eq!(view.memory_note(0xB000_0000), None);
    }

    #[test]
    fn a_measurement_writes_then_puts_back() {
        let mut view = view();
        let (_, g0) = view.ranges();
        let count = |base: u32| [[0, base, base, base, 0, 0]; 3];
        view.answered(g0, &board(count(0), 2));
        assert!(view.measure());
        assert!(!view.measure());
        let writes = view.take_writes();
        assert_eq!(writes.len(), 3);
        assert_eq!(writes[1].address, 0xF883_FC00);
        assert_eq!(writes[1].value, 0x126);
        // A read sent before the writes does not count.
        view.answered(g0, &board(count(5), 2));
        assert!(view.take_writes().is_empty());
        let (_, g1) = view.ranges();
        view.answered(g1, &board([[0x126, 100, 50, 1000, 0, 0]; 3], 2));
        view.answered(g1, &board([[0x126, 300, 150, 1900, 100, 0]; 3], 2));
        let writes = view.take_writes();
        assert_eq!(writes[0].value, 0x24A);
        let (_, g2) = view.ranges();
        view.answered(g2, &board([[0x24A, 400, 200, 0, 0, 0]; 3], 2));
        view.answered(g2, &board([[0x24A, 500, 300, 30, 10, 0]; 3], 2));
        let writes = view.take_writes();
        assert_eq!(writes[2].value, 0);
        assert!(!view.measuring());
        let rows: Vec<CacheRow> = view.rows().into_iter().map(Result::unwrap).collect();
        let (program, data) = rows[1].measured.clone().unwrap();
        assert_eq!(
            program,
            "PCACHE 90.00% hits (900 hits, 100 misses) in 200 cycles, 100 instructions"
        );
        assert_eq!(
            data,
            "DCACHE 75.00% hits (30 hits, 10 misses), 0 dirty misses"
        );
        assert_eq!(
            rows[0].measured.clone().unwrap().1,
            "DRB 75.00% hits (30 hits, 10 misses)"
        );
        assert_eq!(writes[1].arguments().memory_reference, "0xf883fc00");
    }
}
