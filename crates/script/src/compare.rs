//! Comparing a value read from the target with the value a script expects, by the rules of the
//! probe-rs breakpoint conditions: as integers when both read as one (decimal, `0x`, `0b`, `'a'`,
//! `true`/`false`), else as numbers, else as text.

use std::cmp::Ordering;
use std::fmt;

/// A comparison operator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

impl Op {
    /// The operators, longest first, so that `<=` is not read as `<`.
    pub const ALL: [(&'static str, Op); 6] = [
        ("==", Op::Eq),
        ("!=", Op::Ne),
        ("<=", Op::Le),
        (">=", Op::Ge),
        ("<", Op::Lt),
        (">", Op::Gt),
    ];

    pub fn parse(text: &str) -> Option<Op> {
        Self::ALL
            .iter()
            .find(|(symbol, _)| *symbol == text)
            .map(|(_, op)| *op)
    }

    pub fn symbol(self) -> &'static str {
        Self::ALL
            .iter()
            .find(|(_, op)| *op == self)
            .map(|(symbol, _)| *symbol)
            .unwrap_or("?")
    }

    fn holds_for(self, ordering: Ordering) -> bool {
        match self {
            Op::Eq => ordering == Ordering::Equal,
            Op::Ne => ordering != Ordering::Equal,
            Op::Lt => ordering == Ordering::Less,
            Op::Le => ordering != Ordering::Greater,
            Op::Gt => ordering == Ordering::Greater,
            Op::Ge => ordering != Ordering::Less,
        }
    }

    /// Whether `actual <op> expected`.
    pub fn holds(self, actual: &str, expected: &str) -> bool {
        self.holds_for(compare(actual, expected))
    }
}

impl fmt::Display for Op {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.symbol())
    }
}

/// `<name> <op> <value>`: the first operator in `text` splits it.
pub fn split_comparison(text: &str) -> Result<(String, Op, String), String> {
    let text = text.trim();
    let Some((at, symbol, op)) = Op::ALL
        .iter()
        .filter_map(|(symbol, op)| text.find(symbol).map(|at| (at, *symbol, *op)))
        .min_by_key(|(at, _, _)| *at)
    else {
        return Err(format!(
            "`{text}` has no comparison: write <name> <op> <value> with ==, !=, <, <=, > or >="
        ));
    };
    let name = text[..at].trim();
    let value = text[at + symbol.len()..].trim();
    if name.is_empty() || name.contains(char::is_whitespace) {
        return Err(format!("`{text}` needs one name before {symbol}"));
    }
    if value.is_empty() {
        return Err(format!("`{text}` needs a value after {symbol}"));
    }
    Ok((name.to_string(), op, value.to_string()))
}

/// `value` as an integer; a value as probe-rs shows it may carry more after a space.
pub fn parse_integer(value: &str) -> Option<i128> {
    let value = value.trim();
    if let Some(c) = value
        .strip_prefix('\'')
        .and_then(|rest| rest.strip_suffix('\''))
        .and_then(|inner| {
            let mut chars = inner.chars();
            chars.next().filter(|_| chars.next().is_none())
        })
    {
        return Some(c as i128);
    }
    match value {
        "true" => return Some(1),
        "false" => return Some(0),
        _ => {}
    }
    let (negative, digits) = match value.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, value),
    };
    let digits = digits.replace('_', "");
    let magnitude = if let Some(hex) = digits
        .strip_prefix("0x")
        .or_else(|| digits.strip_prefix("0X"))
    {
        i128::from_str_radix(hex, 16).ok()?
    } else if let Some(binary) = digits
        .strip_prefix("0b")
        .or_else(|| digits.strip_prefix("0B"))
    {
        i128::from_str_radix(binary, 2).ok()?
    } else {
        digits.parse::<i128>().ok()?
    };
    Some(if negative { -magnitude } else { magnitude })
}

fn compare(actual: &str, expected: &str) -> Ordering {
    let actual = actual.trim();
    let expected = expected.trim();
    let first_word = actual.split_whitespace().next().unwrap_or(actual);
    if let (Some(a), Some(e)) = (
        parse_integer(actual).or_else(|| parse_integer(first_word)),
        parse_integer(expected),
    ) {
        return a.cmp(&e);
    }
    if let (Ok(a), Ok(e)) = (first_word.parse::<f64>(), expected.parse::<f64>())
        && let Some(ordering) = a.partial_cmp(&e)
    {
        return ordering;
    }
    let unquote = |text: &str| -> String {
        text.strip_prefix('"')
            .and_then(|rest| rest.strip_suffix('"'))
            .unwrap_or(text)
            .to_string()
    };
    unquote(actual).cmp(&unquote(expected))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comparisons() {
        assert!(Op::Eq.holds("3", "3"));
        assert!(Op::Eq.holds("0x0000002A", "42"));
        assert!(Op::Eq.holds("97 'a'", "'a'"));
        assert!(Op::Lt.holds("3", "4"));
        assert!(Op::Ge.holds("-1", "-1"));
        assert!(Op::Gt.holds("2.5", "2.25"));
        assert!(Op::Eq.holds("Running", "Running"));
        assert!(Op::Ne.holds("Idle", "Running"));
        assert!(Op::Eq.holds("true", "1"));
    }

    #[test]
    fn splitting() {
        assert_eq!(
            split_comparison("k == 3").unwrap(),
            ("k".to_string(), Op::Eq, "3".to_string())
        );
        assert_eq!(split_comparison("v<=4").unwrap().1, Op::Le);
        assert!(split_comparison("k").is_err());
        assert!(split_comparison("== 3").is_err());
        assert!(split_comparison("k ==").is_err());
        assert!(split_comparison("a b == 3").is_err());
    }
}
