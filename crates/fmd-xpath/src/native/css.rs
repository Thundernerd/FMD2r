//! CSS 3 selectors, translated into XPath expressions the way internettools'
//! `TXQueryEngine.parseCSSTerm` does (internettools data/xquery.pas:8749-9119).
//!
//! A selector starts at the context node with `descendant-or-self::name` (`descendant::*` for
//! `*`), so a context element matching the first compound selector is part of the result.
//! Classes and attribute matches use the default case-insensitive collation.

use super::syntax::{Axis, BinOp, CmpOp, Expr, NodeTest};
use super::value::{XResult, err};

/// Translates a selector group into an expression.
pub(crate) fn translate(css: &str) -> XResult<Expr> {
    if css.is_empty() {
        return Ok(Expr::Sequence(Vec::new()));
    }
    let mut parser = Css {
        chars: css.chars().collect(),
        pos: 0,
    };
    let mut result: Option<Expr> = None;
    while !parser.at_end() {
        parser.skip_space();
        let sequence = parser.selector()?;
        result = Some(match result {
            None => sequence,
            Some(left) => Expr::Binary(BinOp::Union, Box::new(left), Box::new(sequence)),
        });
        parser.skip_space();
        if parser.at_end() {
            break;
        }
        parser.expect(',')?;
    }
    Ok(result.unwrap_or(Expr::Sequence(Vec::new())))
}

const SPACE: &[char] = &[' ', '\t', '\n', '\u{c}', '\r'];
const TOKEN_END: &[char] = &[
    '=', '~', '^', '$', '*', '|', ':', '(', ')', '#', '>', '+', '[', ']', '.', ',',
];

struct Css {
    chars: Vec<char>,
    pos: usize,
}

fn call(name: &str, args: Vec<Expr>) -> Expr {
    Expr::Call(name.to_owned(), args)
}

fn attribute(name: &str) -> Expr {
    Expr::Step(Axis::Attribute, name_test(name), Vec::new())
}

fn name_test(name: &str) -> NodeTest {
    match name {
        "*" | "*:*" => NodeTest::AnyName,
        _ => match name.strip_prefix("*:") {
            Some(local) => NodeTest::Name(local.to_owned()),
            None => NodeTest::Name(name.to_owned()),
        },
    }
}

fn step(axis: Axis, name: &str) -> Expr {
    Expr::Step(axis, name_test(name), Vec::new())
}

fn path(left: Expr, right: Expr) -> Expr {
    Expr::Path(Box::new(left), Box::new(right))
}

fn equals(left: Expr, right: Expr) -> Expr {
    Expr::Binary(BinOp::General(CmpOp::Eq), Box::new(left), Box::new(right))
}

impl Css {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn at_end(&self) -> bool {
        self.pos >= self.chars.len()
    }

    fn skip_space(&mut self) {
        while self.peek().is_some_and(|c| SPACE.contains(&c)) {
            self.pos += 1;
        }
    }

    fn expect(&mut self, c: char) -> XResult<()> {
        if self.peek() == Some(c) {
            self.pos += 1;
            Ok(())
        } else {
            err(format!("pxp:CSS: `{c}` expected"))
        }
    }

    fn token(&mut self) -> XResult<String> {
        match self.peek() {
            None => Ok(String::new()),
            Some(c) if SPACE.contains(&c) => err("pxp:CSS: unexpected whitespace"),
            Some('*') => {
                self.pos += 1;
                Ok("*".to_owned())
            }
            Some(quote @ ('"' | '\'')) => {
                self.pos += 1;
                let start = self.pos;
                while self.peek().is_some_and(|c| c != quote) {
                    self.pos += 1;
                }
                if self.peek() != Some(quote) {
                    return err("pxp:CSS: unclosed string");
                }
                let token = self.chars[start..self.pos].iter().collect();
                self.pos += 1;
                Ok(token)
            }
            Some(c) => {
                let start = self.pos;
                while self
                    .peek()
                    .is_some_and(|c| !TOKEN_END.contains(&c) && !SPACE.contains(&c))
                {
                    self.pos += 1;
                }
                if self.pos == start {
                    self.pos += 1;
                    return Ok(c.to_string());
                }
                Ok(self.chars[start..self.pos].iter().collect())
            }
        }
    }

