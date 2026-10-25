//! The expressions the probe-rs server reads while the cores run (`liveWatch`, the fork's
//! `rpc/functions/debug_vars/c_expr.rs`), as ORTI files write them: paths to global variables
//! (`OsTcb[1].state`, `p->next->state`, `*p`, `OsTcb[OsCurrentTask].state`), integers, `&path`,
//! casts (`(unsigned char)x`, `*(uint32 *)0x70001000`), C's unary and binary operators and
//! `c ? a : b`. This is the first check, on the text: what passes is sent, and the server says
//! why it could not read the rest (a name it does not know, a type whose size it does not know).
//! Function calls, assignments, `++`/`--`, `,`, `sizeof` and strings are never read.

/// Whether `expression` is one the server reads.
pub fn is_supported(expression: &str) -> bool {
    let text = expression.trim();
    if text.is_empty() || text.contains('"') || text.contains(',') || text.contains(';') {
        return false;
    }
    if text.contains("++") || text.contains("--") {
        return false;
    }
    let bytes = text.as_bytes();
    let mut depth: i32 = 0;
    let mut square: i32 = 0;
    let mut previous_word: Option<&str> = None;
    let mut at = 0;
    while at < bytes.len() {
        let c = bytes[at] as char;
        if c.is_ascii_alphanumeric() || c == '_' {
            let end = text[at..]
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .map_or(text.len(), |n| at + n);
            let word = &text[at..end];
            if word == "sizeof" {
                return false;
            }
            // A number is digits and letters of its base or suffix (`0x2A`, `2u`), not `1x`.
            if word.starts_with(|c: char| c.is_ascii_digit()) {
                let digits = word.trim_end_matches(['u', 'U', 'l', 'L']);
                let number = match digits
                    .strip_prefix("0x")
                    .or_else(|| digits.strip_prefix("0X"))
                {
                    Some(hex) => !hex.is_empty() && hex.chars().all(|c| c.is_ascii_hexdigit()),
                    None => match digits
                        .strip_prefix("0b")
                        .or_else(|| digits.strip_prefix("0B"))
                    {
                        Some(binary) => {
                            !binary.is_empty() && binary.chars().all(|c| c == '0' || c == '1')
                        }
                        None => digits.chars().all(|c| c.is_ascii_digit()),
                    },
                };
                if !number {
                    return false;
                }
            }
            previous_word = Some(word);
            at = end;
            continue;
        }
        match c {
            '(' => {
                // A name right before `(` is a function call.
                if previous_word.is_some() {
                    return false;
                }
                depth += 1;
            }
            ')' => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            '[' => square += 1,
            ']' => {
                square -= 1;
                if square < 0 {
                    return false;
                }
            }
            '=' => {
                // `==`, `!=`, `<=`, `>=` only.
                let before = at.checked_sub(1).map(|i| bytes[i] as char);
                let after = bytes.get(at + 1).map(|b| *b as char);
                if after == Some('=') {
                    at += 2;
                    previous_word = None;
                    continue;
                }
                if !matches!(before, Some('=' | '!' | '<' | '>')) {
                    return false;
                }
            }
            '\'' => {
                // A character: 'a' or '\n'.
                let rest = &text[at + 1..];
                let len = if rest.starts_with('\\') { 2 } else { 1 };
                if rest.get(len..len + 1) != Some("'") {
                    return false;
                }
                at += len + 2;
                previous_word = None;
                continue;
            }
            '.' => {
                // A member name follows.
                let rest = text[at + 1..].trim_start();
                if !rest.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') {
                    return false;
                }
            }
            c if c.is_whitespace() => {
                at += 1;
                continue;
            }
            c if "+-*/%<>&|^!~?:".contains(c) => {}
            _ => return false,
        }
        previous_word = None;
        at += 1;
    }
    if depth != 0 || square != 0 {
        return false;
    }
    // Ends with a value, not an operator.
    let last = text.chars().last().unwrap_or(' ');
    last.is_ascii_alphanumeric() || matches!(last, '_' | ')' | ']' | '\'')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_the_server_reads_and_the_rest() {
        for supported in [
            "g_ticks",
            "OsTcb[1].state",
            "p->next->state",
            "*p",
            "a[0x10]",
            "(unsigned)x / 2",
            "a + 1",
            "a[i]",
            "&x",
            "OsTcb[OsCurrentTask].state",
            "(unsigned char)(Os_Core0.State & 0x0F)",
            "*(uint32 *)0x70001000",
            "x == 'a' ? 1 : 0",
            "a >= 2 && b != 3",
            "2u + 0b101",
        ] {
            assert!(is_supported(supported), "{supported}");
        }
        for unsupported in [
            "1x",
            "a.",
            "a[1",
            "",
            "f(1)",
            "a = 1",
            "a++",
            "x, y",
            "sizeof(int)",
            "\"text\"",
            "(a",
            "a)",
            "a +",
            "0xZZ",
        ] {
            assert!(!is_supported(unsupported), "{unsupported}");
        }
    }
}
