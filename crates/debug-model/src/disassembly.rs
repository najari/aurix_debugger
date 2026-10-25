//! The instructions around a program counter, read further on either side.
//!
//! The window is asked around the stack frame's instruction pointer reference, and reading more
//! asks around the same reference with larger instruction offsets: an adapter may route a
//! reference (probe-rs sends `0x8003a552@1` to core 1), and the plain address of a shown
//! instruction would lose that. Finding the instruction starts before an address is a guess on a
//! variable length instruction set, so an answer may overlap the shown instructions: only the
//! ones beyond the edge are kept.

use dap_types::{DisassembledInstruction, DisassembledInstructionPresentationHint};

use crate::protocol::DisassembleSignedArguments;
use crate::reference::parse_address;

/// Instructions asked for before the program counter, by default.
pub const BEFORE: i64 = 24;
/// Instructions asked for in all at first (before, at and after the program counter).
pub const COUNT: u64 = 80;
/// Instructions asked for by one "read more".
pub const MORE: u64 = 40;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Before,
    After,
}

#[derive(Clone, Debug, Default)]
pub struct DisassemblyWindow {
    /// The reference the instructions were asked around (a stack frame's instruction pointer).
    reference: String,
    /// The address of the reference, the program counter.
    pc: Option<u64>,
    /// The instruction offsets, from the reference, of the first and the last instruction.
    offsets: (i64, i64),
    instructions: Vec<DisassembledInstruction>,
}

impl DisassemblyWindow {
    /// An empty window around `reference`.
    pub fn new(reference: impl Into<String>) -> Self {
        let reference = reference.into();
        Self {
            pc: parse_address(&reference),
            reference,
            ..Default::default()
        }
    }

    pub fn reference(&self) -> &str {
        &self.reference
    }

    pub fn pc(&self) -> Option<u64> {
        self.pc
    }

    pub fn instructions(&self) -> &[DisassembledInstruction] {
        &self.instructions
    }

    /// The request for the first instructions: `before` instructions before the reference and
    /// `count` in all.
    pub fn first_request(&self, before: i64, count: u64) -> DisassembleSignedArguments {
        DisassembleSignedArguments {
            memory_reference: self.reference.clone(),
            instruction_offset: -before,
            instruction_count: count,
            resolve_symbols: true,
        }
    }

    /// The answer to [`Self::first_request`] with `before`.
    pub fn set_first(&mut self, before: i64, instructions: Vec<DisassembledInstruction>) {
        self.offsets = (-before, -before + instructions.len() as i64 - 1);
        self.instructions = instructions;
    }

    /// The request for `count` more instructions on `side`, `None` while the window is empty.
    pub fn more_request(&self, side: Side, count: u64) -> Option<DisassembleSignedArguments> {
        if self.instructions.is_empty() {
            return None;
        }
        let instruction_offset = match side {
            Side::Before => self.offsets.0 - count as i64,
            Side::After => self.offsets.1 + 1,
        };
        Some(DisassembleSignedArguments {
            memory_reference: self.reference.clone(),
            instruction_offset,
            instruction_count: count,
            resolve_symbols: true,
        })
    }

    /// The answer to [`Self::more_request`]: adds the instructions beyond the edge on `side`, and
    /// returns how many (at the front for [`Side::Before`]).
    pub fn add_more(&mut self, side: Side, instructions: Vec<DisassembledInstruction>) -> usize {
        let edge = match side {
            Side::Before => self.instructions.first(),
            Side::After => self.instructions.last(),
        }
        .and_then(|instruction| parse_address(&instruction.address));
        let Some(edge) = edge else {
            return 0;
        };
        let received = instructions.len() as i64;
        match side {
            Side::Before => self.offsets.0 -= received,
            Side::After => self.offsets.1 += received,
        }
        let new = instructions
            .into_iter()
            .filter(|instruction| {
                parse_address(&instruction.address).is_some_and(|address| match side {
                    Side::Before => address < edge,
                    Side::After => address > edge,
                })
            })
            .collect::<Vec<_>>();
        let count = new.len();
        match side {
            Side::Before => {
                let shown = std::mem::replace(&mut self.instructions, new);
                self.instructions.extend(shown);
            }
            Side::After => self.instructions.extend(new),
        }
        count
    }

    /// The index of the instruction that holds the program counter. The program counter of a
    /// caller's frame may point into an instruction rather than at its start (probe-rs gives the
    /// address before the return address, inside the call): the instruction it falls in counts.
    pub fn pc_index(&self) -> Option<usize> {
        let pc = self.pc?;
        let addresses = self
            .instructions
            .iter()
            .map(|instruction| parse_address(&instruction.address))
            .collect::<Vec<_>>();
        let ix = addresses
            .iter()
            .rposition(|address| address.is_some_and(|address| address <= pc))?;
        let next = addresses[ix + 1..].iter().flatten().next();
        (addresses[ix] == Some(pc) || next.is_some_and(|next| *next > pc)).then_some(ix)
    }

