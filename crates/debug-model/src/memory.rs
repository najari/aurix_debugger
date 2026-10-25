//! A memory window: [`ROWS`] rows of [`ROW`] bytes read with DAP `readMemory`, again and again
//! while it is shown (probe-rs reads memory while the cores run). In a session of several cores
//! the window reads through one core (`@core`), so a core's local addresses (`0xD000_0000`,
//! its scratch-pad RAM) read that core's memory. The bytes that changed since the last read are
//! marked, and an answer to a read made before the window moved is dropped.

use dap_types::ReadMemoryArguments;

use crate::reference::{CORE_SEPARATOR, with_core};

/// Bytes per row.
pub const ROW: usize = 16;
/// Rows shown.
pub const ROWS: usize = 16;
/// Bytes read at once.
pub const SIZE: usize = ROW * ROWS;

/// An address typed by the user: hex (with or without `0x`), and an optional `@core`.
pub fn parse_location(text: &str) -> Result<(u64, Option<u64>), String> {
    let text = text.trim();
    let (address, core) = match text.split_once(CORE_SEPARATOR) {
        Some((address, core)) => {
            let core = core
                .trim()
                .parse()
                .map_err(|_| format!("not a core number: {core}"))?;
            (address.trim(), Some(core))
        }
        None => (text, None),
    };
    let digits = address
        .strip_prefix("0x")
        .or_else(|| address.strip_prefix("0X"))
        .unwrap_or(address)
        .replace('_', "");
    if digits.is_empty() {
        return Err("an address, such as 0x7000A000".into());
    }
    u64::from_str_radix(&digits, 16)
        .map(|address| (address, core))
        .map_err(|_| format!("not a hex address: {address}"))
}

#[derive(Clone, Debug, Default)]
pub struct MemoryView {
    /// The first address shown, a multiple of [`ROW`].
    start: Option<u64>,
    /// The core the memory is read through.
    core: Option<u64>,
    /// The bytes read, `None` where the adapter could not read; empty before the first read.
    bytes: Vec<Option<u8>>,
    /// The bytes that differ from the read before.
    changed: Vec<bool>,
    error: Option<String>,
    /// The generation of the read under way, if one is.
    busy: Option<u64>,
    /// Counts the moves of the window; an answer of another generation is dropped.
    generation: u64,
}

impl MemoryView {
    pub fn start(&self) -> Option<u64> {
        self.start
    }

