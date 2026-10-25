//! The Multi Channel Sequencers (MCS) of the GTM, as the debugger shows them: where their
//! registers and memory are, what a channel's registers say, the MCS instructions as text, and
//! the GTM view of the standalone debugger (B).
//!
//! The figures are those of the TC27x (TC27x User's Manual, chapter 25, GTM V1.9): four MCS
//! instances of eight channels each; channel `x` of MCS `i` has its registers at
//! `GTM + 0x30000 + i * 0x1000 + x * 0x80` (R0–R7, then `CTRL` at `+0x20`, `PC` at `+0x40`)
//! and MCS `i` its memory at `GTM + 0x38000 + i * 0x8000`. The registers are read 32 bits at a
//! time: a read across a hole between them fails, so a channel is read as two ranges (R0–R7
//! with CTRL, and PC). The memory layout (MCFG_CTRL) moves pages between neighbouring MCS
//! instances; [`memory_size`] follows the manual's table for `MCS_RAM1_EN_ADDR_MSB = 0`.
//!
//! The GTM is only read here. Halting it with the cores (`GTM_OCS.SUS`) and switching channels
//! on and off are writes, left to a later step.

use crate::memory::decode_base64;
use crate::protocol::{MemoryRange, RangeData};

/// The GTM of the TC27x.
pub const TC27X_GTM_BASE: u32 = 0xF010_0000;
/// MCS instances of the TC27x GTM.
pub const MCS_COUNT: usize = 4;
/// Channels of an MCS instance.
pub const CHANNELS: usize = 8;

const MCS_REGISTERS: u32 = 0x3_0000;
const MCS_STRIDE: u32 = 0x1000;
const CHANNEL_STRIDE: u32 = 0x80;
const CHANNEL_CTRL: u32 = 0x20;
const CHANNEL_PC: u32 = 0x40;
const MCS_MEMORY: u32 = 0x3_8000;
const MCS_MEMORY_STRIDE: u32 = 0x8000;
/// `GTM_REV`: the GTM version.
pub const GTM_REV: u32 = 0x0;
/// `GTM_MCFG_CTRL`: the memory layout of every MCS instance.
pub const MCFG_CTRL: u32 = 0xF40;
/// `GTM_CLC`: clock control (`DISS`, bit 1: the GTM is disabled).
pub const GTM_CLC: u32 = 0x9_FD00;
/// `GTM_OCS`: OCDS suspend control (`SUS`, bits 27–24: 2 = soft suspend).
pub const GTM_OCS: u32 = 0x9_FDE8;

/// Where the GTM of a chip is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gtm {
    pub base: u32,
}

impl Gtm {
    /// The GTM of the chip named in an attach file (`TC275`, `TC27x`, …), if it is known.
    pub fn for_chip(chip: &str) -> Option<Self> {
        chip.to_ascii_uppercase()
            .starts_with("TC27")
            .then_some(Self {
                base: TC27X_GTM_BASE,
            })
    }

    /// R0 of channel `channel` of MCS `mcs`; R1–R7 and CTRL follow it.
    pub fn channel_registers(&self, mcs: usize, channel: usize) -> u32 {
        self.base + MCS_REGISTERS + mcs as u32 * MCS_STRIDE + channel as u32 * CHANNEL_STRIDE
    }

    /// The control register (`EN`, the flags, `SP_CNT`) of channel `channel` of MCS `mcs`.
    pub fn channel_ctrl(&self, mcs: usize, channel: usize) -> u32 {
        self.channel_registers(mcs, channel) + CHANNEL_CTRL
    }

    /// The program counter of channel `channel` of MCS `mcs`.
    pub fn channel_pc(&self, mcs: usize, channel: usize) -> u32 {
        self.channel_registers(mcs, channel) + CHANNEL_PC
    }

    /// Where the memory of MCS `mcs` starts: MCS address 0.
    pub fn memory(&self, mcs: usize) -> u32 {
        self.base + MCS_MEMORY + mcs as u32 * MCS_MEMORY_STRIDE
    }

