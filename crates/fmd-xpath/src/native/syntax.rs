//! XPath 3.1 with internettools' extensions, parsed into an [`Expr`] tree.
//!
//! The parser follows the options `TXQueryEngine.Create` sets (internettools
//! data/xquery.pas:8376-8414): XPath 3.1, JSONiq object literals and `true`/`false`/`null`
//! literals, and the "unambiguous" dot notation, where `.name` reads a JSON property after a
//! closing parenthesis or bracket (`json(*).data.items`) but `$var.name` is a variable name.

use super::value::{XResult, err};

/// One token.
#[derive(Clone, Debug, PartialEq)]
enum Tok {
    /// An NCName or QName; names may contain `.` and `-`.
    Name(String),
    Var(String),
    Str(String),
    Int(String),
    Dec(String),
    Dbl(String),
    Sym(&'static str),
    /// `.name` right after `)`, `]` or `}`: a property lookup in dot notation.
    Dot(String),
    Eof,
}

const SYMBOLS: &[&str] = &[
    "!=", "//", "::", "..", "<=", ">=", "<<", ">>", "||", ":=", "=>", "(", ")", "[", "]", "{", "}",
    ",", "/", "@", ".", "*", "+", "-", "=", "<", ">", "|", "!", "?", ":", "$",
];

fn is_name_start(c: char) -> bool {
    c.is_alphabetic() || c == '_' || (!c.is_ascii() && !c.is_whitespace())
}

fn is_name_char(c: char) -> bool {
    is_name_start(c) || c.is_ascii_digit() || c == '-' || c == '.'
}

fn lex(src: &str) -> XResult<Vec<Tok>> {
    let chars: Vec<char> = src.chars().collect();
    let mut toks = Vec::new();
    let mut i = 0;
    let name_at = |i: usize| -> usize {
        let mut j = i;
        while j < chars.len() && is_name_char(chars[j]) {
            j += 1;
        }
        j
    };
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        // Comments `(: ... :)`, nestable.
        if c == '(' && chars.get(i + 1) == Some(&':') {
            let mut depth = 0;
            while i < chars.len() {
                if chars[i] == '(' && chars.get(i + 1) == Some(&':') {
                    depth += 1;
                    i += 2;
                } else if chars[i] == ':' && chars.get(i + 1) == Some(&')') {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    i += 1;
                }
            }
            if depth != 0 {
                return err("XPST0003: unterminated comment");
            }
            continue;
        }
        if c == '"' || c == '\'' {
            let mut s = String::new();
            i += 1;
            loop {
                match chars.get(i) {
                    None => return err("XPST0003: unterminated string"),
                    Some(&q) if q == c => {
                        if chars.get(i + 1) == Some(&c) {
                            s.push(c);
                            i += 2;
                        } else {
                            i += 1;
                            break;
                        }
                    }
                    // CR LF and a lone CR become LF (`normalizeLineEnding` with the default
                    // `xqlenXML1`, internettools data/xquery__parse.pas:1205-1212, :2653-2656,
                    // data/xquery.pas:8384).
                    Some('\r') => {
                        s.push('\n');
                        i += if chars.get(i + 1) == Some(&'\n') {
                            2
                        } else {
                            1
                        };
                    }
                    Some(&other) => {
                        s.push(other);
                        i += 1;
                    }
                }
            }
            toks.push(Tok::Str(s));
            continue;
        }
        if c.is_ascii_digit() || (c == '.' && chars.get(i + 1).is_some_and(char::is_ascii_digit)) {
            let start = i;
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
            let mut decimal = false;
            if chars.get(i) == Some(&'.') && chars.get(i + 1) != Some(&'.') {
                decimal = true;
                i += 1;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
            }
            let mut double = false;
            if matches!(chars.get(i), Some('e' | 'E')) {
                let mut j = i + 1;
                if matches!(chars.get(j), Some('+' | '-')) {
                    j += 1;
                }
                if chars.get(j).is_some_and(char::is_ascii_digit) {
                    double = true;
                    i = j;
                    while i < chars.len() && chars[i].is_ascii_digit() {
                        i += 1;
                    }
                }
            }
            if chars.get(i).is_some_and(|&c| is_name_start(c)) {
                return err("XPST0003: a number followed by a name");
            }
            let text: String = chars[start..i].iter().collect();
            toks.push(if double {
                Tok::Dbl(text)
            } else if decimal {
                Tok::Dec(text)
            } else {
                Tok::Int(text)
            });
            continue;
        }
        if c == '$' {
            let start = i + 1;
            let mut end = name_at(start);
            if end == start {
                return err("XPST0003: `$` without a name");
            }
            if chars.get(end) == Some(&':') && chars.get(end + 1).is_some_and(|&c| is_name_start(c))
            {
                end = name_at(end + 1);
            }
            toks.push(Tok::Var(chars[start..end].iter().collect()));
            i = end;
            continue;
        }
        // Dot notation: `.name` right after `)`, `]` or `}`.
        if c == '.'
            && matches!(toks.last(), Some(Tok::Sym(")" | "]" | "}")))
            && chars.get(i + 1).is_some_and(|&c| is_name_start(c))
        {
            let end = name_at(i + 1);
            toks.push(Tok::Dot(chars[i + 1..end].iter().collect()));
            i = end;
            continue;
        }
        if is_name_start(c) {
            let mut end = name_at(i);
            // A QName, or `prefix:*`; not an axis (`::`).
            if chars.get(end) == Some(&':') && chars.get(end + 1) != Some(&':') {
                if chars.get(end + 1).is_some_and(|&c| is_name_start(c)) {
                    end = name_at(end + 1);
                } else if chars.get(end + 1) == Some(&'*') {
                    end += 2;
                }
            }
            toks.push(Tok::Name(chars[i..end].iter().collect()));
            i = end;
            continue;
        }
        if c == '*'
            && chars.get(i + 1) == Some(&':')
            && chars.get(i + 2).is_some_and(|&c| is_name_start(c))
        {
            let end = name_at(i + 2);
            toks.push(Tok::Name(chars[i..end].iter().collect()));
            i = end;
            continue;
        }
        let rest: String = chars[i..chars.len().min(i + 2)].iter().collect();
        match SYMBOLS.iter().find(|s| rest.starts_with(**s)) {
            Some(sym) => {
                toks.push(Tok::Sym(sym));
                i += sym.chars().count();
            }
            None => return err(format!("XPST0003: unexpected character {c:?}")),
        }
    }
    toks.push(Tok::Eof);
    Ok(toks)
}