    /// The source line that starts at instruction `ix`: the adapter gives the line on the first
    /// instruction of a line. The file name is the one the instruction names, if any.
    pub fn line_start(&self, ix: usize) -> Option<(Option<&str>, u64)> {
        let instruction = self.instructions.get(ix)?;
        let line = instruction.line?;
        let previous = ix
            .checked_sub(1)
            .and_then(|ix| self.instructions.get(ix))
            .and_then(|previous| previous.line);
        (previous != Some(line)).then(|| {
            let file = instruction
                .location
                .as_ref()
                .and_then(|source| source.name.as_deref());
            (file, line)
        })
    }
}

/// Whether the adapter marked the instruction as filler (unreadable memory, say).
pub fn is_filler(instruction: &DisassembledInstruction) -> bool {
    matches!(
        instruction.presentation_hint,
        Some(DisassembledInstructionPresentationHint::Invalid)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn instructions(addresses: &[u64]) -> Vec<DisassembledInstruction> {
        addresses
            .iter()
            .map(|address| {
                serde_json::from_value(json!({
                    "address": format!("0x{address:08X}"),
                    "instruction": "nop",
                }))
                .unwrap()
            })
            .collect()
    }

    #[test]
    fn reading_more_asks_around_the_reference_and_keeps_what_is_new() {
        let mut window = DisassemblyWindow::new("0x8021c028@0");
        assert_eq!(window.pc(), Some(0x8021_c028));
        assert_eq!(window.more_request(Side::Before, MORE), None);
        let first = window.first_request(2, 5);
        assert_eq!(first.instruction_offset, -2);
        window.set_first(
            2,
            instructions(&[
                0x8021_c020,
                0x8021_c024,
                0x8021_c028,
                0x8021_c02c,
                0x8021_c030,
            ]),
        );
        assert_eq!(window.pc_index(), Some(2));

        let before = window.more_request(Side::Before, 3).unwrap();
        assert_eq!(before.memory_reference, "0x8021c028@0");
        assert_eq!(before.instruction_offset, -5);
        // The answer overlaps the first shown instruction: it is not added twice.
        let added = window.add_more(
            Side::Before,
            instructions(&[0x8021_c018, 0x8021_c01c, 0x8021_c020]),
        );
        assert_eq!(added, 2);
        assert_eq!(window.pc_index(), Some(4));

        let after = window.more_request(Side::After, 2).unwrap();
        assert_eq!(after.instruction_offset, 3);
        assert_eq!(
            window.add_more(Side::After, instructions(&[0x8021_c034, 0x8021_c036])),
            2
        );
        assert_eq!(window.instructions().len(), 9);
        // The offsets follow what was received, not what was kept.
        assert_eq!(
            window
                .more_request(Side::Before, 1)
                .unwrap()
                .instruction_offset,
            -6
        );
        assert_eq!(
            window
                .more_request(Side::After, 1)
                .unwrap()
                .instruction_offset,
            5
        );
    }

    #[test]
    fn a_caller_frame_marks_the_instruction_it_points_into() {
        // 0x8021c022 is inside the 4 byte call at 0x8021c020.
        let mut window = DisassemblyWindow::new("0x8021c022@0");
        window.set_first(1, instructions(&[0x8021_c01c, 0x8021_c020, 0x8021_c024]));
        assert_eq!(window.pc_index(), Some(1));
        // Beyond the last instruction: not shown.
        let mut window = DisassemblyWindow::new("0x8021c030");
        window.set_first(0, instructions(&[0x8021_c020, 0x8021_c024]));
        assert_eq!(window.pc_index(), None);
        // Before the first: not shown either.
        let mut window = DisassemblyWindow::new("0x10");
        window.set_first(0, instructions(&[0x20]));
        assert_eq!(window.pc_index(), None);
    }

    #[test]
    fn a_line_starts_where_the_line_changes() {
        let mut window = DisassemblyWindow::new("0x10");
        let rows = [
            (0x10, Some(22)),
            (0x14, Some(22)),
            (0x18, Some(23)),
            (0x1c, None),
        ]
        .iter()
        .map(|(address, line)| {
            serde_json::from_value(json!({
                "address": format!("0x{address:x}"),
                "instruction": "nop",
                "line": line,
                "location": { "name": "demo.c" },
            }))
            .unwrap()
        })
        .collect();
        window.set_first(0, rows);
        assert_eq!(window.line_start(0), Some((Some("demo.c"), 22)));
        assert_eq!(window.line_start(1), None);
        assert_eq!(window.line_start(2), Some((Some("demo.c"), 23)));
        assert_eq!(window.line_start(3), None);
    }
}