    pub fn register(&self, offset: u32) -> u32 {
        self.base + offset
    }
}

/// The memory layout of an MCS instance (two bits of MCFG_CTRL).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    Default,
    /// Its 2 KB page is swapped with the 4 KB page of the next instance.
    Swap,
    /// It borrows the 4 KB page of the next instance.
    Borrow,
}

impl Layout {
    pub fn of(mcfg_ctrl: u32, mcs: usize) -> Self {
        match (mcfg_ctrl >> (2 * mcs)) & 0b11 {
            0b01 => Self::Swap,
            0b10 => Self::Borrow,
            _ => Self::Default,
        }
    }
}

/// `(MP0, MP1)` of MCS `mcs`: page 0 is MCS addresses `0..MP0`, page 1 `MP0..MP1` (the
/// manual's memory layout parameters, `MCS_RAM1_EN_ADDR_MSB = 0`). The instance before MCS 0
/// is the last one.
pub fn memory_size(mcfg_ctrl: u32, mcs: usize) -> (u32, u32) {
    let before = (mcs + MCS_COUNT - 1) % MCS_COUNT;
    let own = Layout::of(mcfg_ctrl, mcs);
    let previous = Layout::of(mcfg_ctrl, before);
    let mp0 = match previous {
        Layout::Default => 0x1000,
        Layout::Swap => 0x800,
        Layout::Borrow => 0x0,
    };
    let extra = match own {
        Layout::Default => 0x800,
        Layout::Swap => 0x1000,
        Layout::Borrow => 0x1800,
    };
    // Page 1 is the instance's own 2 KB page, unless it gave that away (SWAP) for the next
    // instance's 4 KB page; BORROW adds the next instance's 4 KB page to its own 2 KB.
    (mp0, mp0 + extra)
}

/// What the registers of one MCS channel say.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Channel {
    pub r: [u32; 8],
    pub ctrl: u32,
    pub pc: u32,
}

impl Channel {
    pub fn enabled(&self) -> bool {
        self.ctrl & 1 != 0
    }
    pub fn irq(&self) -> bool {
        self.ctrl & (1 << 1) != 0
    }
    pub fn error(&self) -> bool {
        self.ctrl & (1 << 2) != 0
    }
    /// The status flags `N V Z CY` (bits 7–4 of CTRL), as letters; `-` for a clear flag.
    pub fn flags(&self) -> String {
        [(7, 'N'), (6, 'V'), (5, 'Z'), (4, 'C')]
            .iter()
            .map(|&(bit, c)| if self.ctrl & (1 << bit) != 0 { c } else { '-' })
            .collect()
    }
    /// The stack depth (`SP_CNT`, bits 18–16).
    pub fn stack_depth(&self) -> u32 {
        (self.ctrl >> 16) & 0b111
    }
    pub fn pc(&self) -> u32 {
        self.pc & 0x3FFC
    }
}

/// The internal registers an MCS instruction names (OREG): the field value is the index.
const OREG: [&str; 16] = [
    "R0", "R1", "R2", "R3", "R4", "R5", "R6", "R7", "STA", "ACB", "CTRG", "STRG", "TBU_TS0",
    "TBU_TS1", "TBU_TS2", "MHB",
];

fn oreg(index: u32) -> &'static str {
    OREG[(index & 0xF) as usize]
}

/// The registers an ARU read writes (AREG): R0–R7, the others discard the data (ZERO).
fn areg(index: u32) -> &'static str {
    if index < 8 {
        OREG[index as usize]
    } else {
        "ZERO"
    }
}

fn literal(value: u32) -> String {
    if value < 10 {
        value.to_string()
    } else {
        format!("{value:#x}")
    }
}