/// An axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Axis {
    Child,
    Descendant,
    DescendantOrSelf,
    Attribute,
    SelfNode,
    Parent,
    Ancestor,
    AncestorOrSelf,
    Following,
    FollowingSibling,
    Preceding,
    PrecedingSibling,
}

impl Axis {
    fn from_name(name: &str) -> Option<Axis> {
        Some(match name {
            "child" => Axis::Child,
            "descendant" => Axis::Descendant,
            "descendant-or-self" => Axis::DescendantOrSelf,
            "attribute" => Axis::Attribute,
            "self" => Axis::SelfNode,
            "parent" => Axis::Parent,
            "ancestor" => Axis::Ancestor,
            "ancestor-or-self" => Axis::AncestorOrSelf,
            "following" => Axis::Following,
            "following-sibling" => Axis::FollowingSibling,
            "preceding" => Axis::Preceding,
            "preceding-sibling" => Axis::PrecedingSibling,
            _ => return None,
        })
    }

    /// Whether positions count backwards from the context node.
    pub(crate) fn is_reverse(self) -> bool {
        matches!(
            self,
            Axis::Parent
                | Axis::Ancestor
                | Axis::AncestorOrSelf
                | Axis::Preceding
                | Axis::PrecedingSibling
        )
    }
}

/// A node test.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum NodeTest {
    /// A name (ASCII case-insensitive, like internettools on HTML); `*` matches any.
    Name(String),
    /// `*:name`: the local part of a name, whatever its prefix.
    LocalName(String),
    AnyName,
    /// `node()`.
    Node,
    /// `text()`.
    Text,
    /// `element()` or `element(name)`.
    Element(Option<String>),
    /// `attribute()` or `attribute(name)`.
    AttributeTest(Option<String>),
    /// `document-node()`.
    Document,
    /// `comment()` and `processing-instruction()`: the tree has neither.
    Nothing,
}

/// The node test a name makes: `*:local`, `prefix:*` or a name.
fn name_test(name: String) -> NodeTest {
    if let Some(local) = name.strip_prefix("*:") {
        NodeTest::LocalName(local.to_owned())
    } else if name.ends_with(":*") {
        NodeTest::AnyName
    } else {
        NodeTest::Name(name)
    }
}

