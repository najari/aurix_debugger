//! Addresses in DAP memory references.
//!
//! A memory reference is opaque to a DAP client, but adapters write the address first. In a
//! session of several cores, probe-rs adds the core after `@` (`0x8003a552@1`): a request with
//! that reference goes to that core.

/// Separates the core from the address in a probe-rs memory reference.
pub const CORE_SEPARATOR: char = '@';

/// The leading number of a memory reference or instruction address: hex with `0x`, else
/// decimal. `None` when the text does not start with a number.
pub fn parse_address(text: &str) -> Option<u64> {
    let text = text.trim();
    let (digits, radix) = match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        Some(hex) => (hex, 16),
        None => (text, 10),
    };
    let end = digits
        .find(|c: char| !c.is_digit(radix))
        .unwrap_or(digits.len());
    u64::from_str_radix(&digits[..end], radix).ok()
}

/// The reference without its `@core` suffix, and the core.
pub fn split_core(reference: &str) -> (&str, Option<u64>) {
    match reference.rsplit_once(CORE_SEPARATOR) {
        Some((address, core)) => match core.trim().parse() {
            Ok(core) => (address, Some(core)),
            Err(_) => (reference, None),
        },
        None => (reference, None),
    }
}

/// The reference with the `@core` suffix of `core` (replacing one it has).
pub fn with_core(reference: &str, core: u64) -> String {
    format!("{}{CORE_SEPARATOR}{core}", split_core(reference).0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_are_the_leading_number() {
        assert_eq!(parse_address("0x8021C028"), Some(0x8021_c028));
        assert_eq!(parse_address("0x8003a552@1"), Some(0x8003_a552));
        assert_eq!(parse_address(" 4096 "), Some(4096));
        assert_eq!(parse_address("frame-7"), None);
        assert_eq!(parse_address("0x"), None);
    }

    #[test]
    fn the_core_suffix_is_split_and_added() {
        assert_eq!(split_core("0x7000a004@1"), ("0x7000a004", Some(1)));
        assert_eq!(split_core("0x7000a004"), ("0x7000a004", None));
        assert_eq!(split_core("name@x"), ("name@x", None));
        assert_eq!(with_core("0x7000a004@1", 2), "0x7000a004@2");
        assert_eq!(with_core("0x7000a004", 0), "0x7000a004@0");
    }
}
