//! A memory window: [`ROWS`] rows of [`ROW`] bytes read with DAP `readMemory`, again and again
//! while it is shown (probe-rs reads memory while the cores run). In a session of several cores
//! the window reads through one core (`@core`), so a core's local addresses (`0xD000_0000`,
//! its scratch-pad RAM) read that core's memory. The bytes that changed since the last read are
//! marked, and an answer to a read made before the window moved is dropped.

use dap_types::{ReadMemoryArguments, WriteMemoryArguments};

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
    let value =
        u64::from_str_radix(&digits, 16).map_err(|_| format!("not a hex address: {address}"))?;
    if value > LAST_ADDRESS {
        return Err(format!(
            "{address} is beyond the 32-bit addresses of the AURIX"
        ));
    }
    Ok((value, core))
}

/// The last address of the AURIX's 32-bit address space.
pub const LAST_ADDRESS: u64 = 0xFFFF_FFFF;
/// The last start of the window: its last row ends at [`LAST_ADDRESS`].
const LAST_START: u64 = LAST_ADDRESS + 1 - SIZE as u64;

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
    /// The RAM of the chip, the only memory written.
    rams: RamMap,
}

impl MemoryView {
    /// Sets the chip (`TC275`, `TC36x`) whose RAM the view may write.
    pub fn set_chip(&mut self, chip: Option<&str>) {
        self.rams = RamMap::for_chip(chip);
    }

    pub fn start(&self) -> Option<u64> {
        self.start
    }