/// The key of a `?` lookup.
#[derive(Clone, Debug)]
pub(crate) enum Key {
    Name(String),
    Int(i64),
    Wildcard,
    Expr(Box<Expr>),
}

/// Binary operators.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BinOp {
    Or,
    And,
    /// General comparisons `= != < <= > >=`.
    General(CmpOp),
    /// Value comparisons `eq ne lt le gt ge`.
    Value(CmpOp),
    Is,
    Precedes,
    Follows,
    Concat,
    Range,
    Add,
    Sub,
    Mul,
    Div,
    IDiv,
    Mod,
    Union,
    Intersect,
    Except,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CmpOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

/// A sequence type, as `instance of`, `cast as` and `castable as` take it.
#[derive(Clone, Debug)]
pub(crate) struct SeqType {
    /// The item type's name: an atomic type (`xs:integer`), `item()`, `node()`, ...
    pub(crate) name: String,
    /// `?`, `*` or `+`, or none.
    pub(crate) occurrence: Option<char>,
}

/// An expression.
#[derive(Clone, Debug)]
pub(crate) enum Expr {
    Str(String),
    Int(i64),
    Dec(String),
    Dbl(f64),
    Bool(bool),
    Null,
    Var(String),
    Context,
    /// The root of the context node's tree (`/`).
    Root,
    Sequence(Vec<Expr>),
    /// `a / b`: `b` for every item of `a`.
    Path(Box<Expr>, Box<Expr>),
    Step(Axis, NodeTest, Vec<Expr>),
    /// `e[predicate]`.
    Filter(Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
    DynCall(Box<Expr>, Vec<Expr>),
    Lookup(Box<Expr>, Key),
    UnaryLookup(Key),
    /// Dot notation: `(e).name`.
    Property(Box<Expr>, String),
    SimpleMap(Box<Expr>, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Neg(Box<Expr>),
    If(Box<Expr>, Box<Expr>, Box<Expr>),
    For(String, Box<Expr>, Box<Expr>),
    Let(String, Box<Expr>, Box<Expr>),
    /// `some`/`every` (`true` for `every`).
    Quantified(bool, String, Box<Expr>, Box<Expr>),
    /// A JSONiq object `{"k": v}` or an XPath map `map {"k": v}`.
    Object(Vec<(Expr, Expr)>),
    /// `[a, b]`: one member per expression.
    SquareArray(Vec<Expr>),
    /// `array {e}`: one member per item.
    CurlyArray(Box<Expr>),
    InstanceOf(Box<Expr>, SeqType),
    Cast(Box<Expr>, SeqType),
    Castable(Box<Expr>, SeqType),
}

/// Parses an expression.
pub(crate) fn parse(src: &str) -> XResult<Expr> {
    let toks = lex(src)?;
    let mut parser = Parser { toks, pos: 0 };
    if parser.peek() == &Tok::Eof {
        return err("XPST0003: no input");
    }
    let expr = parser.expr()?;
    if parser.peek() != &Tok::Eof {
        return err(format!("XPST0003: unexpected {:?}", parser.peek()));
    }
    Ok(expr)
}

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
}

const KIND_TESTS: &[&str] = &[
    "node",
    "text",
    "comment",
    "element",
    "attribute",
    "document-node",
    "processing-instruction",
];

impl Parser {
    fn peek(&self) -> &Tok {
        self.peek_at(0)
    }

    fn peek_at(&self, n: usize) -> &Tok {
        self.toks.get(self.pos + n).unwrap_or(&Tok::Eof)
    }

    fn next(&mut self) -> Tok {
        let tok = self.peek().clone();
        if self.pos < self.toks.len() {
            self.pos += 1;
        }
        tok
    }

    fn is_sym(&self, sym: &str) -> bool {
        matches!(self.peek(), Tok::Sym(s) if *s == sym)
    }

    fn is_name(&self, name: &str) -> bool {
        matches!(self.peek(), Tok::Name(n) if n == name)
    }

