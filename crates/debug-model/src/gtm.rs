//! The Multi Channel Sequencers (MCS) of the GTM, as the debugger shows them: where their
//! registers and memory are, what a channel's registers say, the MCS instructions as text, and
//! the GTM view of the standalone debugger (B).
//!
//! The TC27x (TC27x User's Manual, chapter 25, GTM V1.9) has four MCS instances of eight
//! channels each; channel `x` of MCS `i` has its registers at
//! `GTM + 0x30000 + i * 0x1000 + x * 0x80` (R0–R7, then `CTRL` at `+0x20`, `PC` at `+0x40`)
//! and MCS `i` its memory at `GTM + 0x38000 + i * 0x8000`, a 4 KB page and a 2 KB page. The
//! TC36x (TC36x User's Manual appendix, chapter 26, GTM 3) has three, its registers at
//! `GTM + 0xF0000 + i * 0x1000` (from 0xF01F_0000, the channels laid out as on the TC27x), its
//! memory where the TC27x has it but pages of 8 KB and 4 KB ([`Gtm`]); a word of that memory
//! never written fails its ECC check when an MCS reads ahead of the instruction it runs, which
//! stops the channel. The registers are read 32 bits at a
//! time: a read across a hole between them fails, so a channel is read as two ranges (R0–R7
//! with CTRL, and PC). The memory layout (MCFG_CTRL, DEFAULT, SWAP or BORROW on both) moves
//! pages between neighbouring MCS instances; [`Gtm::memory_size`] follows the TC27x manual's
//! table for `MCS_RAM1_EN_ADDR_MSB = 0`, with each chip's page sizes.
//!
//! The GTM is only read here. Halting it with the cores (`GTM_OCS.SUS`) and switching channels
//! on and off are writes, left to a later step.

use crate::memory::decode_base64;
use crate::protocol::{MemoryRange, RangeData};

/// The GTM of the TC27x and of the TC3xx.
pub const TC27X_GTM_BASE: u32 = 0xF010_0000;
/// MCS instances of a GTM at most (the TC27x's four; the TC36x has three).
pub const MCS_COUNT: usize = 4;
/// Channels of an MCS instance.
pub const CHANNELS: usize = 8;

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
/// `GTM_OCS` of the TC27x: OCDS suspend control (`SUS`, bits 27–24: 2 = soft suspend). The
/// TC36x has it at `0x9_FD38`.
pub const GTM_OCS: u32 = 0x9_FDE8;

/// Where the GTM of a chip is and how its MCS instances are laid out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gtm {
    pub base: u32,
    /// The registers of MCS 0, from `base`.
    pub mcs_registers: u32,
    /// The MCS instances.
    pub mcs_count: usize,
    /// The sizes of an instance's two memory pages.
    pub page0: u32,
    pub page1: u32,
    /// The bits of `MCSi_CHx_PC` that are the program counter.
    pub pc_mask: u32,
}

impl Gtm {
    pub const TC27X: Self = Self {
        base: TC27X_GTM_BASE,
        mcs_registers: 0x3_0000,
        mcs_count: 4,
        page0: 0x1000,
        page1: 0x800,
        pc_mask: 0x3FFC,
    };
    pub const TC36X: Self = Self {
        base: TC27X_GTM_BASE,
        mcs_registers: 0xF_0000,
        mcs_count: 3,
        page0: 0x2000,
        page1: 0x1000,
        pc_mask: 0xFFFC,
    };

    /// The GTM of the chip named in an attach file (`TC275`, `TC27x`, `TC36x`, …), if it is
    /// known.
    pub fn for_chip(chip: &str) -> Option<Self> {
        let chip = chip.to_ascii_uppercase();
        if chip.starts_with("TC27") {
            Some(Self::TC27X)
        } else if chip.starts_with("TC36") {
            Some(Self::TC36X)
        } else {
            None
        }
    }

