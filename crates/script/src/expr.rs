//! The expressions of `let`, `if`, `while`, `repeat` and `check`: integers as C writes them
//! (`42`, `0x2A`, `0b101`, `'a'`, `true`), text in double quotes, the script's variables by
//! name, and C's operators with C's precedence. `&&` and `||` stop early as in C.
//!
//! Comparisons follow [`Op`]: as integers when both sides read as one, else as numbers, else as
//! text; so `state == "Running"` and `ratio > 2.5` work on values read from the target.

use std::fmt;

use crate::compare::{Op, parse_integer};

/// A value: what a variable holds and what an expression gives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Int(i128),
    Text(String),
}

impl Value {
    /// The value of a variable's text: an integer when it reads as one.
    pub fn from_text(text: &str) -> Value {
        match parse_integer(text) {
            Some(n) => Value::Int(n),
            None => Value::Text(text.to_string()),
        }
    }

    /// Whether the value holds as a condition: an integer other than 0.
    pub fn truth(&self) -> Result<bool, String> {
        match self {
            Value::Int(n) => Ok(*n != 0),
            Value::Text(text) => Err(format!("\"{text}\" is text, not a condition")),
        }
    }

    fn int(&self, op: &str) -> Result<i128, String> {
        match self {
            Value::Int(n) => Ok(*n),
            Value::Text(text) => Err(format!("`{op}` needs integers, not \"{text}\"")),
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Int(n) => write!(f, "{n}"),
            Value::Text(text) => f.write_str(text),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Value(Value),
    Name(String),
    Unary(&'static str, Box<Expr>),
    Binary(&'static str, Box<Expr>, Box<Expr>),
}

/// The binary operators, loosest first (C's order).
const LEVELS: &[&[&str]] = &[
    &["||"],
    &["&&"],
    &["|"],
    &["^"],
    &["&"],
    &["==", "!="],
    &["<", "<=", ">", ">="],
    &["<<", ">>"],
    &["+", "-"],
    &["*", "/", "%"],
];

/// Every operator, longest first, so that `<=` is not read as `<`.
const OPERATORS: &[&str] = &[
    "||", "&&", "==", "!=", "<=", ">=", "<<", ">>", "<", ">", "|", "^", "&", "+", "-", "*", "/",
    "%", "!", "~",
];

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Value(Value),
    Name(String),
    Op(&'static str),
    Open,
    Close,
}

fn tokens(text: &str) -> Result<Vec<Token>, String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        if c.is_whitespace() {
            rest = &rest[c.len_utf8()..];
            continue;
        }
        if c == '(' || c == ')' {
            out.push(if c == '(' { Token::Open } else { Token::Close });
            rest = &rest[1..];
        } else if c == '"' {
            let mut value = String::new();
            let mut chars = rest[1..].char_indices();
            let mut end = None;
            while let Some((at, c)) = chars.next() {
                match c {
                    '\\' => match chars.next() {
                        Some((_, c)) => value.push(c),
                        None => break,
                    },
                    '"' => {
                        end = Some(at + 2);
                        break;
                    }
                    c => value.push(c),
                }
            }
            let end = end.ok_or_else(|| format!("`{rest}` has no closing double quote"))?;
            out.push(Token::Value(Value::Text(value)));
            rest = &rest[end..];
        } else if c == '\'' {
            let mut chars = rest[1..].chars();
            match (chars.next(), chars.next()) {
                (Some(c), Some('\'')) => {
                    out.push(Token::Value(Value::Int(c as i128)));
                    rest = &rest[2 + c.len_utf8()..];
                }
                _ => return Err(format!("`{rest}`: a character is written 'a'")),
            }
        } else if c.is_ascii_alphanumeric() || c == '_' {
            let word_end = |text: &str| {
                text.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                    .unwrap_or(text.len())
            };
            let mut end = word_end(rest);
            // A decimal fraction (`2.5`): compared as a number, no arithmetic.
            let fraction = c.is_ascii_digit()
                && rest[end..].starts_with('.')
                && rest[end + 1..].starts_with(|c: char| c.is_ascii_digit());
            if fraction {
                end += 1 + word_end(&rest[end + 1..]);
            }
            let word = &rest[..end];
            if fraction {
                word.parse::<f64>()
                    .map_err(|_| format!("`{word}` is not a number"))?;
                out.push(Token::Value(Value::Text(word.to_string())));
            } else if c.is_ascii_digit() {
                let n = parse_integer(word).ok_or_else(|| format!("`{word}` is not a number"))?;
                out.push(Token::Value(Value::Int(n)));
            } else {
                out.push(Token::Name(word.to_string()));
            }
            rest = &rest[end..];
        } else {
            let op = OPERATORS
                .iter()
                .find(|op| rest.starts_with(**op))
                .ok_or_else(|| format!("`{c}` is not an operator"))?;
            out.push(Token::Op(op));
            rest = &rest[op.len()..];
        }
    }
    Ok(out)
}

struct Parser {
    tokens: Vec<Token>,
    at: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.at)
    }