/// One MCS instruction as text (TC27x User's Manual 25.13.2). An unknown code is shown as
/// `.word`.
pub fn disassemble(word: u32) -> String {
    let opc0 = word >> 28;
    let a = (word >> 24) & 0xF;
    let b = (word >> 20) & 0xF;
    let opc1 = (word >> 16) & 0xF;
    let lit24 = word & 0xFF_FFFF;
    let lit16 = word & 0xFFFF;
    let address = word & 0x3FFF;
    let unknown = || format!(".word {word:#010x}");
    let literal_op = |name: &str| format!("{name} {}, {}", oreg(a), literal(lit24));
    let two = |name: &str| format!("{name} {}, {}", oreg(a), oreg(b));
    match opc0 {
        0x0 => "NOP".into(),
        0x1 => literal_op("MOVL"),
        0x2 => literal_op("ADDL"),
        0x3 => literal_op("SUBL"),
        0x4 => literal_op("ANDL"),
        0x5 => literal_op("ORL"),
        0x6 => literal_op("XORL"),
        0x7 => literal_op("ATUL"),
        0x8 => literal_op("ATSL"),
        0x9 => literal_op("BTL"),
        0xA => match opc1 {
            0x0 => two("MOV"),
            0x1 => format!("MRD {}, {address:#06x}", oreg(a)),
            0x2 => format!("MWR {}, {address:#06x}", oreg(a)),
            0x3 => two("MRDI"),
            0x4 => two("MWRI"),
            0x5 => format!("POP {}", oreg(a)),
            0x6 => format!("PUSH {}", oreg(a)),
            0x7 => format!("MWR24 {}, {address:#06x}", oreg(a)),
            0x8 => two("MWRI24"),
            _ => unknown(),
        },
        0xB => {
            let aru =
                |name: &str, c: u32| format!("{name} {}, {}, {}", areg(a), areg(b), literal(c));
            let aru_indexed = |name: &str| format!("{name} {}, {}", areg(a), areg(b));
            match opc1 {
                0x0 => aru("ARD", word & 0x1FF),
                0x1 => aru("AWR", word & 0x1F),
                0x2 => aru("NARD", word & 0x1FF),
                0x3 => aru_indexed("NARDI"),
                0x4 => aru_indexed("ARDI"),
                0x5 => aru_indexed("AWRI"),
                _ => unknown(),
            }
        }
        0xC => match opc1 {
            0x0 => two("ADD"),
            0x1 => two("SUB"),
            0x2 => two("NEG"),
            0x3 => two("AND"),
            0x4 => two("OR"),
            0x5 => two("XOR"),
            0x6 => format!("SHR {}, {}", oreg(a), word & 0x1F),
            0x7 => format!("SHL {}, {}", oreg(a), word & 0x1F),
            _ => unknown(),
        },
        0xD => match opc1 {
            0x0 => two("ATU"),
            0x1 => two("ATS"),
            0x2 => two("BT"),
            _ => unknown(),
        },
        0xE => match opc1 {
            0x0 => format!("JMP {address:#06x}"),
            0x1 => format!("JBS {}, {b}, {address:#06x}", oreg(a)),
            0x2 => format!("JBC {}, {b}, {address:#06x}", oreg(a)),
            0x3 => format!("CALL {address:#06x}"),
            0x4 => "RET".into(),
            _ => unknown(),
        },
        0xF => match opc1 {
            0x0 => format!("WURM {}, {}, {lit16:#06x}", oreg(a), oreg(b)),
            _ => unknown(),
        },
        _ => unknown(),
    }
}

/// One line of the code window: an MCS address, its word and the instruction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodeLine {
    pub address: u32,
    pub word: u32,
    pub text: String,
    /// The selected channel's program counter is here.
    pub at_pc: bool,
}

/// One row of the channel table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChannelRow {
    pub channel: usize,
    /// The channel as read, or why it could not be read.
    pub state: Result<Channel, String>,
    /// Which of PC, R0–R7 and CTRL changed since the read before: `[pc, r0, …, r7, ctrl]`.
    pub changed: [bool; 10],
}

/// The words of the code window shown around the program counter.
const CODE_BEFORE: u32 = 8;
const CODE_WORDS: u32 = 24;