    pub fn core(&self) -> Option<u64> {
        self.core
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Shows the memory from `address` (rounded down to a row) through `core`.
    pub fn show(&mut self, address: u64, core: Option<u64>) {
        self.start = Some(address - address % ROW as u64);
        self.core = core;
        self.moved();
    }

    /// Moves the window by `rows` rows (back when negative).
    pub fn scroll(&mut self, rows: i64) {
        if let Some(start) = self.start {
            let delta = rows.unsigned_abs() * ROW as u64;
            self.start = Some(if rows < 0 {
                start.saturating_sub(delta)
            } else {
                start.saturating_add(delta)
            });
            self.moved();
        }
    }

    /// Reads through `core` from now on.
    pub fn set_core(&mut self, core: Option<u64>) {
        if self.core != core {
            self.core = core;
            self.moved();
        }
    }

    fn moved(&mut self) {
        self.generation += 1;
        self.bytes.clear();
        self.changed.clear();
        self.error = None;
        self.busy = None;
    }

    /// The read to send now with its generation, none while one is under way or nothing is
    /// shown. [`Self::answered`] takes its answer.
    pub fn take_request(&mut self) -> Option<(ReadMemoryArguments, u64)> {
        let start = self.start?;
        if self.busy.is_some() {
            return None;
        }
        self.busy = Some(self.generation);
        let reference = format!("{start:#010x}");
        let memory_reference = match self.core {
            Some(core) => with_core(&reference, core),
            None => reference,
        };
        Some((
            ReadMemoryArguments {
                memory_reference,
                offset: None,
                count: SIZE as u64,
            },
            self.generation,
        ))
    }

    /// The answer to the read of `generation`: the data (base64) and how many bytes at its end
    /// could not be read, or why nothing could.
    pub fn answered(
        &mut self,
        generation: u64,
        answer: Result<(Option<String>, Option<u64>), String>,
    ) {
        if self.busy == Some(generation) {
            self.busy = None;
        }
        if generation != self.generation {
            return;
        }
        let bytes = match answer {
            Ok((data, _unreadable)) => {
                let data = match data.as_deref().map(decode_base64).transpose() {
                    Ok(data) => data.unwrap_or_default(),
                    Err(error) => {
                        self.error = Some(error);
                        return;
                    }
                };
                self.error = None;
                (0..SIZE)
                    .map(|ix| data.get(ix).copied())
                    .collect::<Vec<_>>()
            }
            Err(error) => {
                self.error = Some(error);
                vec![None; SIZE]
            }
        };
        if self.bytes.len() != bytes.len() {
            self.changed = vec![false; bytes.len()];
        } else if self.bytes != bytes {
            // Only bytes read both times compare.
            self.changed = self
                .bytes
                .iter()
                .zip(&bytes)
                .map(|(a, b)| a.is_some() && b.is_some() && a != b)
                .collect();
        }
        // The same bytes again (a halted core): the marks of the last change stay.
        self.bytes = bytes;
    }

    /// The rows read: address, bytes and their change marks.
    pub fn rows(&self) -> impl Iterator<Item = (u64, &[Option<u8>], &[bool])> {
        let start = self.start.unwrap_or(0);
        self.bytes
            .chunks(ROW)
            .zip(self.changed.chunks(ROW))
            .enumerate()
            .map(move |(ix, (bytes, changed))| (start + (ix * ROW) as u64, bytes, changed))
    }
}

/// The bytes of standard base64 (padding optional).
pub fn decode_base64(text: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let mut buffer = 0u32;
    let mut bits = 0;
    for c in text.bytes().filter(|c| !c.is_ascii_whitespace()) {
        let value = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            _ => return Err(format!("not base64: {:?}", c as char)),
        };
        buffer = (buffer << 6) | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
            buffer &= (1 << bits) - 1;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locations_are_hex_with_an_optional_core() {
        assert_eq!(parse_location("0x7000A000"), Ok((0x7000_a000, None)));
        assert_eq!(parse_location("d000a000@1"), Ok((0xd000_a000, Some(1))));
        assert_eq!(
            parse_location(" 0x7000_a000 @ 2 "),
            Ok((0x7000_a000, Some(2)))
        );
        assert!(parse_location("g_ticks").is_err());
        assert!(parse_location("0x10@x").is_err());
        assert!(parse_location("").is_err());
    }

    #[test]
    fn base64_decodes() {
        assert_eq!(decode_base64("AAEC/w==").unwrap(), [0, 1, 2, 255]);
        assert_eq!(decode_base64("aGVsbG8").unwrap(), b"hello");
        assert!(decode_base64("a*b").is_err());
    }

    fn answer(bytes: &[u8]) -> Result<(Option<String>, Option<u64>), String> {
        // Encodes as probe-rs does, through the decoder's inverse for these test bytes.
        const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut text = String::new();
        for chunk in bytes.chunks(3) {
            let n = chunk
                .iter()
                .enumerate()
                .fold(0u32, |n, (ix, b)| n | u32::from(*b) << (16 - 8 * ix));
            for ix in 0..=chunk.len() {
                text.push(ALPHABET[(n >> (18 - 6 * ix) & 63) as usize] as char);
            }
            for _ in chunk.len()..3 {
                text.push('=');
            }
        }
        Ok((Some(text), None))
    }

    #[test]
    fn reads_go_through_the_core_and_changes_are_marked() {
        let mut view = MemoryView::default();
        assert!(view.take_request().is_none());
        view.show(0xd000_a004, Some(1));
        assert_eq!(view.start(), Some(0xd000_a000));
        let (request, generation) = view.take_request().unwrap();
        assert_eq!(request.memory_reference, "0xd000a000@1");
        assert_eq!(request.count, SIZE as u64);
        // Under way: not asked again.
        assert!(view.take_request().is_none());

        let mut memory = vec![0u8; SIZE];
        view.answered(generation, answer(&memory));
        assert_eq!(view.rows().count(), ROWS);
        assert!(
            view.rows()
                .all(|(_, _, changed)| changed.iter().all(|c| !c))
        );

        memory[17] = 0x2a;
        let (_, generation) = view.take_request().unwrap();
        view.answered(generation, answer(&memory));
        let (address, bytes, changed) = view.rows().nth(1).unwrap();
        assert_eq!(address, 0xd000_a010);
        assert_eq!(bytes[1], Some(0x2a));
        assert!(changed[1] && !changed[0]);

        // Read again unchanged (the core is halted): the mark stays until the next change.
        let (_, generation) = view.take_request().unwrap();
        view.answered(generation, answer(&memory));
        assert!(view.rows().nth(1).unwrap().2[1]);
        memory[0] = 1;
        let (_, generation) = view.take_request().unwrap();
        view.answered(generation, answer(&memory));
        assert!(!view.rows().nth(1).unwrap().2[1]);
        assert!(view.rows().next().unwrap().2[0]);
    }

    #[test]
    fn an_answer_from_before_a_move_is_dropped() {
        let mut view = MemoryView::default();
        view.show(0x7000_a000, None);
        let (request, old) = view.take_request().unwrap();
        assert_eq!(request.memory_reference, "0x7000a000");
        view.scroll(-1);
        assert_eq!(view.start(), Some(0x7000_9ff0));
        view.answered(old, answer(&[1; SIZE]));
        assert_eq!(view.rows().count(), 0);

        // A short answer: the rest could not be read.
        let (_, generation) = view.take_request().unwrap();
        view.answered(generation, answer(&[7; 20]));
        let (_, bytes, _) = view.rows().nth(1).unwrap();
        assert_eq!((bytes[3], bytes[4]), (Some(7), None));

        let (_, generation) = view.take_request().unwrap();
        view.answered(generation, Err("Could not read any data".into()));
        assert_eq!(view.error(), Some("Could not read any data"));
        assert!(
            view.rows()
                .all(|(_, bytes, _)| bytes.iter().all(Option::is_none))
        );
    }
}