    pub fn core(&self) -> Option<u64> {
        self.core
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Shows the memory from `address` (rounded down to a row) through `core`; the window
    /// stays inside the 32-bit address space.
    pub fn show(&mut self, address: u64, core: Option<u64>) {
        self.start = Some((address - address % ROW as u64).min(LAST_START));
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
                start.saturating_add(delta).min(LAST_START)
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

/// A RAM of a chip that the memory view may write: name, start, end (exclusive), and the
/// core whose local address it is, if it is a core's scratch-pad RAM (from probe-rs's
/// `TC27x.yaml`, `TC36x.yaml`).
#[derive(Debug)]
struct Ram {
    name: &'static str,
    start: u64,
    end: u64,
    core: Option<u64>,
}

const TC27X_RAM: &[Ram] = &[
    Ram {
        name: "DSPR0",
        start: 0x7000_0000,
        end: 0x7001_c000,
        core: Some(0),
    },
    Ram {
        name: "PSPR0",
        start: 0x7010_0000,
        end: 0x7010_6000,
        core: Some(0),
    },
    Ram {
        name: "DSPR1",
        start: 0x6000_0000,
        end: 0x6001_e000,
        core: Some(1),
    },
    Ram {
        name: "PSPR1",
        start: 0x6010_0000,
        end: 0x6010_8000,
        core: Some(1),
    },
    Ram {
        name: "DSPR2",
        start: 0x5000_0000,
        end: 0x5001_e000,
        core: Some(2),
    },
    Ram {
        name: "PSPR2",
        start: 0x5010_0000,
        end: 0x5010_8000,
        core: Some(2),
    },
    Ram {
        name: "LMU",
        start: 0x9000_0000,
        end: 0x9000_8000,
        core: None,
    },
    Ram {
        name: "LMU (not cached)",
        start: 0xb000_0000,
        end: 0xb000_8000,
        core: None,
    },
];

/// The TC36x (TriBoard TC3X5): two cores with 192 KiB of DSPR and 32 KiB of PSPR each, and two
/// 64 KiB DLMUs, also seen not cached in segment B (the same RAM: a write through one address
/// reads back through the other on the board).
const TC36X_RAM: &[Ram] = &[
    Ram {
        name: "DSPR0",
        start: 0x7000_0000,
        end: 0x7003_0000,
        core: Some(0),
    },
    Ram {
        name: "PSPR0",
        start: 0x7010_0000,
        end: 0x7010_8000,
        core: Some(0),
    },
    Ram {
        name: "DSPR1",
        start: 0x6000_0000,
        end: 0x6003_0000,
        core: Some(1),
    },
    Ram {
        name: "PSPR1",
        start: 0x6010_0000,
        end: 0x6010_8000,
        core: Some(1),
    },
    Ram {
        name: "DLMU0",
        start: 0x9000_0000,
        end: 0x9001_0000,
        core: None,
    },
    Ram {
        name: "DLMU1",
        start: 0x9001_0000,
        end: 0x9002_0000,
        core: None,
    },
    Ram {
        name: "DLMU0 (not cached)",
        start: 0xb000_0000,
        end: 0xb001_0000,
        core: None,
    },
    Ram {
        name: "DLMU1 (not cached)",
        start: 0xb001_0000,
        end: 0xb002_0000,
        core: None,
    },
];

/// The RAM a chip's memory view may write.
#[derive(Clone, Copy, Debug)]
pub struct RamMap {
    rams: &'static [Ram],
}

impl Default for RamMap {
    fn default() -> Self {
        Self::TC27X
    }
}

impl RamMap {
    pub const TC27X: Self = Self { rams: TC27X_RAM };
    pub const TC36X: Self = Self { rams: TC36X_RAM };

    /// The RAM of `chip` (`TC275`, `TC36x`, as an attach file names it); the TC27x's when the
    /// chip is not known.
    pub fn for_chip(chip: Option<&str>) -> Self {
        match chip {
            Some(chip) if chip.to_ascii_uppercase().starts_with("TC36") => Self::TC36X,
            _ => Self::TC27X,
        }
    }
}

/// The core of the HSM in a target variant that has it as a core (`TC36x_HSM`, as an attach
/// file names it): after the TriCore cores. It is a Cortex-M core with an address space of its
/// own, reached through the HSM debug window; it is not a TriCore core.
pub fn hsm_core(chip: Option<&str>) -> Option<u64> {
    let chip = chip?.to_ascii_uppercase();
    if !chip.ends_with("_HSM") {
        return None;
    }
    if chip.starts_with("TC36") {
        Some(2)
    } else if chip.starts_with("TC27") {
        Some(3)
    } else {
        None
    }
}

/// Where a core sees its own data and program scratch-pad RAM.
const LOCAL_DSPR: u64 = 0xd000_0000;
const LOCAL_PSPR: u64 = 0xc000_0000;

/// [`RamMap::writable`] for the TC27x.
pub fn writable_ram(address: u64, len: usize, core: Option<u64>) -> Result<String, String> {
    RamMap::TC27X.writable(address, len, core)
}

impl RamMap {
    /// The RAM that `len` bytes from `address`, read through `core`, are in: its name, or why
    /// they may not be written. Flash, the UCBs, the peripheral registers and anything not RAM
    /// are refused, as are bytes that leave the RAM they start in. A local address
    /// (`0xD000_0000`, `0xC000_0000`) names the RAM of the core it goes through.
    pub fn writable(&self, address: u64, len: usize, core: Option<u64>) -> Result<String, String> {
        let rams = self.rams;
        if len == 0 {
            return Err("nothing to write".into());
        }
        let end = address
            .checked_add(len as u64)
            .ok_or_else(|| "past the end of the address space".to_string())?;
        let local = |base: u64, kind: &str| {
            let core = core.ok_or_else(|| {
                format!("{address:#010x} is a core's local address: name the core (@0, @1, …)")
            })?;
            let name = format!("{kind}{core}");
            let ram = rams
                .iter()
                .find(|ram| ram.core == Some(core) && ram.name == name)
                .ok_or_else(|| format!("core {core} has no {kind}"))?;
            let size = ram.end - ram.start;
            if address >= base && end <= base + size {
                Ok(format!("{} (core {core}, local)", ram.name))
            } else {
                Err(format!(
                    "{address:#010x}..{end:#010x} leaves {} ({base:#010x}..{:#010x})",
                    ram.name,
                    base + size
                ))
            }
        };
        match address >> 28 {
            0xd => return local(LOCAL_DSPR, "DSPR"),
            0xc => return local(LOCAL_PSPR, "PSPR"),
            _ => {}
        }
        let Some(ram) = rams
            .iter()
            .find(|ram| address >= ram.start && address < ram.end)
        else {
            let what = match address >> 28 {
                0x8 | 0xa => "flash (program or data flash, UCBs)",
                0xf => "a peripheral register",
                _ => "not RAM",
            };
            return Err(format!("{address:#010x} is {what}: only RAM is written"));
        };
        if end > ram.end {
            return Err(format!(
                "{address:#010x}..{end:#010x} leaves {} (ends at {:#010x})",
                ram.name, ram.end
            ));
        }
        Ok(ram.name.to_string())
    }
}

/// Bytes typed in hex: `2A 00 ff`, `2a00ff`, `0x2A,0x00`.
pub fn parse_bytes(text: &str) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    for word in text
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|w| !w.is_empty())
    {
        let digits = word
            .strip_prefix("0x")
            .or_else(|| word.strip_prefix("0X"))
            .unwrap_or(word);
        if digits.is_empty() || digits.len() % 2 != 0 {
            return Err(format!("{word}: bytes are two hex digits each"));
        }
        for pair in digits.as_bytes().chunks(2) {
            let pair = std::str::from_utf8(pair).unwrap_or_default();
            bytes.push(u8::from_str_radix(pair, 16).map_err(|_| format!("{word}: not hex"))?);
        }
    }
    if bytes.is_empty() {
        return Err("bytes in hex, such as 2A 00".into());
    }
    Ok(bytes)
}

impl MemoryView {
    /// The request that writes `bytes` at `address` through the core the view reads through,
    /// if that is RAM; the RAM's name with it.
    pub fn write_request(
        &self,
        address: u64,
        bytes: &[u8],
    ) -> Result<(WriteMemoryArguments, String), String> {
        let ram = self.rams.writable(address, bytes.len(), self.core)?;
        let reference = format!("{address:#010x}");
        let memory_reference = match self.core {
            Some(core) => with_core(&reference, core),
            None => reference,
        };
        Ok((
            WriteMemoryArguments {
                memory_reference,
                offset: None,
                allow_partial: Some(false),
                data: encode_base64(bytes),
            },
            ram,
        ))
    }
}

/// Standard base64 with padding.
pub fn encode_base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut text = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (ix, b)| n | (u32::from(*b) << (16 - 8 * ix)));
        for ix in 0..=chunk.len() {
            text.push(ALPHABET[((n >> (18 - 6 * ix)) & 63) as usize] as char);
        }
        for _ in chunk.len()..3 {
            text.push('=');
        }
    }
    text
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
        assert_eq!(parse_location("0xFFFFFFFF"), Ok((0xFFFF_FFFF, None)));
        assert!(parse_location("FFFFFFFFFFFFFF80").is_err());
    }

    #[test]
    fn base64_decodes() {
        assert_eq!(decode_base64("AAEC/w==").unwrap(), [0, 1, 2, 255]);
        assert_eq!(decode_base64("aGVsbG8").unwrap(), b"hello");
        assert!(decode_base64("a*b").is_err());
    }

    #[test]
    fn the_window_stays_in_the_32_bit_address_space() {
        let mut view = MemoryView::default();
        view.show(0xFFFF_FFF0, None);
        assert_eq!(view.start(), Some(0x1_0000_0000 - SIZE as u64));
        view.scroll(3);
        assert_eq!(view.start(), Some(0x1_0000_0000 - SIZE as u64));
        view.scroll(-1);
        assert_eq!(view.start(), Some(0x1_0000_0000 - SIZE as u64 - ROW as u64));
        view.show(0x20, None);
        view.scroll(-5);
        assert_eq!(view.start(), Some(0));
    }

    fn answer(bytes: &[u8]) -> Result<(Option<String>, Option<u64>), String> {
        Ok((Some(encode_base64(bytes)), None))
    }

    #[test]
    fn base64_round_trips() {
        assert_eq!(encode_base64(&[0, 1, 2, 255]), "AAEC/w==");
        assert_eq!(encode_base64(b"hello"), "aGVsbG8=");
        let bytes = (0..=255).collect::<Vec<u8>>();
        assert_eq!(decode_base64(&encode_base64(&bytes)).unwrap(), bytes);
    }

    #[test]
    fn only_ram_is_written() {
        assert_eq!(writable_ram(0x7000_a004, 4, None).unwrap(), "DSPR0");
        assert_eq!(writable_ram(0x6000_0000, 1, Some(0)).unwrap(), "DSPR1");
        assert_eq!(writable_ram(0x9000_7fff, 1, None).unwrap(), "LMU");
        // Leaving the RAM, at its end.
        assert!(writable_ram(0x7001_bffe, 4, None).is_err());
        // Flash, the UCBs, peripheral registers, nothing.
        for address in [
            0x8021_c000,
            0xa000_0000,
            0xaf00_0000,
            0xaf10_0000,
            0xf000_0010,
            0x1000,
        ] {
            let error = writable_ram(address, 1, None).unwrap_err();
            assert!(error.contains("only RAM"), "{error}");
        }
        assert!(
            writable_ram(0x8021_c000, 1, None)
                .unwrap_err()
                .contains("flash")
        );
        // A local address is the RAM of the core it goes through, of that core's size.
        assert!(writable_ram(0xd000_a000, 4, None).is_err());
        assert_eq!(
            writable_ram(0xd000_a000, 4, Some(1)).unwrap(),
            "DSPR1 (core 1, local)"
        );
        // DSPR0 is 112 KiB, DSPR1 120 KiB.
        assert!(writable_ram(0xd001_c000, 1, Some(0)).is_err());
        assert!(writable_ram(0xd001_c000, 1, Some(1)).is_ok());
        assert!(writable_ram(0xc000_0000, 2, Some(2)).is_ok());
        assert!(writable_ram(0x7000_0000, 0, None).is_err());
    }

    #[test]
    fn the_hsm_core_follows_the_tricore_cores() {
        assert_eq!(hsm_core(Some("TC36x_HSM")), Some(2));
        assert_eq!(hsm_core(Some("tc36x_hsm")), Some(2));
        assert_eq!(hsm_core(Some("TC275_HSM")), Some(3));
        assert_eq!(hsm_core(Some("TC36x")), None);
        assert_eq!(hsm_core(Some("TC4D7_HSM")), None);
        assert_eq!(hsm_core(None), None);
        // The HSM variant has the TriCore chip's RAM.
        assert_eq!(
            RamMap::for_chip(Some("TC36x_HSM"))
                .writable(0x7001_c000, 4, None)
                .unwrap(),
            "DSPR0"
        );
    }

    #[test]
    fn the_tc36x_ram_is_its_own() {
        let rams = RamMap::for_chip(Some("TC36x"));
        // 192 KiB of DSPR: the TC27x's 112 KiB end is inside it.
        assert_eq!(rams.writable(0x7001_c000, 4, None).unwrap(), "DSPR0");
        assert_eq!(rams.writable(0x6002_fffc, 4, None).unwrap(), "DSPR1");
        assert!(rams.writable(0x7003_0000, 1, None).is_err());
        assert!(
            RamMap::for_chip(Some("TC275"))
                .writable(0x7001_c000, 4, None)
                .is_err()
        );
        assert_eq!(rams.writable(0x7010_7ffc, 4, None).unwrap(), "PSPR0");
        assert!(rams.writable(0x7010_8000, 1, None).is_err());
        assert_eq!(rams.writable(0x9001_0000, 4, None).unwrap(), "DLMU1");
        assert_eq!(
            rams.writable(0xb000_fffc, 4, None).unwrap(),
            "DLMU0 (not cached)"
        );
        assert_eq!(
            rams.writable(0xb001_0000, 4, None).unwrap(),
            "DLMU1 (not cached)"
        );
        // Past the DLMUs: a bus error on the board.
        assert!(rams.writable(0x9002_0000, 1, None).is_err());
        assert!(rams.writable(0xb002_0000, 1, None).is_err());
        // The local windows are of the core's size; there is no CPU2.
        assert_eq!(
            rams.writable(0xd002_fffc, 4, Some(1)).unwrap(),
            "DSPR1 (core 1, local)"
        );
        assert!(rams.writable(0xd003_0000, 1, Some(1)).is_err());
        assert!(rams.writable(0xd000_0000, 1, Some(2)).is_err());
        // Flash and registers stay refused.
        assert!(
            rams.writable(0x8040_0000, 1, None)
                .unwrap_err()
                .contains("flash")
        );
        assert!(
            rams.writable(0xf000_0010, 1, None)
                .unwrap_err()
                .contains("only RAM")
        );
        // An unknown chip is the TC27x.
        assert!(
            RamMap::for_chip(None)
                .writable(0x7001_c000, 1, None)
                .is_err()
        );
    }

    #[test]
    fn a_view_writes_the_ram_of_its_chip() {
        let mut view = MemoryView::default();
        view.show(0x7001_c000, None);
        assert!(view.write_request(0x7001_c000, &[1]).is_err());
        view.set_chip(Some("TC36x"));
        assert!(view.write_request(0x7001_c000, &[1]).is_ok());
    }

    #[test]
    fn bytes_are_typed_in_hex() {
        assert_eq!(parse_bytes("2A 00 ff").unwrap(), [0x2a, 0, 0xff]);
        assert_eq!(parse_bytes("2a00ff").unwrap(), [0x2a, 0, 0xff]);
        assert_eq!(parse_bytes("0x2A,0x00").unwrap(), [0x2a, 0]);
        assert!(parse_bytes("2").is_err());
        assert!(parse_bytes("zz").is_err());
        assert!(parse_bytes("  ").is_err());
    }

    #[test]
    fn a_write_goes_through_the_core_read_through() {
        let mut view = MemoryView::default();
        view.show(0xd000_a000, Some(1));
        let (request, ram) = view.write_request(0xd000_a004, &[0x2a]).unwrap();
        assert_eq!(request.memory_reference, "0xd000a004@1");
        assert_eq!(request.data, "Kg==");
        assert_eq!(ram, "DSPR1 (core 1, local)");
        assert!(view.write_request(0x8021_c000, &[0]).is_err());
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