/// The GTM view: the channels of one MCS instance read again and again, what changed, and the
/// code around the program counter of the selected channel.
#[derive(Clone, Debug)]
pub struct GtmView {
    gtm: Gtm,
    mcs: usize,
    selected: usize,
    mcfg_ctrl: Option<u32>,
    channels: Vec<Result<Channel, String>>,
    previous: Vec<Option<Channel>>,
    changed: Vec<[bool; 10]>,
    code: Option<(u32, Vec<u32>)>,
    code_error: Option<String>,
    error: Option<String>,
}

impl GtmView {
    pub fn new(gtm: Gtm) -> Self {
        Self {
            gtm,
            mcs: 0,
            selected: 0,
            mcfg_ctrl: None,
            channels: Vec::new(),
            previous: vec![None; CHANNELS],
            changed: vec![[false; 10]; CHANNELS],
            code: None,
            code_error: None,
            error: None,
        }
    }

    pub fn gtm(&self) -> Gtm {
        self.gtm
    }
    pub fn mcs(&self) -> usize {
        self.mcs
    }
    pub fn selected(&self) -> usize {
        self.selected
    }
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
    pub fn mcfg_ctrl(&self) -> Option<u32> {
        self.mcfg_ctrl
    }

    /// Shows MCS `mcs`: what was read of another one is forgotten.
    pub fn show_mcs(&mut self, mcs: usize) {
        if mcs != self.mcs && mcs < MCS_COUNT {
            *self = Self {
                mcs,
                ..Self::new(self.gtm)
            };
        }
    }

    /// Selects the channel whose code is shown.
    pub fn select(&mut self, channel: usize) {
        if channel < CHANNELS && channel != self.selected {
            self.selected = channel;
            self.code = None;
            self.code_error = None;
        }
    }

    /// The memory size `(MP0, MP1)` of the MCS shown, once MCFG_CTRL was read.
    pub fn memory_size(&self) -> Option<(u32, u32)> {
        self.mcfg_ctrl.map(|m| memory_size(m, self.mcs))
    }

    /// The code window to read: around the selected channel's last PC, inside its memory.
    fn code_window(&self) -> Option<(u32, u32)> {
        let pc = self.channels.get(self.selected)?.as_ref().ok()?.pc();
        let (_, end) = self.memory_size()?;
        if end == 0 {
            return None;
        }
        let start = pc
            .saturating_sub(CODE_BEFORE * 4)
            .min(end.saturating_sub(4));
        let words = CODE_WORDS.min((end - start) / 4);
        Some((start, words))
    }

    /// The ranges to read, in the order [`Self::answered`] takes them: MCFG_CTRL, then for each
    /// channel R0–R7 with CTRL and PC, then the code window when there is one.
    pub fn ranges(&self) -> Vec<MemoryRange> {
        let mut ranges = vec![MemoryRange {
            address: self.gtm.register(MCFG_CTRL) as u64,
            count: 4,
        }];
        for channel in 0..CHANNELS {
            ranges.push(MemoryRange {
                address: self.gtm.channel_registers(self.mcs, channel) as u64,
                count: 9 * 4,
            });
            ranges.push(MemoryRange {
                address: self.gtm.channel_pc(self.mcs, channel) as u64,
                count: 4,
            });
        }
        if let Some((start, words)) = self.code_window() {
            ranges.push(MemoryRange {
                address: (self.gtm.memory(self.mcs) + start) as u64,
                count: (words * 4) as usize,
            });
        }
        ranges
    }

