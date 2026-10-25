//! ORTI (OSEK/AUTOSAR Run Time Interface) files and the OS view built on them.
//!
//! An AUTOSAR or OSEK system builder writes an ORTI file (`.ort`, `.orti`) next to the program: it
//! declares the kinds of OS objects (OS, TASK, ISR, RESOURCE, …) and their attributes, then lists
//! the objects with a C expression per attribute that the debugger reads to show the object's
//! state. This module reads ORTI 2.1 and 2.2 ([`Orti::parse`]) and keeps the values read for its
//! expressions ([`OsView`]).
//!
//! - `VERSION { KOIL = "2.2"; OSSEMANTICS = "ORTI", "2.2"; };`
//! - `IMPLEMENTATION name { OS { ENUM "unsigned char" [ "NO_TASK" = 0xFF, "TaskA" : TaskA = 0 ]
//!   RUNNINGTASK, "Running task"; CTYPE "unsigned int" vs_TICKS; STRING vs_NOTE; }, "OS"; … };`
//! - `TASK TaskA { STATE = "OsTaskState[0]"; vs_NOTE = "text"; };`
//!
//! What it does not know (a statement of another form, an attribute without a declaration) it
//! skips with a warning: the rest of the file is still shown.

use std::collections::{HashMap, HashSet};

use crate::path_expr;
use crate::protocol::LiveWatchValue;

/// An ENUM item: the name shown for a value, and the object it names, if any.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnumItem {
    pub name: String,
    pub object: Option<String>,
    /// The value, when it is a number.
    pub value: Option<i128>,
    /// The value, when it is an expression read from the target (`"&OsTcb[0]"`: the item names
    /// the attribute's value that equals it).
    pub expression: Option<String>,
}

/// The kind of an attribute.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AttrKind {
    /// A value of the C type.
    CType(Option<String>),
    /// A value shown by the name of its item.
    Enum {
        ctype: Option<String>,
        items: Vec<EnumItem>,
    },
    /// Text written in the ORTI file itself.
    String,
}

/// An attribute declaration of an object kind.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttrDecl {
    pub name: String,
    pub kind: AttrKind,
    pub description: Option<String>,
}

/// A kind of OS object and its attributes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectType {
    /// `OS`, `TASK`, `ISR`, …
    pub name: String,
    pub description: Option<String>,
    pub attrs: Vec<AttrDecl>,
}

/// An OS object and the value of each attribute: the expression, or the text of a STRING.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Object {
    pub kind: String,
    pub name: String,
    pub values: Vec<(String, String)>,
}

/// An ORTI file, read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Orti {
    /// The ORTI version (`KOIL`).
    pub version: Option<String>,
    pub implementation: Option<String>,
    pub types: Vec<ObjectType>,
    pub objects: Vec<Object>,
    /// What was skipped, with its line.
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Token {
    Ident(String),
    Number(i128),
    Str(String),
    Punct(char),
}