    fn integer(&mut self) -> String {
        let start = self.pos;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.pos += 1;
        }
        self.chars[start..self.pos].iter().collect()
    }

    /// An element or attribute name, `*` when there is none (`namespacedIdent`).
    fn ident(&mut self) -> XResult<String> {
        let mut namespace = "*".to_owned();
        let mut element = "*".to_owned();
        match self.peek() {
            Some('|') => {
                namespace = String::new();
                self.pos += 1;
                element = self.token()?;
            }
            Some('#' | '.' | '[' | ':') | None => {}
            Some(_) => {
                let token = self.token()?;
                if self.peek() != Some('|') || self.chars.get(self.pos + 1) == Some(&'=') {
                    element = token;
                } else {
                    namespace = token;
                    self.pos += 1;
                    element = self.token()?;
                }
            }
        }
        Ok(if namespace != "*" || element != "*" {
            if namespace.is_empty() || namespace == "*" {
                element
            } else {
                format!("{namespace}:{element}")
            }
        } else {
            element
        })
    }

    fn hash(&mut self) -> XResult<Expr> {
        self.expect('#')?;
        Ok(equals(attribute("id"), Expr::Str(self.token()?)))
    }

    fn class(&mut self) -> XResult<Expr> {
        self.expect('.')?;
        Ok(call(
            "split-equal",
            vec![attribute("class"), Expr::Str(self.token()?)],
        ))
    }

    fn attrib(&mut self) -> XResult<Expr> {
        self.expect('[')?;
        self.skip_space();
        let name = self.ident()?;
        self.skip_space();
        let op = self.peek();
        self.pos += 1;
        if op == Some(']') {
            return Ok(call("exists", vec![attribute(&name)]));
        }
        if op != Some('=') {
            self.expect('=')?;
        }
        self.skip_space();
        let value = Expr::Str(self.token()?);
        let a = attribute(&name);
        let result = match op {
            Some('^') => call("starts-with", vec![a, value]),
            Some('$') => call("ends-with", vec![a, value]),
            Some('*') => call("contains", vec![a, value]),
            Some('~') => call("split-equal", vec![a, value, Expr::Str(" ".into())]),
            Some('|') => call("split-equal", vec![a, value, Expr::Str("-".into())]),
            Some('=') => equals(a, value),
            _ => return err("pxp:CSS: unknown attribute operator"),
        };
        self.skip_space();
        self.expect(']')?;
        Ok(result)
    }

    /// The siblings of the same type as `element` along `axis` (`allOfSameType`).
    fn same_type(element: &str, axis: Option<Axis>) -> Expr {
        let siblings = |name: &str| match axis {
            Some(axis) => step(axis, name),
            None => path(
                Expr::Step(Axis::Parent, NodeTest::Node, Vec::new()),
                step(Axis::Child, name),
            ),
        };
        if element != "*" {
            return siblings(element);
        }
        // for $__csstemp in name() return siblings::*[$__csstemp = name()]
        Expr::For(
            "__csstemp".to_owned(),
            Box::new(call("name", Vec::new())),
            Box::new(Expr::Filter(
                Box::new(siblings("*")),
                Box::new(equals(
                    Expr::Var("__csstemp".to_owned()),
                    call("name", Vec::new()),
                )),
            )),
        )
    }

    fn pseudo(&mut self, element: &str) -> XResult<Expr> {
        self.expect(':')?;
        if self.peek() == Some(':') {
            return err("pxp:CSS: pseudo elements are not supported");
        }
        let name = self.token()?.to_ascii_lowercase();
        let preceding = || step(Axis::PrecedingSibling, "*");
        let following = || step(Axis::FollowingSibling, "*");
        if self.peek() != Some('(') {
            let empty = |e: Expr| call("empty", vec![e]);
            return Ok(match name.as_str() {
                "root" => Expr::Binary(
                    BinOp::Is,
                    Box::new(Expr::Step(Axis::Parent, NodeTest::Node, Vec::new())),
                    Box::new(Expr::Root),
                ),
                "first-child" => empty(preceding()),
                "last-child" => empty(following()),
                "first-of-type" => empty(Self::same_type(element, Some(Axis::PrecedingSibling))),
                "last-of-type" => empty(Self::same_type(element, Some(Axis::FollowingSibling))),
                "only-child" => Expr::Binary(
                    BinOp::And,
                    Box::new(empty(preceding())),
                    Box::new(empty(following())),
                ),
                "only-of-type" => equals(
                    call("count", vec![Self::same_type(element, None)]),
                    Expr::Int(1),
                ),
                "empty" => call(
                    "not",
                    vec![Expr::Step(Axis::Child, NodeTest::Node, Vec::new())],
                ),
                "link" => Expr::Binary(
                    BinOp::And,
                    Box::new(step(Axis::SelfNode, "a")),
                    Box::new(call("exists", vec![attribute("href")])),
                ),
                "checked" => call("exists", vec![attribute("checked")]),
                _ => return err(format!("pxp:CSS: unsupported pseudo class {name}")),
            });
        }
        self.expect('(')?;
        self.skip_space();
        let result = match name.as_str() {
            "lang" => {
                let lang = Expr::Str(self.token()?);
                equals(
                    lang,
                    Expr::Filter(
                        Box::new(path(step(Axis::AncestorOrSelf, "*"), attribute("lang"))),
                        Box::new(call("last", Vec::new())),
                    ),
                )
            }
            "not" => {
                let inner = match self.peek() {
                    None => return err("pxp:CSS: unclosed not"),
                    Some('#') => self.hash()?,
                    Some('.') => self.class()?,
                    Some('[') => self.attrib()?,
                    Some(':') => self.pseudo(element)?,
                    Some(_) => {
                        let ident = self.ident()?;
                        step(Axis::SelfNode, &ident)
                    }
                };
                call("not", vec![inner])
            }
            "nth-child" | "nth-last-child" | "nth-of-type" | "nth-last-of-type" => {
                let (a, b) = self.nth()?;
                let index = match name.as_str() {
                    "nth-child" => call("count", vec![preceding()]),
                    "nth-last-child" => call("count", vec![following()]),
                    "nth-of-type" => call(
                        "count",
                        vec![Self::same_type(element, Some(Axis::PrecedingSibling))],
                    ),
                    _ => call(
                        "count",
                        vec![Self::same_type(element, Some(Axis::FollowingSibling))],
                    ),
                };
                if a == 0 {
                    equals(index, Expr::Int(b - 1))
                } else {
                    call("is-nth", vec![index, Expr::Int(a), Expr::Int(b - 1)])
                }
            }
            _ => return err(format!("pxp:CSS: unknown function {name}")),
        };
        self.skip_space();
        self.expect(')')?;
        Ok(result)
    }

    /// `an+b`, `odd` or `even`.
    fn nth(&mut self) -> XResult<(i64, i64)> {
        let number = |s: &str| -> XResult<i64> {
            s.parse()
                .map_err(|_| super::value::XPathError(format!("pxp:CSS: bad number {s:?}")))
        };
        match self.peek() {
            Some('o' | 'O') => {
                if !self.token()?.eq_ignore_ascii_case("odd") {
                    return err("pxp:CSS: expected odd");
                }
                Ok((2, 1))
            }
            Some('e' | 'E') => {
                if !self.token()?.eq_ignore_ascii_case("even") {
                    return err("pxp:CSS: expected even");
                }
                Ok((2, 0))
            }
            Some('-' | '+' | '0'..='9') => {
                let mut sign = 1;
                if self.peek() == Some('-') {
                    self.pos += 1;
                    sign = -1;
                } else if self.peek() == Some('+') {
                    self.pos += 1;
                }
                let digits = self.integer();
                if matches!(self.peek(), Some('n' | 'N')) {
                    let a = if digits.is_empty() {
                        sign
                    } else {
                        sign * number(&digits)?
                    };
                    self.pos += 1;
                    Ok((a, self.offset()?))
                } else {
                    Ok((0, sign * number(&digits)?))
                }
            }
            Some('n' | 'N') => {
                self.pos += 1;
                Ok((1, self.offset()?))
            }
            _ => err("pxp:CSS: expected nth"),
        }
    }

    /// The `+b` of `an+b`.
    fn offset(&mut self) -> XResult<i64> {
        self.skip_space();
        let sign = match self.peek() {
            Some('-') => -1,
            Some('+') => 1,
            _ => return Ok(0),
        };
        self.pos += 1;
        self.skip_space();
        let token = self.token()?;
        token
            .parse::<i64>()
            .map(|b| sign * b)
            .map_err(|_| super::value::XPathError(format!("pxp:CSS: bad number {token:?}")))
    }

    /// One complex selector (`simple_selector_sequence`).
    fn selector(&mut self) -> XResult<Expr> {
        let mut axis = Axis::DescendantOrSelf;
        let mut adjacent = false;
        let mut result: Option<Expr> = None;
        while !self.at_end() {
            let element = self.ident()?;
            if (element == "*" || element == "*:*") && axis == Axis::DescendantOrSelf {
                axis = Axis::Descendant;
            }
            let matcher = if adjacent {
                Expr::Filter(
                    Box::new(Expr::Step(axis, NodeTest::AnyName, vec![Expr::Int(1)])),
                    Box::new(step(Axis::SelfNode, &element)),
                )
            } else {
                step(axis, &element)
            };
            let mut current = match result.take() {
                None => matcher,
                Some(left) => path(left, matcher),
            };
            while let Some(c @ ('#' | '.' | '[' | ':')) = self.peek() {
                let filter = match c {
                    '#' => self.hash()?,
                    '.' => self.class()?,
                    '[' => self.attrib()?,
                    _ => self.pseudo(&element)?,
                };
                current = Expr::Filter(Box::new(current), Box::new(filter));
            }
            result = Some(current);
            self.skip_space();
            match self.peek() {
                None | Some(',' | ')') => break,
                Some('+') => {
                    axis = Axis::FollowingSibling;
                    adjacent = true;
                    self.pos += 1;
                    self.skip_space();
                }
                Some('>') => {
                    axis = Axis::Child;
                    adjacent = false;
                    self.pos += 1;
                    self.skip_space();
                }
                Some('~') => {
                    axis = Axis::FollowingSibling;
                    adjacent = false;
                    self.pos += 1;
                    self.skip_space();
                }
                Some(_) => {
                    axis = Axis::Descendant;
                    adjacent = false;
                }
            }
        }
        match result {
            Some(result) => Ok(result),
            None => err("pxp:CSS: empty selector"),
        }
    }
}