    /// R0 of channel `channel` of MCS `mcs`; R1–R7 and CTRL follow it.
    pub fn channel_registers(&self, mcs: usize, channel: usize) -> u32 {
        self.base + self.mcs_registers + mcs as u32 * MCS_STRIDE + channel as u32 * CHANNEL_STRIDE
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

/// [`Gtm::memory_size`] of the TC27x.
pub fn memory_size(mcfg_ctrl: u32, mcs: usize) -> (u32, u32) {
    Gtm::TC27X.memory_size(mcfg_ctrl, mcs)
}

impl Gtm {
    /// `(MP0, MP1)` of MCS `mcs`: page 0 is MCS addresses `0..MP0`, page 1 `MP0..MP1` (the
    /// manual's memory layout parameters, `MCS_RAM1_EN_ADDR_MSB = 0`). The instance before MCS
    /// 0 is the last one.
    pub fn memory_size(&self, mcfg_ctrl: u32, mcs: usize) -> (u32, u32) {
        let count = self.mcs_count;
        let before = (mcs + count - 1) % count;
        let own = Layout::of(mcfg_ctrl, mcs);
        let previous = Layout::of(mcfg_ctrl, before);
        let mp0 = match previous {
            Layout::Default => self.page0,
            Layout::Swap => self.page1,
            Layout::Borrow => 0x0,
        };
        let extra = match own {
            Layout::Default => self.page1,
            Layout::Swap => self.page0,
            Layout::Borrow => self.page0 + self.page1,
        };
        // Page 1 is the instance's own small page, unless it gave that away (SWAP) for the next
        // instance's large page; BORROW adds the next instance's large page to its own small one.
        (mp0, mp0 + extra)
    }
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
    /// The program counter, as [`GtmView`] kept it (`PC` with the GTM's [`Gtm::pc_mask`]).
    pub fn pc(&self) -> u32 {
        self.pc
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

/// A write of [`GtmView::switch`]: one 32-bit word to a channel's control register.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SwitchWrite {
    pub address: u32,
    pub value: u32,
    pub mcs: usize,
    pub channel: usize,
    pub on: bool,
}

impl SwitchWrite {
    /// The `writeMemory` arguments: four bytes at a word address, which the TriCore bus writes
    /// as one 32-bit access (the MCS registers take no other).
    pub fn arguments(&self) -> dap_types::WriteMemoryArguments {
        dap_types::WriteMemoryArguments {
            memory_reference: format!("{:#010x}", self.address),
            offset: None,
            allow_partial: Some(false),
            data: crate::memory::encode_base64(&self.value.to_le_bytes()),
        }
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

/// A read of [`GtmView::ranges`]: the ranges, and the MCS and channel they were asked for.
#[derive(Clone, Debug)]
pub struct GtmRead {
    pub ranges: Vec<MemoryRange>,
    mcs: usize,
    selected: usize,
}

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
        if mcs != self.mcs && mcs < self.gtm.mcs_count {
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
        self.mcfg_ctrl.map(|m| self.gtm.memory_size(m, self.mcs))
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
    pub fn ranges(&self) -> GtmRead {
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
        GtmRead {
            ranges,
            mcs: self.mcs,
            selected: self.selected,
        }
    }

    /// The answer to [`Self::ranges`] (asked as `asked`). An answer for another MCS than the one
    /// shown is dropped (the MCS was switched while it was read), and its code window when
    /// another channel is selected now.
    pub fn answered(&mut self, asked: &GtmRead, answer: &[RangeData]) {
        if asked.mcs != self.mcs {
            return;
        }
        let code_asked = asked.selected == self.selected;
        let asked = &asked.ranges;
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
                        pc: pc[0] & self.gtm.pc_mask,
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
        if let Some(code) = answer.get(1 + 2 * CHANNELS).filter(|_| code_asked) {
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

    /// The write that switches channel `channel` of the MCS shown on or off: `MCSi_CHx_CTRL`
    /// with `EN` (the only writable bit), one 32-bit word. Off keeps the channel's PC and
    /// registers, on goes on from there.
    pub fn switch(&self, channel: usize, on: bool) -> Option<SwitchWrite> {
        (channel < CHANNELS).then(|| SwitchWrite {
            address: self.gtm.channel_ctrl(self.mcs, channel),
            value: u32::from(on),
            mcs: self.mcs,
            channel,
            on,
        })
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
        assert_eq!(Gtm::for_chip("TC36x"), Some(Gtm::TC36X));
        assert!(Gtm::for_chip("TC39x").is_none());
    }

    /// The TC36x as the board reads it (gtm_demo -Board tc36x on MCS1) and the SFR headers say.
    #[test]
    fn the_tc36x_addresses_and_pages() {
        let gtm = Gtm::for_chip("TC36x").unwrap();
        assert_eq!(gtm.channel_registers(1, 0), 0xF01F_1000);
        assert_eq!(gtm.channel_ctrl(1, 0), 0xF01F_1020);
        assert_eq!(gtm.channel_pc(1, 1), 0xF01F_10C0);
        assert_eq!(gtm.memory(1), 0xF014_0000);
        assert_eq!(gtm.memory(2), 0xF014_8000);
        // DEFAULT: 8 KB and 4 KB each.
        for mcs in 0..gtm.mcs_count {
            assert_eq!(gtm.memory_size(0, mcs), (0x2000, 0x3000));
        }
        // MCS2 borrows: MCS0, after it (the last one wraps), has no page 0 of its own.
        assert_eq!(gtm.memory_size(0b10_0000, 2), (0x2000, 0x5000));
        assert_eq!(gtm.memory_size(0b10_0000, 0), (0x0, 0x1000));
        // MCS0 swaps with MCS1.
        assert_eq!(gtm.memory_size(0b01, 0), (0x2000, 0x4000));
        assert_eq!(gtm.memory_size(0b01, 1), (0x1000, 0x2000));
    }

    fn data(words: &[u32]) -> RangeData {
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        RangeData {
            address: String::new(),
            data: Some(encode_base64(&bytes)),
            error: None,
        }
    }

    fn answer(view: &GtmView, pc0: u32, r0: u32, code: &[u32]) -> (GtmRead, Vec<RangeData>) {
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
        if asked.ranges.len() > answer.len() {
            answer.push(data(code));
        }
        (asked, answer)
    }

    #[test]
    fn channels_changes_and_code_around_the_pc() {
        let mut view = GtmView::new(Gtm::for_chip("TC275").unwrap());
        view.show_mcs(1);
        // First read: no code window yet (the PC is not known).
        assert_eq!(view.ranges().ranges.len(), 1 + 2 * CHANNELS);
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
        let asked = view.ranges().ranges;
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

    #[test]
    fn an_answer_for_the_mcs_shown_before_is_dropped() {
        let mut view = GtmView::new(Gtm::for_chip("TC36x").unwrap());
        let (asked, reply) = answer(&view, 0x8, 5, &[]);
        view.answered(&asked, &reply);
        // The code window of MCS0 is asked, then MCS1 is shown before the answer comes.
        let (asked, reply) = answer(&view, 0xC, 6, &[0x1000_0000; 24]);
        assert_eq!(asked.ranges.len(), 2 + 2 * CHANNELS);
        view.show_mcs(1);
        view.answered(&asked, &reply);
        assert_eq!(view.mcs(), 1);
        assert!(view.rows().is_empty());
        assert!(view.code().unwrap().is_empty());

        // Another channel selected while its code was read: the channels count, the code not.
        let mut view = GtmView::new(Gtm::for_chip("TC36x").unwrap());
        let (asked, reply) = answer(&view, 0x8, 5, &[]);
        view.answered(&asked, &reply);
        let (asked, reply) = answer(&view, 0xC, 6, &[0x1000_0000; 24]);
        view.select(2);
        view.answered(&asked, &reply);
        assert_eq!(view.rows()[0].state.as_ref().unwrap().pc(), 0xC);
        assert!(view.code().unwrap().is_empty());
    }

    #[test]
    fn a_channel_is_switched_by_its_control_register() {
        let mut view = GtmView::new(Gtm::for_chip("TC275").unwrap());
        view.show_mcs(1);
        let off = view.switch(2, false).unwrap();
        assert_eq!(off.address, 0xF013_1120);
        assert_eq!(off.value, 0);
        let on = view.switch(2, true).unwrap();
        let arguments = on.arguments();
        assert_eq!(arguments.memory_reference, "0xf0131120");
        assert_eq!(arguments.data, encode_base64(&[1, 0, 0, 0]));
        assert!(view.switch(8, true).is_none());
    }
}