/// The tokens of `text`, each with its line.
fn tokenize(text: &str) -> Result<Vec<(Token, usize)>, String> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    let mut line = 1;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\n' => {
                line += 1;
                i += 1;
            }
            c if c.is_whitespace() => i += 1,
            '/' if chars.get(i + 1) == Some(&'/') => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            '/' if chars.get(i + 1) == Some(&'*') => {
                let start = line;
                i += 2;
                loop {
                    match chars.get(i) {
                        None => return Err(format!("line {start}: a comment without its end")),
                        Some('*') if chars.get(i + 1) == Some(&'/') => {
                            i += 2;
                            break;
                        }
                        Some('\n') => {
                            line += 1;
                            i += 1;
                        }
                        Some(_) => i += 1,
                    }
                }
            }
            '"' => {
                let start = line;
                let mut text = String::new();
                i += 1;
                loop {
                    match chars.get(i) {
                        None => return Err(format!("line {start}: a string without its end")),
                        Some('\\') => {
                            if let Some(&next) = chars.get(i + 1) {
                                text.push(next);
                            }
                            i += 2;
                        }
                        Some('"') => {
                            i += 1;
                            break;
                        }
                        Some(&c) => {
                            if c == '\n' {
                                line += 1;
                            }
                            text.push(c);
                            i += 1;
                        }
                    }
                }
                tokens.push((Token::Str(text), start));
            }
            c if c.is_ascii_digit() => {
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                let word: String = chars[start..i].iter().collect();
                let value = match word.strip_prefix("0x").or_else(|| word.strip_prefix("0X")) {
                    Some(hex) => i128::from_str_radix(hex, 16),
                    None => word.parse(),
                }
                .map_err(|_| format!("line {line}: `{word}` is not a number"))?;
                tokens.push((Token::Number(value), line));
            }
            c if c.is_ascii_alphabetic() || c == '_' => {
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                tokens.push((Token::Ident(chars[start..i].iter().collect()), line));
            }
            '-' if chars.get(i + 1).is_some_and(char::is_ascii_digit) => {
                // A negative number: read the digits as one.
                i += 1;
                let start = i;
                while i < chars.len() && chars[i].is_ascii_alphanumeric() {
                    i += 1;
                }
                let word: String = chars[start..i].iter().collect();
                let value: i128 = word
                    .parse()
                    .map_err(|_| format!("line {line}: `-{word}` is not a number"))?;
                tokens.push((Token::Number(-value), line));
            }
            c => {
                tokens.push((Token::Punct(c), line));
                i += 1;
            }
        }
    }
    Ok(tokens)
}

struct Parser {
    tokens: Vec<(Token, usize)>,
    at: usize,
    warnings: Vec<String>,
}

