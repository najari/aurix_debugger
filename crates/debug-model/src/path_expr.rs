//! The expressions the probe-rs server reads by path (`liveWatch`, the fork's
//! `rpc/functions/debug_vars.rs`): a global variable's name, then `.member`, `[index]` and
//! `->member`, with `*` in front for a pointer's target. Casts, arithmetic and `&` are not read.

/// Whether `expression` is a path the server reads.
pub fn is_supported(expression: &str) -> bool {
    fn name(text: &str) -> Option<&str> {
        let end = text
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .unwrap_or(text.len());
        let (name, rest) = text.split_at(end);
        (!name.is_empty() && !name.starts_with(|c: char| c.is_ascii_digit())).then_some(rest)
    }
    let mut text = expression.trim();
    while let Some(rest) = text.strip_prefix('*') {
        text = rest.trim_start();
    }
    let Some(mut text) = name(text) else {
        return false;
    };
    loop {
        text = text.trim_start();
        if text.is_empty() {
            return true;
        }
        let rest = if let Some(rest) = text.strip_prefix("->") {
            name(rest.trim_start())
        } else if let Some(rest) = text.strip_prefix('.') {
            name(rest.trim_start())
        } else if let Some(rest) = text.strip_prefix('[') {
            rest.split_once(']').and_then(|(index, rest)| {
                let index = index.trim();
                let number = match index
                    .strip_prefix("0x")
                    .or_else(|| index.strip_prefix("0X"))
                {
                    Some(hex) => u64::from_str_radix(hex, 16).is_ok(),
                    None => index.parse::<u64>().is_ok(),
                };
                number.then_some(rest)
            })
        } else {
            None
        };
        match rest {
            Some(rest) => text = rest,
            None => return false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_and_the_rest() {
        for supported in [
            "g_ticks",
            "OsTcb[1].state",
            "p->next->state",
            "*p",
            "a[0x10]",
        ] {
            assert!(is_supported(supported), "{supported}");
        }
        for unsupported in [
            "(unsigned)x / 2",
            "a + 1",
            "a[i]",
            "&x",
            "1x",
            "a.",
            "a[1",
            "",
        ] {
            assert!(!is_supported(unsupported), "{unsupported}");
        }
    }
}