    fn binary(&mut self, level: usize) -> Result<Expr, String> {
        let Some(ops) = LEVELS.get(level) else {
            return self.unary();
        };
        let mut left = self.binary(level + 1)?;
        while let Some(Token::Op(op)) = self.peek()
            && ops.contains(op)
        {
            let op = *op;
            self.at += 1;
            let right = self.binary(level + 1)?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn unary(&mut self) -> Result<Expr, String> {
        match self.peek().cloned() {
            Some(Token::Op(op @ ("-" | "!" | "~"))) => {
                self.at += 1;
                Ok(Expr::Unary(op, Box::new(self.unary()?)))
            }
            Some(Token::Value(value)) => {
                self.at += 1;
                Ok(Expr::Value(value))
            }
            Some(Token::Name(name)) => {
                self.at += 1;
                Ok(Expr::Name(name))
            }
            Some(Token::Open) => {
                self.at += 1;
                let inner = self.binary(0)?;
                if self.peek() != Some(&Token::Close) {
                    return Err("a `(` without its `)`".into());
                }
                self.at += 1;
                Ok(inner)
            }
            Some(Token::Op(op)) => Err(format!("`{op}` needs a value before it")),
            Some(Token::Close) => Err("a `)` without its `(`".into()),
            None => Err("a value is missing at the end".into()),
        }
    }
}

/// Reads an expression.
pub fn parse(text: &str) -> Result<Expr, String> {
    let mut parser = Parser {
        tokens: tokens(text)?,
        at: 0,
    };
    if parser.tokens.is_empty() {
        return Err("an expression is missing".into());
    }
    let expr = parser.binary(0)?;
    match parser.peek() {
        None => Ok(expr),
        Some(Token::Close) => Err("a `)` without its `(`".into()),
        Some(_) => Err(format!(
            "`{text}`: an operator is missing between two values"
        )),
    }
}

impl Expr {
    /// The variables the expression reads, each once, in order.
    pub fn names(&self) -> Vec<&str> {
        let mut out = Vec::new();
        self.collect_names(&mut out);
        out
    }

    fn collect_names<'a>(&'a self, out: &mut Vec<&'a str>) {
        match self {
            Expr::Value(_) => {}
            Expr::Name(name) => {
                if !out.contains(&name.as_str()) {
                    out.push(name);
                }
            }
            Expr::Unary(_, inner) => inner.collect_names(out),
            Expr::Binary(_, left, right) => {
                left.collect_names(out);
                right.collect_names(out);
            }
        }
    }

    /// The value, with `lookup` giving the variables' values.
    pub fn eval(&self, lookup: &dyn Fn(&str) -> Option<String>) -> Result<Value, String> {
        match self {
            Expr::Value(value) => Ok(value.clone()),
            Expr::Name(name) => match lookup(name) {
                Some(text) => Ok(Value::from_text(&text)),
                None => match name.as_str() {
                    "true" => Ok(Value::Int(1)),
                    "false" => Ok(Value::Int(0)),
                    _ => Err(format!(
                        "`{name}` has no value yet (`let {name} = …` first)"
                    )),
                },
            },
            Expr::Unary(op, inner) => {
                let value = inner.eval(lookup)?;
                let n = match *op {
                    "!" => return Ok(Value::Int(i128::from(!value.truth()?))),
                    "-" => value.int(op)?.checked_neg(),
                    _ => Some(!value.int(op)?),
                };
                n.map(Value::Int)
                    .ok_or_else(|| "the value is too large".into())
            }
            Expr::Binary(op @ ("&&" | "||"), left, right) => {
                let left = left.eval(lookup)?.truth()?;
                // C stops at the first side that decides.
                if left == (*op == "||") {
                    return Ok(Value::Int(i128::from(left)));
                }
                Ok(Value::Int(i128::from(right.eval(lookup)?.truth()?)))
            }
            Expr::Binary(op, left, right) => {
                let left = left.eval(lookup)?;
                let right = right.eval(lookup)?;
                if let Some(compare) = Op::parse(op) {
                    let holds = compare.holds(&left.to_string(), &right.to_string());
                    return Ok(Value::Int(i128::from(holds)));
                }
                let (a, b) = (left.int(op)?, right.int(op)?);
                let shift = || u32::try_from(b).ok().filter(|b| *b < 128);
                let n = match *op {
                    "+" => a.checked_add(b),
                    "-" => a.checked_sub(b),
                    "*" => a.checked_mul(b),
                    "/" | "%" if b == 0 => return Err(format!("`{op}` by 0")),
                    "/" => a.checked_div(b),
                    "%" => a.checked_rem(b),
                    "&" => Some(a & b),
                    "|" => Some(a | b),
                    "^" => Some(a ^ b),
                    "<<" | ">>" => {
                        let b = shift().ok_or_else(|| format!("`{op} {b}`: shift by 0 to 127"))?;
                        if *op == "<<" {
                            a.checked_shl(b)
                        } else {
                            a.checked_shr(b)
                        }
                    }
                    _ => return Err(format!("`{op}` is not an operator")),
                };
                n.map(Value::Int)
                    .ok_or_else(|| "the value is too large".into())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn eval_with(text: &str, vars: &[(&str, &str)]) -> Result<Value, String> {
        let vars: HashMap<String, String> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        parse(text)?.eval(&|name| vars.get(name).cloned())
    }

    fn eval(text: &str) -> Value {
        eval_with(text, &[]).unwrap()
    }

    #[test]
    fn arithmetic_as_c() {
        assert_eq!(eval("1 + 2 * 3"), Value::Int(7));
        assert_eq!(eval("(1 + 2) * 3"), Value::Int(9));
        assert_eq!(eval("0x10 | 0b1 << 2"), Value::Int(0x14));
        assert_eq!(eval("-7 / 2"), Value::Int(-3));
        assert_eq!(eval("7 % 3 - ~0"), Value::Int(2));
        assert_eq!(eval("'a' + 1"), Value::Int(98));
        // As in C, `&` is looser than `==`.
        assert_eq!(eval("6 & 2 == 2"), Value::Int(0));
    }

    #[test]
    fn conditions() {
        assert_eq!(eval("3 < 4 && 4 <= 4"), Value::Int(1));
        assert_eq!(eval("!(1 > 2) || x"), Value::Int(1));
        assert_eq!(eval("0 && x"), Value::Int(0));
        let vars = [("state", "Running"), ("v", "0x0000002A"), ("r", "2.5")];
        assert_eq!(
            eval_with("state == \"Running\" && v == 42 && r > 2.25", &vars),
            Ok(Value::Int(1))
        );
        assert_eq!(eval_with("v + 1", &vars), Ok(Value::Int(43)));
    }

    #[test]
    fn mistakes() {
        assert!(parse("1 +").is_err());
        assert!(parse("(1").is_err());
        assert!(parse("1)").is_err());
        assert!(parse("1 2").is_err());
        assert!(parse("1 $ 2").is_err());
        assert!(parse("\"open").is_err());
        assert!(parse("").is_err());
        assert!(eval_with("1 / 0", &[]).is_err());
        assert!(eval_with("n + 1", &[]).unwrap_err().contains("let n"));
        assert!(eval_with("s + 1", &[("s", "Idle")]).is_err());
        assert!(eval_with("1 << 200", &[]).is_err());
    }

    #[test]
    fn names_once() {
        assert_eq!(parse("a + b * a < c").unwrap().names(), ["a", "b", "c"]);
    }
}