type Step<T> = Result<T, String>;

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.at).map(|(t, _)| t)
    }

    fn line(&self) -> usize {
        self.tokens
            .get(self.at)
            .or_else(|| self.tokens.last())
            .map_or(0, |(_, line)| *line)
    }

    fn next(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.at).map(|(t, _)| t.clone());
        self.at += 1;
        token
    }

    fn error<T>(&self, what: &str) -> Step<T> {
        let found = match self.peek() {
            Some(Token::Ident(word)) => format!("`{word}`"),
            Some(Token::Number(n)) => format!("{n}"),
            Some(Token::Str(text)) => format!("\"{text}\""),
            Some(Token::Punct(c)) => format!("`{c}`"),
            None => "the end of the file".to_string(),
        };
        Err(format!("line {}: {what}, found {found}", self.line()))
    }

    fn punct(&mut self, c: char) -> Step<()> {
        if self.peek() == Some(&Token::Punct(c)) {
            self.at += 1;
            Ok(())
        } else {
            self.error(&format!("`{c}` expected"))
        }
    }

    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(&Token::Punct(c)) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn ident(&mut self) -> Step<String> {
        match self.peek() {
            Some(Token::Ident(word)) => {
                let word = word.clone();
                self.at += 1;
                Ok(word)
            }
            _ => self.error("a name expected"),
        }
    }

    fn string(&mut self) -> Step<String> {
        match self.peek() {
            Some(Token::Str(text)) => {
                let text = text.clone();
                self.at += 1;
                Ok(text)
            }
            _ => self.error("a string expected"),
        }
    }

    fn optional_string(&mut self) -> Option<String> {
        match self.peek() {
            Some(Token::Str(text)) => {
                let text = text.clone();
                self.at += 1;
                Some(text)
            }
            _ => None,
        }
    }

    /// Skips to the end of the statement: past the next `;` at this depth, or to the `}` that
    /// closes the block (left for the caller).
    fn skip_statement(&mut self) {
        let mut depth = 0i32;
        while let Some(token) = self.peek() {
            match token {
                Token::Punct('{') | Token::Punct('[') => depth += 1,
                Token::Punct('}') | Token::Punct(']') => {
                    if depth == 0 {
                        return;
                    }
                    depth -= 1;
                }
                Token::Punct(';') if depth == 0 => {
                    self.at += 1;
                    return;
                }
                _ => {}
            }
            self.at += 1;
        }
    }

    /// Runs `statement`; when it fails, notes why and skips the statement.
    fn recover(&mut self, statement: impl FnOnce(&mut Self) -> Step<()>) {
        let start = self.at;
        if let Err(error) = statement(self) {
            self.warnings.push(error);
            if self.at == start {
                self.at += 1;
            }
            self.at = self.at.max(start);
            self.skip_statement();
        }
    }

    fn version(&mut self, orti: &mut Orti) -> Step<()> {
        self.punct('{')?;
        while !self.eat('}') {
            if self.peek().is_none() {
                return self.error("`}` expected");
            }
            self.recover(|p| {
                let name = p.ident()?;
                p.punct('=')?;
                let value = p.string()?;
                while p.eat(',') {
                    p.string()?;
                }
                p.punct(';')?;
                if name == "KOIL" {
                    orti.version = Some(value);
                }
                Ok(())
            });
        }
        self.punct(';')
    }

    fn enum_items(&mut self) -> Step<Vec<EnumItem>> {
        let mut items = Vec::new();
        if !self.eat('[') {
            return Ok(items);
        }
        loop {
            if self.eat(']') {
                break;
            }
            let name = self.string()?;
            let object = if self.eat(':') {
                Some(self.ident()?)
            } else {
                None
            };
            self.punct('=')?;
            let (value, expression) = match self.next() {
                Some(Token::Number(n)) => (Some(n), None),
                Some(Token::Str(text)) => (None, Some(text)),
                _ => {
                    self.at -= 1;
                    return self.error("an ENUM value expected");
                }
            };
            items.push(EnumItem {
                name,
                object,
                value,
                expression,
            });
            if !self.eat(',') {
                self.punct(']')?;
                break;
            }
        }
        Ok(items)
    }

    fn attr_decl(&mut self) -> Step<AttrDecl> {
        let mut keyword = self.ident()?;
        if keyword == "TOTRACE" {
            keyword = self.ident()?;
        }
        let kind = match keyword.as_str() {
            "CTYPE" => AttrKind::CType(self.optional_string()),
            "ENUM" => {
                let ctype = self.optional_string();
                AttrKind::Enum {
                    ctype,
                    items: self.enum_items()?,
                }
            }
            "STRING" => AttrKind::String,
            other => {
                self.at -= 1;
                return Err(format!(
                    "line {}: `{other}` is not CTYPE, ENUM or STRING",
                    self.line()
                ));
            }
        };
        let name = self.ident()?;
        // An attribute of several values (`STACK[]`): read as one.
        if self.eat('[') {
            self.punct(']')?;
        }
        let description = if self.eat(',') {
            Some(self.string()?)
        } else {
            None
        };
        self.punct(';')?;
        Ok(AttrDecl {
            name,
            kind,
            description,
        })
    }

    fn implementation(&mut self, orti: &mut Orti) -> Step<()> {
        orti.implementation = Some(self.ident()?);
        self.punct('{')?;
        while !self.eat('}') {
            if self.peek().is_none() {
                return self.error("`}` expected");
            }
            let mut ty = None;
            self.recover(|p| {
                let name = p.ident()?;
                p.punct('{')?;
                let mut attrs = Vec::new();
                while !p.eat('}') {
                    if p.peek().is_none() {
                        return p.error("`}` expected");
                    }
                    let mut decl = None;
                    p.recover(|p| {
                        decl = Some(p.attr_decl()?);
                        Ok(())
                    });
                    attrs.extend(decl);
                }
                let description = if p.eat(',') { Some(p.string()?) } else { None };
                p.punct(';')?;
                ty = Some(ObjectType {
                    name,
                    description,
                    attrs,
                });
                Ok(())
            });
            orti.types.extend(ty);
        }
        self.punct(';')
    }

    fn object(&mut self, kind: String) -> Step<Object> {
        let name = self.ident()?;
        self.punct('{')?;
        let mut values = Vec::new();
        while !self.eat('}') {
            if self.peek().is_none() {
                return self.error("`}` expected");
            }
            let mut value = None;
            self.recover(|p| {
                let mut attr = p.ident()?;
                if p.eat('[') {
                    let index = match p.next() {
                        Some(Token::Number(n)) => n,
                        _ => {
                            p.at -= 1;
                            return p.error("an index expected");
                        }
                    };
                    p.punct(']')?;
                    attr = format!("{attr}[{index}]");
                }
                p.punct('=')?;
                let text = p.string()?;
                p.punct(';')?;
                value = Some((attr, text));
                Ok(())
            });
            values.extend(value);
        }
        self.punct(';')?;
        Ok(Object { kind, name, values })
    }
}

