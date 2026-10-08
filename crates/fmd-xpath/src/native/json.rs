//! JSON text to items, with the options `TXQueryEngine.Create` gives its parser:
//! several top-level values, liberal syntax (unquoted keys, single-quoted strings, `INF`,
//! `NaN`), trailing commas, and JSONiq number types (internettools data/xquery.pas:8408-8413,
//! data/fastjsonreader.pas:90-200).

use std::rc::Rc;

use rust_decimal::Decimal;

use super::value::{Item, Object, Seq, XResult, err, parse_decimal};

/// Parses JSON text into a sequence of its top-level values (empty text gives none).
pub(crate) fn parse(text: &str) -> XResult<Seq> {
    let mut parser = Json {
        chars: text.chars().collect(),
        pos: 0,
        depth: 0,
    };
    let mut items = Vec::new();
    loop {
        parser.skip_space();
        if parser.at_end() {
            return Ok(items);
        }
        items.push(parser.value()?);
    }
}

struct Json {
    chars: Vec<char>,
    pos: usize,
    /// Open objects and arrays, bounded so hostile input can't exhaust the stack.
    depth: usize,
}

/// The deepest nesting accepted.
const MAX_DEPTH: usize = 512;

impl Json {
    fn at_end(&self) -> bool {
        self.pos >= self.chars.len()
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn skip_space(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.pos += 1;
        }
    }

    fn value(&mut self) -> XResult<Item> {
        self.skip_space();
        match self.peek() {
            Some(c @ ('{' | '[')) => {
                if self.depth >= MAX_DEPTH {
                    return err("jerr:JNDY0021: nested too deeply");
                }
                self.depth += 1;
                let value = if c == '{' {
                    self.object()
                } else {
                    self.array()
                };
                self.depth -= 1;
                value
            }
            Some(q @ ('"' | '\'')) => Ok(Item::str(self.string(q)?)),
            Some(c) if c == '-' || c.is_ascii_digit() => self.number(),
            Some(c) if c.is_alphabetic() => {
                let word = self.identifier();
                match word.as_str() {
                    "true" => Ok(Item::Bool(true)),
                    "false" => Ok(Item::Bool(false)),
                    "null" => Ok(Item::Null),
                    w if w.eq_ignore_ascii_case("inf") || w.eq_ignore_ascii_case("infinity") => {
                        Ok(Item::Dbl(f64::INFINITY))
                    }
                    w if w.eq_ignore_ascii_case("nan") => Ok(Item::Dbl(f64::NAN)),
                    _ => err(format!("jerr:JNDY0021: unexpected {word:?}")),
                }
            }
            _ => err("jerr:JNDY0021: unexpected character"),
        }
    }