    fn eat_sym(&mut self, sym: &str) -> bool {
        if self.is_sym(sym) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn eat_name(&mut self, name: &str) -> bool {
        if self.is_name(name) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect_sym(&mut self, sym: &str) -> XResult<()> {
        if self.eat_sym(sym) {
            Ok(())
        } else {
            err(format!(
                "XPST0003: expected `{sym}`, found {:?}",
                self.peek()
            ))
        }
    }

    fn expect_name(&mut self, name: &str) -> XResult<()> {
        if self.eat_name(name) {
            Ok(())
        } else {
            err(format!(
                "XPST0003: expected `{name}`, found {:?}",
                self.peek()
            ))
        }
    }

    fn var_name(&mut self) -> XResult<String> {
        match self.next() {
            Tok::Var(name) => Ok(name),
            other => err(format!("XPST0003: expected a variable, found {other:?}")),
        }
    }

    /// `Expr ::= ExprSingle ("," ExprSingle)*`
    fn expr(&mut self) -> XResult<Expr> {
        let first = self.expr_single()?;
        if !self.is_sym(",") {
            return Ok(first);
        }
        let mut items = vec![first];
        while self.eat_sym(",") {
            items.push(self.expr_single()?);
        }
        Ok(Expr::Sequence(items))
    }

    fn expr_single(&mut self) -> XResult<Expr> {
        if let (Tok::Name(n), Tok::Var(_)) = (self.peek(), self.peek_at(1)) {
            match n.as_str() {
                "for" => return self.for_expr(),
                "let" => return self.let_expr(),
                "some" | "every" => return self.quantified(),
                _ => {}
            }
        }
        if self.is_name("if") && matches!(self.peek_at(1), Tok::Sym("(")) {
            self.pos += 2;
            let condition = self.expr()?;
            self.expect_sym(")")?;
            self.expect_name("then")?;
            let then = self.expr_single()?;
            self.expect_name("else")?;
            let otherwise = self.expr_single()?;
            return Ok(Expr::If(
                Box::new(condition),
                Box::new(then),
                Box::new(otherwise),
            ));
        }
        self.or_expr()
    }

    fn for_expr(&mut self) -> XResult<Expr> {
        self.pos += 1;
        let mut bindings = Vec::new();
        loop {
            let name = self.var_name()?;
            self.expect_name("in")?;
            bindings.push((name, self.expr_single()?));
            if !self.eat_sym(",") {
                break;
            }
        }
        self.expect_name("return")?;
        let mut body = self.expr_single()?;
        for (name, source) in bindings.into_iter().rev() {
            body = Expr::For(name, Box::new(source), Box::new(body));
        }
        Ok(body)
    }

    fn let_expr(&mut self) -> XResult<Expr> {
        self.pos += 1;
        let mut bindings = Vec::new();
        loop {
            let name = self.var_name()?;
            self.expect_sym(":=")?;
            bindings.push((name, self.expr_single()?));
            if !self.eat_sym(",") {
                break;
            }
        }
        self.expect_name("return")?;
        let mut body = self.expr_single()?;
        for (name, value) in bindings.into_iter().rev() {
            body = Expr::Let(name, Box::new(value), Box::new(body));
        }
        Ok(body)
    }

    fn quantified(&mut self) -> XResult<Expr> {
        let every = matches!(self.next(), Tok::Name(n) if n == "every");
        let mut bindings = Vec::new();
        loop {
            let name = self.var_name()?;
            self.expect_name("in")?;
            bindings.push((name, self.expr_single()?));
            if !self.eat_sym(",") {
                break;
            }
        }
        self.expect_name("satisfies")?;
        let mut body = self.expr_single()?;
        for (name, source) in bindings.into_iter().rev() {
            body = Expr::Quantified(every, name, Box::new(source), Box::new(body));
        }
        Ok(body)
    }

    fn or_expr(&mut self) -> XResult<Expr> {
        let mut left = self.and_expr()?;
        while self.eat_name("or") {
            left = Expr::Binary(BinOp::Or, Box::new(left), Box::new(self.and_expr()?));
        }
        Ok(left)
    }

    fn and_expr(&mut self) -> XResult<Expr> {
        let mut left = self.comparison()?;
        while self.eat_name("and") {
            left = Expr::Binary(BinOp::And, Box::new(left), Box::new(self.comparison()?));
        }
        Ok(left)
    }

    fn comparison(&mut self) -> XResult<Expr> {
        let left = self.concat()?;
        let op = match self.peek() {
            Tok::Sym("=") => BinOp::General(CmpOp::Eq),
            Tok::Sym("!=") => BinOp::General(CmpOp::Ne),
            Tok::Sym("<") => BinOp::General(CmpOp::Lt),
            Tok::Sym("<=") => BinOp::General(CmpOp::Le),
            Tok::Sym(">") => BinOp::General(CmpOp::Gt),
            Tok::Sym(">=") => BinOp::General(CmpOp::Ge),
            Tok::Sym("<<") => BinOp::Precedes,
            Tok::Sym(">>") => BinOp::Follows,
            Tok::Name(n) => match n.as_str() {
                "eq" => BinOp::Value(CmpOp::Eq),
                "ne" => BinOp::Value(CmpOp::Ne),
                "lt" => BinOp::Value(CmpOp::Lt),
                "le" => BinOp::Value(CmpOp::Le),
                "gt" => BinOp::Value(CmpOp::Gt),
                "ge" => BinOp::Value(CmpOp::Ge),
                "is" => BinOp::Is,
                _ => return Ok(left),
            },
            _ => return Ok(left),
        };
        self.pos += 1;
        let right = self.concat()?;
        Ok(Expr::Binary(op, Box::new(left), Box::new(right)))
    }

    fn concat(&mut self) -> XResult<Expr> {
        let mut left = self.range()?;
        while self.eat_sym("||") {
            left = Expr::Binary(BinOp::Concat, Box::new(left), Box::new(self.range()?));
        }
        Ok(left)
    }

    fn range(&mut self) -> XResult<Expr> {
        let left = self.additive()?;
        if self.eat_name("to") {
            let right = self.additive()?;
            return Ok(Expr::Binary(BinOp::Range, Box::new(left), Box::new(right)));
        }
        Ok(left)
    }

    fn additive(&mut self) -> XResult<Expr> {
        let mut left = self.multiplicative()?;
        loop {
            let op = if self.eat_sym("+") {
                BinOp::Add
            } else if self.eat_sym("-") {
                BinOp::Sub
            } else {
                return Ok(left);
            };
            left = Expr::Binary(op, Box::new(left), Box::new(self.multiplicative()?));
        }
    }

    fn multiplicative(&mut self) -> XResult<Expr> {
        let mut left = self.union()?;
        loop {
            let op = if self.eat_sym("*") {
                BinOp::Mul
            } else if self.eat_name("div") {
                BinOp::Div
            } else if self.eat_name("idiv") {
                BinOp::IDiv
            } else if self.eat_name("mod") {
                BinOp::Mod
            } else {
                return Ok(left);
            };
            left = Expr::Binary(op, Box::new(left), Box::new(self.union()?));
        }
    }

    fn union(&mut self) -> XResult<Expr> {
        let mut left = self.intersect()?;
        while self.eat_sym("|") || self.eat_name("union") {
            left = Expr::Binary(BinOp::Union, Box::new(left), Box::new(self.intersect()?));
        }
        Ok(left)
    }

    fn intersect(&mut self) -> XResult<Expr> {
        let mut left = self.instance_of()?;
        loop {
            let op = if self.eat_name("intersect") {
                BinOp::Intersect
            } else if self.eat_name("except") {
                BinOp::Except
            } else {
                return Ok(left);
            };
            left = Expr::Binary(op, Box::new(left), Box::new(self.instance_of()?));
        }
    }

    fn instance_of(&mut self) -> XResult<Expr> {
        let left = self.castable()?;
        if self.is_name("instance") && matches!(self.peek_at(1), Tok::Name(n) if n == "of") {
            self.pos += 2;
            let ty = self.seq_type()?;
            return Ok(Expr::InstanceOf(Box::new(left), ty));
        }
        Ok(left)
    }

    fn castable(&mut self) -> XResult<Expr> {
        let left = self.cast()?;
        if self.is_name("castable") && matches!(self.peek_at(1), Tok::Name(n) if n == "as") {
            self.pos += 2;
            let ty = self.seq_type()?;
            return Ok(Expr::Castable(Box::new(left), ty));
        }
        Ok(left)
    }

    fn cast(&mut self) -> XResult<Expr> {
        let left = self.arrow()?;
        if self.is_name("cast") && matches!(self.peek_at(1), Tok::Name(n) if n == "as") {
            self.pos += 2;
            let ty = self.seq_type()?;
            return Ok(Expr::Cast(Box::new(left), ty));
        }
        Ok(left)
    }

    fn seq_type(&mut self) -> XResult<SeqType> {
        let name = match self.next() {
            Tok::Name(n) => n,
            other => return err(format!("XPST0003: expected a type, found {other:?}")),
        };
        if self.eat_sym("(") {
            // `item()`, `node()`, `element()`, ...: arguments are ignored.
            let mut depth = 1;
            while depth > 0 {
                match self.next() {
                    Tok::Sym("(") => depth += 1,
                    Tok::Sym(")") => depth -= 1,
                    Tok::Eof => return err("XPST0003: unterminated type"),
                    _ => {}
                }
            }
        }
        let occurrence = match self.peek() {
            Tok::Sym("?") => Some('?'),
            Tok::Sym("*") => Some('*'),
            Tok::Sym("+") => Some('+'),
            _ => None,
        };
        if occurrence.is_some() {
            self.pos += 1;
        }
        Ok(SeqType { name, occurrence })
    }

    fn arrow(&mut self) -> XResult<Expr> {
        let mut left = self.unary()?;
        while self.eat_sym("=>") {
            let name = match self.next() {
                Tok::Name(n) => n,
                other => return err(format!("XPST0003: expected a function, found {other:?}")),
            };
            let mut args = vec![left];
            args.extend(self.arguments()?);
            left = Expr::Call(name, args);
        }
        Ok(left)
    }

    fn unary(&mut self) -> XResult<Expr> {
        if self.eat_sym("-") {
            return Ok(Expr::Neg(Box::new(self.unary()?)));
        }
        if self.eat_sym("+") {
            let operand = self.unary()?;
            return Ok(Expr::Binary(
                BinOp::Add,
                Box::new(Expr::Int(0)),
                Box::new(operand),
            ));
        }
        self.simple_map()
    }

    fn simple_map(&mut self) -> XResult<Expr> {
        let mut left = self.path()?;
        while self.eat_sym("!") {
            left = Expr::SimpleMap(Box::new(left), Box::new(self.path()?));
        }
        Ok(left)
    }

    fn path(&mut self) -> XResult<Expr> {
        if self.eat_sym("/") {
            if self.starts_step() {
                let first = self.step()?;
                return self.relative_path(Expr::Path(Box::new(Expr::Root), Box::new(first)));
            }
            return Ok(Expr::Root);
        }
        if self.eat_sym("//") {
            let first = self.step()?;
            return self.relative_path(Expr::Path(
                Box::new(Expr::Root),
                Box::new(descendants(first)),
            ));
        }
        let first = self.step()?;
        self.relative_path(first)
    }

    /// Whether the next token can start a step, so `/` alone is the root.
    fn starts_step(&self) -> bool {
        match self.peek() {
            Tok::Name(n) => {
                !matches!(
                    n.as_str(),
                    "or" | "and"
                        | "div"
                        | "mod"
                        | "idiv"
                        | "eq"
                        | "ne"
                        | "lt"
                        | "le"
                        | "gt"
                        | "ge"
                        | "is"
                        | "to"
                        | "union"
                        | "intersect"
                        | "except"
                        | "instance"
                        | "cast"
                        | "castable"
                        | "treat"
                        | "return"
                        | "satisfies"
                        | "then"
                        | "else"
                ) || matches!(self.peek_at(1), Tok::Sym("(" | "::"))
            }
            Tok::Var(_) | Tok::Str(_) | Tok::Int(_) | Tok::Dec(_) | Tok::Dbl(_) => true,
            Tok::Sym(s) => matches!(*s, "(" | "@" | "." | ".." | "*" | "[" | "{" | "?"),
            Tok::Dot(_) | Tok::Eof => false,
        }
    }

    /// The steps after `left`, left-associative: `a/b/c` is `(a/b)/c`.
    fn relative_path(&mut self, mut left: Expr) -> XResult<Expr> {
        loop {
            if self.eat_sym("/") {
                let right = self.step()?;
                left = Expr::Path(Box::new(left), Box::new(right));
            } else if self.eat_sym("//") {
                let right = self.step()?;
                left = Expr::Path(Box::new(left), Box::new(descendants(right)));
            } else {
                return Ok(left);
            }
        }
    }

    fn step(&mut self) -> XResult<Expr> {
        if self.eat_sym("..") {
            return self.predicates(Axis::Parent, NodeTest::Node);
        }
        if self.eat_sym("@") {
            let test = self.node_test(Axis::Attribute)?;
            return self.predicates(Axis::Attribute, test);
        }
        if self.is_sym("*") {
            self.pos += 1;
            return self.predicates(Axis::Child, NodeTest::AnyName);
        }
        if let Tok::Name(name) = self.peek().clone() {
            if matches!(self.peek_at(1), Tok::Sym("::")) {
                let axis = Axis::from_name(&name)
                    .ok_or_else(|| super::value::XPathError(format!("XPST0003: no axis {name}")))?;
                self.pos += 2;
                let test = self.node_test(axis)?;
                return self.predicates(axis, test);
            }
            let next = self.peek_at(1).clone();
            let is_call = matches!(next, Tok::Sym("("));
            let is_constructor =
                matches!(next, Tok::Sym("{")) && matches!(name.as_str(), "map" | "array");
            let is_literal = matches!(name.as_str(), "true" | "false" | "null");
            if is_call && KIND_TESTS.contains(&name.as_str()) {
                let test = self.node_test(Axis::Child)?;
                let axis = if matches!(test, NodeTest::AttributeTest(_)) {
                    Axis::Attribute
                } else {
                    Axis::Child
                };
                return self.predicates(axis, test);
            }
            if !is_call && !is_constructor && !is_literal {
                self.pos += 1;
                return self.predicates(Axis::Child, name_test(name));
            }
        }
        self.postfix()
    }

    fn node_test(&mut self, axis: Axis) -> XResult<NodeTest> {
        match self.next() {
            Tok::Sym("*") => Ok(NodeTest::AnyName),
            Tok::Name(name) => {
                if !self.is_sym("(") || !KIND_TESTS.contains(&name.as_str()) {
                    return Ok(name_test(name));
                }
                self.pos += 1;
                let argument = match self.peek().clone() {
                    Tok::Name(n) => {
                        self.pos += 1;
                        Some(n)
                    }
                    Tok::Sym("*") => {
                        self.pos += 1;
                        None
                    }
                    Tok::Str(_) => {
                        self.pos += 1;
                        None
                    }
                    _ => None,
                };
                self.expect_sym(")")?;
                Ok(match name.as_str() {
                    "node" => NodeTest::Node,
                    "text" => NodeTest::Text,
                    "element" => NodeTest::Element(argument),
                    "attribute" => NodeTest::AttributeTest(argument),
                    "document-node" => NodeTest::Document,
                    _ => NodeTest::Nothing,
                })
            }
            other => {
                let _ = axis;
                err(format!("XPST0003: expected a node test, found {other:?}"))
            }
        }
    }

    fn predicates(&mut self, axis: Axis, test: NodeTest) -> XResult<Expr> {
        let mut predicates = Vec::new();
        while self.eat_sym("[") {
            predicates.push(self.expr()?);
            self.expect_sym("]")?;
        }
        let step = Expr::Step(axis, test, predicates);
        // internettools allows lookups and dot notation after a step too (`genres?*?name`).
        if self.is_sym("?") && self.lookup_follows() || matches!(self.peek(), Tok::Dot(_)) {
            return self.postfix_of(step);
        }
        Ok(step)
    }

    fn arguments(&mut self) -> XResult<Vec<Expr>> {
        self.expect_sym("(")?;
        let mut args = Vec::new();
        if self.eat_sym(")") {
            return Ok(args);
        }
        loop {
            args.push(self.expr_single()?);
            if self.eat_sym(")") {
                return Ok(args);
            }
            self.expect_sym(",")?;
        }
    }

    fn postfix(&mut self) -> XResult<Expr> {
        let primary = self.primary()?;
        self.postfix_of(primary)
    }

    /// Predicates, calls, lookups and dot notation after `expr`.
    fn postfix_of(&mut self, mut expr: Expr) -> XResult<Expr> {
        loop {
            if self.eat_sym("[") {
                let predicate = self.expr()?;
                self.expect_sym("]")?;
                expr = Expr::Filter(Box::new(expr), Box::new(predicate));
            } else if self.is_sym("(") {
                let args = self.arguments()?;
                expr = Expr::DynCall(Box::new(expr), args);
            } else if self.is_sym("?") && self.lookup_follows() {
                self.pos += 1;
                let key = self.key()?;
                expr = Expr::Lookup(Box::new(expr), key);
            } else if let Tok::Dot(name) = self.peek().clone() {
                self.pos += 1;
                expr = properties(expr, &name);
            } else {
                return Ok(expr);
            }
        }
    }

    /// Whether the `?` at the cursor starts a lookup key.
    fn lookup_follows(&self) -> bool {
        matches!(
            self.peek_at(1),
            Tok::Name(_) | Tok::Int(_) | Tok::Sym("*" | "(")
        )
    }

    fn key(&mut self) -> XResult<Key> {
        match self.next() {
            Tok::Name(name) => Ok(Key::Name(name)),
            Tok::Int(i) => {
                Ok(Key::Int(i.parse().map_err(|_| {
                    super::value::XPathError("FOAR0002: key overflow".into())
                })?))
            }
            Tok::Sym("*") => Ok(Key::Wildcard),
            Tok::Sym("(") => {
                let expr = self.expr()?;
                self.expect_sym(")")?;
                Ok(Key::Expr(Box::new(expr)))
            }
            other => err(format!("XPST0003: expected a lookup key, found {other:?}")),
        }
    }

    fn primary(&mut self) -> XResult<Expr> {
        match self.next() {
            Tok::Str(s) => Ok(Expr::Str(s)),
            Tok::Int(i) => match i.parse() {
                Ok(i) => Ok(Expr::Int(i)),
                Err(_) => Ok(Expr::Dec(i)),
            },
            Tok::Dec(d) => Ok(Expr::Dec(d)),
            Tok::Dbl(d) => d
                .parse()
                .map(Expr::Dbl)
                .map_err(|_| super::value::XPathError(format!("XPST0003: bad number {d}"))),
            Tok::Var(name) => Ok(Expr::Var(name)),
            Tok::Sym("(") => {
                if self.eat_sym(")") {
                    return Ok(Expr::Sequence(Vec::new()));
                }
                let inner = self.expr()?;
                self.expect_sym(")")?;
                // Keep a parenthesized expression distinct, so `(a)[1]` filters the whole.
                Ok(match inner {
                    Expr::Sequence(items) => Expr::Sequence(items),
                    other => Expr::Sequence(vec![other]),
                })
            }
            Tok::Sym(".") => Ok(Expr::Context),
            Tok::Sym("?") => Ok(Expr::UnaryLookup(self.key()?)),
            Tok::Sym("[") => {
                let mut members = Vec::new();
                if !self.eat_sym("]") {
                    loop {
                        members.push(self.expr_single()?);
                        if self.eat_sym("]") {
                            break;
                        }
                        self.expect_sym(",")?;
                    }
                }
                Ok(Expr::SquareArray(members))
            }
            Tok::Sym("{") => self.object(),
            Tok::Name(name) => match name.as_str() {
                "true" if !self.is_sym("(") => Ok(Expr::Bool(true)),
                "false" if !self.is_sym("(") => Ok(Expr::Bool(false)),
                "null" if !self.is_sym("(") => Ok(Expr::Null),
                "map" if self.is_sym("{") => {
                    self.pos += 1;
                    self.object()
                }
                "array" if self.is_sym("{") => {
                    self.pos += 1;
                    if self.eat_sym("}") {
                        return Ok(Expr::SquareArray(Vec::new()));
                    }
                    let inner = self.expr()?;
                    self.expect_sym("}")?;
                    Ok(Expr::CurlyArray(Box::new(inner)))
                }
                _ => {
                    let args = self.arguments()?;
                    Ok(Expr::Call(name, args))
                }
            },
            other => err(format!("XPST0003: unexpected {other:?}")),
        }
    }

    /// The rest of `{"key": value, ...}` after `{`.
    fn object(&mut self) -> XResult<Expr> {
        let mut entries = Vec::new();
        if self.eat_sym("}") {
            return Ok(Expr::Object(entries));
        }
        loop {
            let key = self.expr_single()?;
            self.expect_sym(":")?;
            let value = self.expr_single()?;
            entries.push((key, value));
            if self.eat_sym("}") {
                return Ok(Expr::Object(entries));
            }
            self.expect_sym(",")?;
        }
    }
}

/// `e.a.b`: one property lookup per dot-separated name.
fn properties(mut expr: Expr, names: &str) -> Expr {
    for name in names.split('.') {
        expr = Expr::Property(Box::new(expr), name.to_owned());
    }
    expr
}

/// `//e`: `descendant-or-self::node()/e`.
fn descendants(step: Expr) -> Expr {
    Expr::Path(
        Box::new(Expr::Step(
            Axis::DescendantOrSelf,
            NodeTest::Node,
            Vec::new(),
        )),
        Box::new(step),
    )
}