    /// The answer to [`Self::ranges`] (asked as `asked`).
    pub fn answered(&mut self, asked: &[MemoryRange], answer: &[RangeData]) {
        self.error = None;
        if answer.len() != asked.len() {
            self.error = Some(format!(
                "asked {} ranges, got {}",
                asked.len(),
                answer.len()
            ));
            return;
        }
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
        match words(&answer[0]) {
            Ok(w) => self.mcfg_ctrl = w.first().copied(),
            Err(error) => self.error = Some(format!("MCFG_CTRL: {error}")),
        }
        let mut channels = Vec::with_capacity(CHANNELS);
        for channel in 0..CHANNELS {
            let regs = words(&answer[1 + 2 * channel]);
            let pc = words(&answer[2 + 2 * channel]);
            channels.push(match (regs, pc) {
                (Ok(regs), Ok(pc)) if regs.len() == 9 && pc.len() == 1 => {
                    let mut r = [0; 8];
                    r.copy_from_slice(&regs[..8]);
                    Ok(Channel {
                        r,
                        ctrl: regs[8],
                        pc: pc[0],
                    })
                }
                (Err(error), _) | (_, Err(error)) => Err(error),
                _ => Err("short read".into()),
            });
        }
        // Mark what changed since the read before; keep the marks while nothing changes.
        for (channel, read) in channels.iter().enumerate() {
            let Ok(now) = read else { continue };
            if let Some(before) = self.previous[channel]
                && before != *now
            {
                let mut marks = [false; 10];
                marks[0] = before.pc != now.pc;
                for i in 0..8 {
                    marks[1 + i] = before.r[i] != now.r[i];
                }
                marks[9] = before.ctrl != now.ctrl;
                self.changed[channel] = marks;
            }
            self.previous[channel] = Some(*now);
        }
        self.channels = channels;
        if let Some(code) = answer.get(1 + 2 * CHANNELS) {
            let start = asked[1 + 2 * CHANNELS].address as u32 - self.gtm.memory(self.mcs);
            match words(code) {
                Ok(w) => {
                    self.code = Some((start, w));
                    self.code_error = None;
                }
                Err(error) => self.code_error = Some(error),
            }
        }
    }

    /// The request failed as a whole.
    pub fn failed(&mut self, error: &str) {
        self.error = Some(error.to_string());
    }

    pub fn rows(&self) -> Vec<ChannelRow> {
        self.channels
            .iter()
            .enumerate()
            .map(|(channel, state)| ChannelRow {
                channel,
                state: state.clone(),
                changed: self.changed[channel],
            })
            .collect()
    }