impl Orti {
    /// Reads an ORTI file. Only a file that cannot be read at all (an unclosed string or comment,
    /// a bad number) is an error; anything else it does not know is a warning.
    pub fn parse(text: &str) -> Result<Orti, String> {
        let mut parser = Parser {
            tokens: tokenize(text)?,
            at: 0,
            warnings: Vec::new(),
        };
        let mut orti = Orti::default();
        while parser.peek().is_some() {
            let mut object = None;
            let mut statement = |p: &mut Parser| -> Step<()> {
                let keyword = p.ident()?;
                match keyword.as_str() {
                    "VERSION" => p.version(&mut orti),
                    "IMPLEMENTATION" => p.implementation(&mut orti),
                    kind => {
                        object = Some(p.object(kind.to_string())?);
                        Ok(())
                    }
                }
            };
            let start = parser.at;
            if let Err(error) = statement(&mut parser) {
                parser.warnings.push(error);
                parser.at = parser.at.max(start + 1);
                parser.skip_statement();
            }
            orti.objects.extend(object);
        }
        // Objects of a kind the IMPLEMENTATION does not declare, attributes it does not declare.
        for object in &orti.objects {
            match orti.object_type(&object.kind) {
                None => parser.warnings.push(format!(
                    "{} {}: the kind {} is not declared",
                    object.kind, object.name, object.kind
                )),
                Some(ty) => {
                    for (attr, _) in &object.values {
                        let base = attr.split('[').next().unwrap_or(attr);
                        if !ty.attrs.iter().any(|a| a.name == base) {
                            parser.warnings.push(format!(
                                "{} {}: the attribute {attr} is not declared",
                                object.kind, object.name
                            ));
                        }
                    }
                }
            }
        }
        orti.warnings = parser.warnings;
        Ok(orti)
    }

    pub fn object_type(&self, kind: &str) -> Option<&ObjectType> {
        self.types.iter().find(|t| t.name == kind)
    }

    pub fn decl(&self, kind: &str, attr: &str) -> Option<&AttrDecl> {
        let base = attr.split('[').next().unwrap_or(attr);
        self.object_type(kind)?
            .attrs
            .iter()
            .find(|a| a.name == base)
    }

    /// The expressions to read: those of the CTYPE and ENUM attributes that [`path_expr`] can
    /// read, and the expression values of the ENUM items of those attributes, each once.
    pub fn expressions(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut add = |value: &String| {
            if path_expr::is_supported(value) && !out.contains(value) {
                out.push(value.clone());
            }
        };
        for object in &self.objects {
            for (attr, value) in &object.values {
                let Some(decl) = self.decl(&object.kind, attr) else {
                    continue;
                };
                match &decl.kind {
                    AttrKind::String => continue,
                    AttrKind::CType(_) => {}
                    AttrKind::Enum { items, .. } => {
                        for expression in items.iter().filter_map(|i| i.expression.as_ref()) {
                            add(expression);
                        }
                    }
                }
                add(value);
            }
        }
        out
    }
}

/// `value` as an integer, as the debugger shows numbers and characters: decimal, `0x`, `'a'`,
/// `'\u{ff}'`, `true`/`false`, maybe followed by more (`97 'a'`).
fn integer_of(value: &str) -> Option<i128> {
    let value = value.trim();
    if let Some(inner) = value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')) {
        let code = match inner {
            "\\0" => Some(0),
            "\\n" => Some(10),
            "\\r" => Some(13),
            "\\t" => Some(9),
            _ => {
                if let Some(hex) = inner.strip_prefix("\\u{").and_then(|v| v.strip_suffix('}')) {
                    u32::from_str_radix(hex, 16).ok()
                } else if let Some(hex) = inner.strip_prefix("\\x") {
                    u32::from_str_radix(hex, 16).ok()
                } else {
                    let mut chars = inner.chars();
                    chars
                        .next()
                        .filter(|_| chars.next().is_none())
                        .map(u32::from)
                }
            }
        };
        return code.map(i128::from);
    }
    match value {
        "true" => return Some(1),
        "false" => return Some(0),
        _ => {}
    }
    let number = |word: &str| match word.strip_prefix("0x").or_else(|| word.strip_prefix("0X")) {
        Some(hex) => i128::from_str_radix(hex, 16).ok(),
        None => word.parse().ok(),
    };
    let word = value.split_whitespace().next()?;
    // A pointer reads as its target's type and the address it holds: `*OsTcb @ 0x70000034`.
    number(word).or_else(|| number(value.rsplit_once(" @ ")?.1.trim()))
}