    fn identifier(&mut self) -> String {
        let start = self.pos;
        while self
            .peek()
            .is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '$')
        {
            self.pos += 1;
        }
        self.chars[start..self.pos].iter().collect()
    }

    fn object(&mut self) -> XResult<Item> {
        self.pos += 1;
        let mut object = Object::new();
        loop {
            self.skip_space();
            match self.peek() {
                Some('}') => {
                    self.pos += 1;
                    return Ok(Item::Object(Rc::new(object)));
                }
                Some(q @ ('"' | '\'')) => {
                    let key = self.string(q)?;
                    self.entry(&mut object, key)?;
                }
                Some(c) if c.is_alphanumeric() || c == '_' || c == '$' => {
                    let key = self.identifier();
                    self.entry(&mut object, key)?;
                }
                _ => return err("jerr:JNDY0021: expected a property name"),
            }
            self.skip_space();
            match self.peek() {
                Some(',') => self.pos += 1,
                Some('}') => {}
                _ => return err("jerr:JNDY0021: expected , or }"),
            }
        }
    }

    fn entry(&mut self, object: &mut Object, key: String) -> XResult<()> {
        self.skip_space();
        if self.peek() != Some(':') {
            return err("jerr:JNDY0021: expected : after a property name");
        }
        self.pos += 1;
        let value = self.value()?;
        object.insert(key, vec![value]);
        Ok(())
    }

    fn array(&mut self) -> XResult<Item> {
        self.pos += 1;
        let mut members = Vec::new();
        loop {
            self.skip_space();
            if self.peek() == Some(']') {
                self.pos += 1;
                return Ok(Item::Array(Rc::new(members)));
            }
            members.push(vec![self.value()?]);
            self.skip_space();
            match self.peek() {
                Some(',') => self.pos += 1,
                Some(']') => {}
                _ => return err("jerr:JNDY0021: expected , or ]"),
            }
        }
    }

    fn string(&mut self, quote: char) -> XResult<String> {
        self.pos += 1;
        let mut s = String::new();
        loop {
            let Some(c) = self.peek() else {
                return err("jerr:JNDY0021: unterminated string");
            };
            self.pos += 1;
            match c {
                c if c == quote => return Ok(s),
                '\\' => {
                    let Some(e) = self.peek() else {
                        return err("jerr:JNDY0021: unterminated escape");
                    };
                    self.pos += 1;
                    match e {
                        'n' => s.push('\n'),
                        't' => s.push('\t'),
                        'r' => s.push('\r'),
                        'b' => s.push('\u{8}'),
                        'f' => s.push('\u{c}'),
                        'u' => {
                            let unit = self.hex4()?;
                            if (0xD800..0xDC00).contains(&unit)
                                && self.peek() == Some('\\')
                                && self.chars.get(self.pos + 1) == Some(&'u')
                            {
                                self.pos += 2;
                                let low = self.hex4()?;
                                let code = 0x10000
                                    + ((unit - 0xD800) << 10)
                                    + (low.wrapping_sub(0xDC00) & 0x3FF);
                                s.push(char::from_u32(code).unwrap_or('\u{FFFD}'));
                            } else {
                                s.push(char::from_u32(unit).unwrap_or('\u{FFFD}'));
                            }
                        }
                        other => s.push(other),
                    }
                }
                c => s.push(c),
            }
        }
    }

    fn hex4(&mut self) -> XResult<u32> {
        let digits: String = self.chars.iter().skip(self.pos).take(4).collect();
        match u32::from_str_radix(&digits, 16) {
            Ok(unit) if digits.len() == 4 => {
                self.pos += 4;
                Ok(unit)
            }
            _ => err("jerr:JNDY0021: bad \\u escape"),
        }
    }

    /// A number: an integer, a decimal, or (with an exponent) a double, as JSONiq types
    /// them. Integers too large for 64 bits become decimals.
    fn number(&mut self) -> XResult<Item> {
        let start = self.pos;
        if self.peek() == Some('-') {
            self.pos += 1;
        }
        let digits = |p: &mut Self| {
            while p.peek().is_some_and(|c| c.is_ascii_digit()) {
                p.pos += 1;
            }
        };
        digits(self);
        let mut decimal = false;
        if self.peek() == Some('.') {
            decimal = true;
            self.pos += 1;
            digits(self);
        }
        let mut double = false;
        if matches!(self.peek(), Some('e' | 'E')) {
            double = true;
            self.pos += 1;
            if matches!(self.peek(), Some('+' | '-')) {
                self.pos += 1;
            }
            digits(self);
        }
        let text: String = self.chars[start..self.pos].iter().collect();
        if double {
            return match text.parse::<f64>() {
                Ok(f) => Ok(Item::Dbl(f)),
                Err(_) => err(format!("jerr:JNDY0021: bad number {text}")),
            };
        }
        if !decimal && let Ok(i) = text.parse::<i64>() {
            return Ok(Item::Int(i));
        }
        match parse_decimal(&text) {
            Some(d) => Ok(Item::Dec(d)),
            None => match text.parse::<f64>() {
                Ok(f) if !decimal => Ok(Item::Dec(Decimal::from_f64_retain(f).unwrap_or_default())),
                _ => err(format!("jerr:JNDY0021: bad number {text}")),
            },
        }
    }
}