    /// The code around the selected channel's program counter, or why it could not be read.
    pub fn code(&self) -> Result<Vec<CodeLine>, String> {
        if let Some(error) = &self.code_error {
            return Err(error.clone());
        }
        let pc = self
            .channels
            .get(self.selected)
            .and_then(|c| c.as_ref().ok())
            .map(Channel::pc);
        let Some((start, words)) = &self.code else {
            return Ok(Vec::new());
        };
        Ok(words
            .iter()
            .enumerate()
            .map(|(i, &word)| {
                let address = start + 4 * i as u32;
                CodeLine {
                    address,
                    word,
                    text: disassemble(word),
                    at_pc: Some(address) == pc,
                }
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::encode_base64;

    #[test]
    fn instructions_read_as_the_manual_writes_them() {
        // MOVL R0, 0 / ADDL R0, 1 / MWR R0, 0x400 / JMP 0x4
        assert_eq!(disassemble(0x1000_0000), "MOVL R0, 0");
        assert_eq!(disassemble(0x2000_0001), "ADDL R0, 1");
        assert_eq!(disassemble(0xA002_0400), "MWR R0, 0x0400");
        assert_eq!(disassemble(0xE000_0004), "JMP 0x0004");
        assert_eq!(disassemble(0x3300_0123), "SUBL R3, 0x123");
        assert_eq!(disassemble(0xC120_0000), "ADD R1, R2");
        assert_eq!(disassemble(0xCB06_0003), "SHR STRG, 3");
        assert_eq!(disassemble(0xE851_0010), "JBS STA, 5, 0x0010");
        assert_eq!(disassemble(0xE003_0100), "CALL 0x0100");
        assert_eq!(disassemble(0xE004_0000), "RET");
        assert_eq!(disassemble(0xF0B0_00FF), "WURM R0, STRG, 0x00ff");
        assert_eq!(disassemble(0xB080_0005), "ARD R0, ZERO, 5");
        assert_eq!(disassemble(0x0123_4567), "NOP");
        assert_eq!(disassemble(0xA00F_0000), ".word 0xa00f0000");
    }

    #[test]
    fn the_memory_layout_follows_the_manual_table() {
        // All DEFAULT: 4 KB + 2 KB each.
        for mcs in 0..MCS_COUNT {
            assert_eq!(memory_size(0, mcs), (0x1000, 0x1800));
        }
        // This ShieldBuddy: MCS3 BORROW (0x80). MCS3 takes MCS0's 4 KB page.
        assert_eq!(memory_size(0x80, 3), (0x1000, 0x2800));
        assert_eq!(memory_size(0x80, 0), (0x0, 0x800));
        assert_eq!(memory_size(0x80, 1), (0x1000, 0x1800));
        // SWAP of MCS1: it gets MCS2's 4 KB page for its 2 KB one.
        assert_eq!(memory_size(0b0100, 1), (0x1000, 0x2000));
        assert_eq!(memory_size(0b0100, 2), (0x800, 0x1000));
    }

    #[test]
    fn the_tc27x_addresses() {
        let gtm = Gtm::for_chip("TC275").unwrap();
        assert_eq!(gtm.channel_registers(0, 0), 0xF013_0000);
        assert_eq!(gtm.channel_registers(1, 2), 0xF013_1100);
        assert_eq!(gtm.channel_pc(3, 7), 0xF013_33C0);
        assert_eq!(gtm.memory(1), 0xF014_0000);
        assert_eq!(gtm.register(GTM_OCS), 0xF019_FDE8);
        assert!(Gtm::for_chip("TC36x").is_none());
    }

    fn data(words: &[u32]) -> RangeData {
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        RangeData {
            address: String::new(),
            data: Some(encode_base64(&bytes)),
            error: None,
        }
    }

    fn answer(
        view: &GtmView,
        pc0: u32,
        r0: u32,
        code: &[u32],
    ) -> (Vec<MemoryRange>, Vec<RangeData>) {
        let asked = view.ranges();
        let mut answer = vec![data(&[0])];
        for channel in 0..CHANNELS {
            if channel == 0 {
                answer.push(data(&[r0, 0, 0, 0, 0, 0, 0, 0, 0x21]));
                answer.push(data(&[pc0]));
            } else {
                answer.push(RangeData {
                    address: String::new(),
                    data: None,
                    error: Some("0x8d".into()),
                });
                answer.push(data(&[4 * channel as u32]));
            }
        }
        if asked.len() > answer.len() {
            answer.push(data(code));
        }
        (asked, answer)
    }

    #[test]
    fn channels_changes_and_code_around_the_pc() {
        let mut view = GtmView::new(Gtm::for_chip("TC275").unwrap());
        view.show_mcs(1);
        // First read: no code window yet (the PC is not known).
        assert_eq!(view.ranges().len(), 1 + 2 * CHANNELS);
        let (asked, reply) = answer(&view, 0x8, 5, &[]);
        view.answered(&asked, &reply);
        let rows = view.rows();
        let ch0 = rows[0].state.as_ref().unwrap();
        assert!(ch0.enabled());
        assert_eq!(ch0.flags(), "--Z-");
        assert_eq!(ch0.pc(), 0x8);
        assert_eq!(rows[1].state, Err("0x8d".into()));
        assert_eq!(rows[0].changed, [false; 10]);

        // Second read: the code window around PC 0x8, from MCS address 0.
        let asked = view.ranges();
        assert_eq!(asked.len(), 2 + 2 * CHANNELS);
        let window = asked.last().unwrap();
        assert_eq!(window.address, 0xF014_0000);
        assert_eq!(window.count, 24 * 4);
        let code = [0x1000_0000, 0x2000_0001, 0xA002_0400, 0xE000_0004];
        let (asked, reply) = answer(&view, 0xC, 6, &code);
        view.answered(&asked, &reply);
        let rows = view.rows();
        assert!(rows[0].changed[0] && rows[0].changed[1] && !rows[0].changed[2]);
        let lines = view.code().unwrap();
        assert_eq!(lines[3].text, "JMP 0x0004");
        assert!(lines[3].at_pc);
        assert!(!lines[2].at_pc);

        // The same values again: the marks stay.
        let (asked, reply) = answer(&view, 0xC, 6, &code);
        view.answered(&asked, &reply);
        assert!(view.rows()[0].changed[0]);
    }
}