/// One cell of an OS table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cell {
    pub text: String,
    /// The value differs from the read before.
    pub changed: bool,
    /// The text is why there is no value.
    pub error: bool,
}

/// One object: its name and a cell per attribute of its kind.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableRow {
    pub object: String,
    pub cells: Vec<Cell>,
}

/// The objects of one kind: a column per attribute.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Table {
    pub kind: String,
    pub title: String,
    pub columns: Vec<String>,
    pub rows: Vec<TableRow>,
}

/// The values read for the expressions of an ORTI file, and what changed.
#[derive(Clone, Debug, Default)]
pub struct OsView {
    orti: Orti,
    values: HashMap<String, Result<String, String>>,
    previous: HashMap<String, String>,
    changed: HashSet<String>,
}

impl OsView {
    pub fn new(orti: Orti) -> Self {
        Self {
            orti,
            ..Default::default()
        }
    }

    pub fn orti(&self) -> &Orti {
        &self.orti
    }

    /// The expressions to read (`liveWatch`).
    pub fn expressions(&self) -> Vec<String> {
        self.orti.expressions()
    }

    /// The values read for [`Self::expressions`]; a value that differs from the read before is
    /// marked. A read where nothing changed (the core halted) keeps the marks, so that what the
    /// last change was stays visible.
    pub fn answered(&mut self, values: Vec<LiveWatchValue>) {
        let mut changed = HashSet::new();
        for value in values {
            let read = match value.value {
                Some(text) => Ok(text),
                None => Err(value.error.unwrap_or_else(|| "no value".into())),
            };
            if let Ok(text) = &read {
                if self
                    .previous
                    .get(&value.expression)
                    .is_some_and(|before| before != text)
                {
                    changed.insert(value.expression.clone());
                }
                self.previous.insert(value.expression.clone(), text.clone());
            }
            self.values.insert(value.expression, read);
        }
        if !changed.is_empty() {
            self.changed = changed;
        }
    }

    /// The read failed as a whole (the core cannot be reached, say).
    pub fn failed(&mut self, error: &str) {
        for expression in self.expressions() {
            self.values
                .insert(expression, Err(format!("not read: {error}")));
        }
        self.changed.clear();
    }

    /// What a cell shows for the attribute `attr` of `object`.
    fn cell(&self, object: &Object, decl: &AttrDecl) -> Cell {
        let value = object
            .values
            .iter()
            .find(|(attr, _)| attr == &decl.name || attr.starts_with(&format!("{}[", decl.name)))
            .map(|(_, value)| value);
        let Some(expression) = value else {
            return Cell {
                text: String::new(),
                changed: false,
                error: false,
            };
        };
        if decl.kind == AttrKind::String {
            return Cell {
                text: expression.clone(),
                changed: false,
                error: false,
            };
        }
        if !path_expr::is_supported(expression) {
            return Cell {
                text: format!("not supported: {expression}"),
                changed: false,
                error: true,
            };
        }
        match self.values.get(expression) {
            None => Cell {
                text: "…".into(),
                changed: false,
                error: false,
            },
            Some(Err(error)) => Cell {
                text: error.clone(),
                changed: false,
                error: true,
            },
            Some(Ok(text)) => {
                // An item's value: its number, or what its expression read.
                let item_value = |item: &EnumItem| {
                    item.value
                        .or_else(|| match self.values.get(item.expression.as_ref()?) {
                            Some(Ok(text)) => integer_of(text),
                            _ => None,
                        })
                };
                let shown = match &decl.kind {
                    AttrKind::Enum { items, .. } => integer_of(text)
                        .and_then(|n| items.iter().find(|item| item_value(item) == Some(n)))
                        .map(|item| item.name.clone())
                        .unwrap_or_else(|| {
                            integer_of(text).map_or_else(|| text.clone(), |n| n.to_string())
                        }),
                    _ => integer_of(text).map_or_else(|| text.clone(), |n| n.to_string()),
                };
                Cell {
                    text: shown,
                    changed: self.changed.contains(expression),
                    error: false,
                }
            }
        }
    }

    /// A table per object kind with objects, in the order of the IMPLEMENTATION.
    pub fn tables(&self) -> Vec<Table> {
        self.orti
            .types
            .iter()
            .filter_map(|ty| {
                let objects: Vec<&Object> = self
                    .orti
                    .objects
                    .iter()
                    .filter(|o| o.kind == ty.name)
                    .collect();
                if objects.is_empty() {
                    return None;
                }
                let columns = ty
                    .attrs
                    .iter()
                    .map(|a| a.description.clone().unwrap_or_else(|| a.name.clone()))
                    .collect();
                let rows = objects
                    .iter()
                    .map(|object| TableRow {
                        object: object.name.clone(),
                        cells: ty.attrs.iter().map(|a| self.cell(object, a)).collect(),
                    })
                    .collect();
                Some(Table {
                    kind: ty.name.clone(),
                    title: ty.description.clone().unwrap_or_else(|| ty.name.clone()),
                    columns,
                    rows,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEMO: &str = include_str!("../../../zed/aurix_tcf_demo/orti_demo.ort");

    fn value(expression: &str, value: &str) -> LiveWatchValue {
        LiveWatchValue {
            expression: expression.into(),
            value: Some(value.into()),
            type_: None,
            error: None,
        }
    }

    #[test]
    fn the_demo_file_reads_without_warnings() {
        let orti = Orti::parse(DEMO).unwrap();
        assert_eq!(orti.warnings, Vec::<String>::new());
        assert_eq!(orti.version.as_deref(), Some("2.2"));
        assert_eq!(orti.implementation.as_deref(), Some("orti_demo_ORTI"));
        let kinds: Vec<&str> = orti.types.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(kinds, ["OS", "TASK", "ISR"]);
        let running = orti.decl("OS", "RUNNINGTASK").unwrap();
        match &running.kind {
            AttrKind::Enum { ctype, items } => {
                assert_eq!(ctype.as_deref(), Some("unsigned char"));
                assert_eq!(items[0].value, Some(0xFF));
                assert_eq!(items[1].object.as_deref(), Some("TaskA"));
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(orti.objects.len(), 5);
        assert_eq!(orti.objects[1].name, "TaskA");
        // The pointer path, the cast, and the ENUM items' addresses are read.
        let expressions = orti.expressions();
        for expression in [
            "OsRunningTcb->state",
            "(unsigned)OsTicks / 2",
            "&OsTcb[1]",
            "OsCurrentTask < 3 ? OsTcb[OsCurrentTask].prio : 0",
        ] {
            assert!(
                expressions.contains(&expression.to_string()),
                "{expression}"
            );
        }
        // OsTicks once, for the OS and the ISR.
        assert_eq!(expressions.iter().filter(|e| *e == "OsTicks").count(), 1);
    }

    #[test]
    fn values_show_by_their_enum_names_and_changes_are_marked() {
        let mut view = OsView::new(Orti::parse(DEMO).unwrap());
        view.answered(vec![
            value("OsCurrentTask", "1"),
            value("OsTaskState[0]", "1"),
            value("OsTaskState[1]", "0"),
            value("OsTaskState[2]", "'\\u{3}'"),
            value("OsTcb[0].prio", "3"),
            value("OsTicks", "100"),
            value("(unsigned)OsTicks / 2", "50"),
            value("OsRunningTcb", "*OsTcbType @ 0x70000034"),
            value("&OsTcb[0]", "0x70000020"),
            value("&OsTcb[1]", "0x70000034"),
            value("&OsTcb[2]", "0x70000048"),
        ]);
        let tables = view.tables();
        let os = &tables[0];
        assert_eq!(os.kind, "OS");
        assert_eq!(os.columns[0], "Running task");
        assert_eq!(os.rows[0].cells[0].text, "TaskB");
        assert_eq!(os.rows[0].cells[4].text, "50");
        // A pointer shown by the ENUM item whose address it holds.
        assert_eq!(os.rows[0].cells[5].text, "TCB of TaskB");
        let tasks = &tables[1];
        assert_eq!(tasks.rows[0].cells[0].text, "READY");
        assert_eq!(tasks.rows[1].cells[0].text, "RUNNING");
        assert_eq!(tasks.rows[2].cells[0].text, "SUSPENDED");
        assert_eq!(tasks.rows[0].cells[1].text, "3");
        assert_eq!(tasks.rows[0].cells[3].text, "highest priority");
        assert!(!tasks.rows[1].cells[0].changed);

        view.answered(vec![
            value("OsCurrentTask", "2"),
            value("OsTaskState[1]", "1"),
            value("OsTaskState[2]", "0"),
        ]);
        let tables = view.tables();
        assert_eq!(tables[0].rows[0].cells[0].text, "TaskC");
        assert!(tables[0].rows[0].cells[0].changed);
        assert!(tables[1].rows[1].cells[0].changed);
        assert!(!tables[1].rows[0].cells[0].changed);
        // The same values again (the core halted): the marks stay.
        view.answered(vec![
            value("OsCurrentTask", "2"),
            value("OsTaskState[1]", "1"),
            value("OsTaskState[2]", "0"),
        ]);
        assert!(view.tables()[0].rows[0].cells[0].changed);
        // A value not in the ENUM shows as its number.
        view.answered(vec![value("OsCurrentTask", "7")]);
        assert_eq!(view.tables()[0].rows[0].cells[0].text, "7");
    }

    #[test]
    fn what_it_does_not_know_is_skipped_with_a_warning() {
        let text = r#"
            VERSION { KOIL = "2.1"; };
            IMPLEMENTATION x {
                OS {
                    ENUM [ "A" = 0, "B" = 1 ] RUNNINGTASK, "Running";
                    WHATEVER "x" FOO;
                    CTYPE vs_X;
                };
                TASK { CTYPE PRIORITY; }, "Tasks";
            };
            OS os { RUNNINGTASK = "cur"; vs_X = "x"; vs_Y = "y"; };
            TASK t1 { PRIORITY = 3; };
            TASK t2 { PRIORITY = "p2"; };
            RESOURCE r { };
        "#;
        let orti = Orti::parse(text).unwrap();
        assert_eq!(orti.version.as_deref(), Some("2.1"));
        assert_eq!(orti.types[0].attrs.len(), 2);
        let all = orti.warnings.join("\n");
        assert!(all.contains("WHATEVER"), "{all}");
        assert!(all.contains("vs_Y is not declared"), "{all}");
        assert!(
            all.contains("RESOURCE r: the kind RESOURCE is not declared"),
            "{all}"
        );
        // t1's value is not a string: skipped, the object stays.
        assert_eq!(orti.objects.len(), 4);
        assert!(orti.objects[1].values.is_empty());
        assert_eq!(orti.objects[2].values, [("PRIORITY".into(), "p2".into())]);
        // A comment, broken at the end: an error.
        assert!(Orti::parse("/* never closed").is_err());
        assert!(Orti::parse("OS x { A = \"open").is_err());
    }

    #[test]
    fn characters_and_numbers() {
        assert_eq!(integer_of("97 'a'"), Some(97));
        assert_eq!(integer_of("'\\u{ff}'"), Some(255));
        assert_eq!(integer_of("'\\0'"), Some(0));
        assert_eq!(integer_of("0x2A"), Some(42));
        assert_eq!(integer_of("*OsTcbType @ 0x70000034"), Some(0x7000_0034));
        assert_eq!(integer_of("Idle"), None);
    }

    #[test]
    fn an_expression_the_server_never_reads_is_marked() {
        let text = r#"
            IMPLEMENTATION x { OS { CTYPE vs_A; CTYPE vs_B; }; };
            OS os { vs_A = "GetTaskState(1)"; vs_B = "a = 1"; };
        "#;
        let view = OsView::new(Orti::parse(text).unwrap());
        assert!(view.expressions().is_empty());
        let cells = &view.tables()[0].rows[0].cells;
        assert!(
            cells
                .iter()
                .all(|c| c.error && c.text.starts_with("not supported"))
        );
    }
}
