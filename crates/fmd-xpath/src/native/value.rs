//! Items and sequences, with internettools' conversions and comparisons
//! (internettools data/xquery.pas, data/xquery_types.inc).

use std::cmp::Ordering;
use std::rc::Rc;

use indexmap::IndexMap;
use rust_decimal::Decimal;
use rust_decimal::prelude::{FromPrimitive, ToPrimitive};

use super::dom::{Dom, NodeId, NodeKind};

/// An evaluation error. FMD2 swallows every one (baseunits/XQueryEngineHTML.pas:252-284), so
/// only the fact that one happened matters; the message helps debugging.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub(crate) struct XPathError(pub(crate) String);

pub(crate) type XResult<T> = Result<T, XPathError>;

pub(crate) fn err<T>(message: impl Into<String>) -> XResult<T> {
    Err(XPathError(message.into()))
}

/// A sequence of items.
pub(crate) type Seq = Vec<Item>;

/// A node of a parsed document.
#[derive(Clone)]
pub(crate) struct NodeRef {
    pub(crate) dom: Rc<Dom>,
    pub(crate) id: NodeId,
}

impl NodeRef {
    pub(crate) fn kind(&self) -> &NodeKind {
        &self.dom.node(self.id).kind
    }

    pub(crate) fn with_id(&self, id: NodeId) -> NodeRef {
        NodeRef {
            dom: self.dom.clone(),
            id,
        }
    }

    /// The same node of the same document.
    pub(crate) fn same(&self, other: &NodeRef) -> bool {
        Rc::ptr_eq(&self.dom, &other.dom) && self.id == other.id
    }

    /// Document order; nodes of different documents order by their documents' addresses.
    pub(crate) fn order(&self, other: &NodeRef) -> Ordering {
        (Rc::as_ptr(&self.dom) as usize, self.id).cmp(&(Rc::as_ptr(&other.dom) as usize, other.id))
    }

    pub(crate) fn inner_html(&self) -> String {
        self.dom.html(self.id, false)
    }

    pub(crate) fn outer_html(&self) -> String {
        self.dom.html(self.id, true)
    }

    pub(crate) fn inner_text(&self) -> String {
        self.dom.inner_text(self.id)
    }

    /// The node's string, trimmed: internettools trims every node-to-string conversion
    /// (`XQGlobalTrimNodes`, internettools data/xquery.pas:3141-3145, :5281-5292).
    pub(crate) fn string(&self) -> String {
        trim(&self.dom.deep_text(self.id)).to_owned()
    }
}

/// What kind of string an item is; comparisons treat them alike, `instance of` doesn't.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum StrType {
    String,
    Untyped,
    AnyUri,
}

/// A JSON object: insertion-ordered properties.
pub(crate) type Object = IndexMap<String, Seq>;

/// One item.
#[derive(Clone)]
pub(crate) enum Item {
    Node(NodeRef),
    Str(Rc<str>, StrType),
    Int(i64),
    Dec(Decimal),
    Dbl(f64),
    Bool(bool),
    /// JSON `null`.
    Null,
    Object(Rc<Object>),
    /// A JSON array or XPath 3.1 array; each member is a sequence.
    Array(Rc<Vec<Seq>>),
}

impl Item {
    pub(crate) fn str(s: impl Into<Rc<str>>) -> Item {
        Item::Str(s.into(), StrType::String)
    }

    pub(crate) fn untyped(s: impl Into<Rc<str>>) -> Item {
        Item::Str(s.into(), StrType::Untyped)
    }

    pub(crate) fn as_node(&self) -> Option<&NodeRef> {
        match self {
            Item::Node(node) => Some(node),
            _ => None,
        }
    }

    pub(crate) fn is_numeric(&self) -> bool {
        matches!(self, Item::Int(_) | Item::Dec(_) | Item::Dbl(_))
    }

    /// `IXQValue.toString`: a node's trimmed text, an array's members' strings concatenated,
    /// `""` for an object.
    pub(crate) fn string(&self) -> String {
        match self {
            Item::Node(node) => node.string(),
            Item::Str(s, _) => s.to_string(),
            Item::Int(i) => i.to_string(),
            Item::Dec(d) => decimal_string(*d),
            Item::Dbl(f) => double_string(*f),
            Item::Bool(b) => b.to_string(),
            Item::Null => "null".to_owned(),
            Item::Object(_) => String::new(),
            Item::Array(members) => members.iter().map(|m| seq_string(m)).collect(),
        }
    }

    /// Atomizes the item: a node becomes its (trimmed) string as `xs:untypedAtomic`.
    pub(crate) fn atomize(&self) -> Item {
        match self {
            Item::Node(node) => Item::untyped(node.string()),
            other => other.clone(),
        }
    }

    /// `toBooleanEffective` of one item (internettools data/xquery_types.inc:410-470).
    pub(crate) fn ebv(&self) -> XResult<bool> {
        Ok(match self {
            Item::Node(_) | Item::Object(_) | Item::Array(_) => true,
            Item::Str(s, _) => !s.is_empty(),
            Item::Int(i) => *i != 0,
            Item::Dec(d) => !d.is_zero(),
            Item::Dbl(f) => !f.is_nan() && *f != 0.0,
            Item::Bool(b) => *b,
            Item::Null => false,
        })
    }

    /// The item as a double (`toDouble`): strings are parsed after trimming, failures are NaN.
    pub(crate) fn to_double(&self) -> f64 {
        match self {
            Item::Int(i) => *i as f64,
            Item::Dec(d) => d.to_f64().unwrap_or(f64::NAN),
            Item::Dbl(f) => *f,
            Item::Bool(b) => f64::from(u8::from(*b)),
            Item::Null | Item::Object(_) | Item::Array(_) => f64::NAN,
            other => parse_double(&other.string()).unwrap_or(f64::NAN),
        }
    }
}

/// `IXQValue.toString` of a sequence: its items' strings concatenated.
pub(crate) fn seq_string(seq: &[Item]) -> String {
    seq.iter().map(Item::string).collect()
}

/// The effective boolean value of a sequence (internettools data/xquery_types.inc:410-422).
pub(crate) fn ebv(seq: &[Item]) -> XResult<bool> {
    match seq {
        [] => Ok(false),
        [item] => item.ebv(),
        [Item::Node(_) | Item::Object(_) | Item::Array(_), ..] => Ok(true),
        _ => err("FORG0006: a sequence starting with an atomic value has no boolean value"),
    }
}

/// Trims every character up to `' '`, like internettools' `strTrim` (bbutils.pas:2725).
pub(crate) fn trim(s: &str) -> &str {
    s.trim_matches(|c: char| c <= ' ')
}

/// Parses an XPath double literal or cast source (`INF`, `-INF`, `NaN` included).
pub(crate) fn parse_double(s: &str) -> Option<f64> {
    let s = trim(s);
    match s {
        "INF" | "+INF" => return Some(f64::INFINITY),
        "-INF" => return Some(f64::NEG_INFINITY),
        "NaN" => return Some(f64::NAN),
        _ => {}
    }
    let valid = !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '+' | '-'))
        && s.chars().any(|c| c.is_ascii_digit());
    if valid { s.parse().ok() } else { None }
}

/// Parses an `xs:decimal` (or integer) lexical form.
pub(crate) fn parse_decimal(s: &str) -> Option<Decimal> {
    let s = trim(s);
    let digits = s.strip_prefix(['+', '-']).unwrap_or(s);
    let valid = !digits.is_empty()
        && digits.chars().all(|c| c.is_ascii_digit() || c == '.')
        && digits.chars().filter(|&c| c == '.').count() <= 1
        && digits.chars().any(|c| c.is_ascii_digit());
    if !valid {
        return None;
    }
    Decimal::from_str_exact(s.strip_prefix('+').unwrap_or(s)).ok()
}

/// An `xs:decimal`'s canonical string: no exponent, no trailing zeros.
pub(crate) fn decimal_string(d: Decimal) -> String {
    let d = d.normalize();
    if d.is_zero() {
        "0".to_owned()
    } else {
        d.to_string()
    }
}

/// An `xs:double`'s canonical string (XPath 3.1 casting rules, as internettools prints them):
/// plain notation for 1e-6 <= |x| < 1e6, otherwise `1.0E10` style.
pub(crate) fn double_string(f: f64) -> String {
    if f.is_nan() {
        return "NaN".to_owned();
    }
    if f.is_infinite() {
        return if f > 0.0 { "INF" } else { "-INF" }.to_owned();
    }
    if f == 0.0 {
        return if f.is_sign_negative() { "-0" } else { "0" }.to_owned();
    }
    let abs = f.abs();
    if (1e-6..1e6).contains(&abs) {
        return format!("{f}");
    }
    let s = format!("{f:e}");
    let (mantissa, exponent) = s.split_once('e').unwrap_or((&s, "0"));
    let mantissa = if mantissa.contains('.') {
        mantissa.to_owned()
    } else {
        format!("{mantissa}.0")
    };
    format!("{mantissa}E{exponent}")
}

/// Converts a double to a decimal, if finite.
pub(crate) fn double_to_decimal(f: f64) -> Option<Decimal> {
    Decimal::from_f64(f)
}

/// The result of comparing two atomic values (`TXQCompareResult`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Cmp {
    Less,
    Equal,
    Greater,
    /// One side is NaN.
    NaN,
    /// One side is the empty sequence.
    Empty,
}

impl Cmp {
    fn from_ordering(o: Ordering) -> Cmp {
        match o {
            Ordering::Less => Cmp::Less,
            Ordering::Equal => Cmp::Equal,
            Ordering::Greater => Cmp::Greater,
        }
    }

    fn invert(self) -> Cmp {
        match self {
            Cmp::Less => Cmp::Greater,
            Cmp::Greater => Cmp::Less,
            other => other,
        }
    }
}

/// The default collation, `case-insensitive-clever` (internettools data/xquery.pas:10507):
/// ASCII case-insensitive, with digit runs compared as numbers
/// (`striCompareClever`, bbutils.pas:4559-4605).
pub(crate) fn collation_compare(a: &str, b: &str) -> Ordering {
    let a = a.to_ascii_lowercase();
    let b = b.to_ascii_lowercase();
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        if a[i].is_ascii_digit() && b[j].is_ascii_digit() {
            let (iz, jz) = (i, j);
            while i < a.len() && a[i] == b'0' {
                i += 1;
            }
            while j < b.len() && b[j] == b'0' {
                j += 1;
            }
            let (ib, jb) = (i, j);
            while i < a.len() && a[i].is_ascii_digit() {
                i += 1;
            }
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            let longer = (i - ib).cmp(&(j - jb));
            if longer != Ordering::Equal {
                return longer;
            }
            let digits = a[ib..i].cmp(&b[jb..j]);
            if digits != Ordering::Equal {
                return digits;
            }
            let zeros = (i - iz).cmp(&(j - jz));
            if zeros != Ordering::Equal {
                return zeros;
            }
        } else {
            if a[i] != b[j] {
                return a[i].cmp(&b[j]);
            }
            i += 1;
            j += 1;
        }
    }
    (a.len() - i).cmp(&(b.len() - j))
}

/// Finds `needle` in `haystack` with the default collation (ASCII case-insensitive,
/// `striIndexOf`); returns the byte offset.
pub(crate) fn collation_find(haystack: &str, needle: &str) -> Option<usize> {
    let h = haystack.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() {
        return Some(0);
    }
    (0..=h.len().checked_sub(n.len())?).find(|&i| h[i..i + n.len()].eq_ignore_ascii_case(n))
}

/// Compares two items like `TXQStaticContext.compareCommon` (internettools
/// data/xquery.pas:7145-7372). `cast_unknown_to_string` is set for value comparisons (`eq`),
/// which compare a string with anything as strings; general comparisons (`=`) convert the
/// string to the other side's type instead.
pub(crate) fn compare(a: &Item, b: &Item, cast_unknown_to_string: bool) -> XResult<Cmp> {
    use Item::*;
    let stringish = |i: &Item| matches!(i, Str(..) | Node(_));
    if let (Array(_), _) | (_, Array(_)) = (a, b) {
        return compare_arrays(a, b, cast_unknown_to_string);
    }
    match (a, b) {
        (Bool(x), Bool(y)) => return Ok(Cmp::from_ordering(x.cmp(y))),
        (Null, Null) => return Ok(Cmp::Equal),
        (Object(_), _) | (_, Object(_)) => return err("XPTY0004: objects are incomparable"),
        _ => {}
    }
    if stringish(a) && stringish(b) {
        return Ok(Cmp::from_ordering(collation_compare(
            &a.string(),
            &b.string(),
        )));
    }
    if let Null = b {
        return Ok(Cmp::Greater);
    }
    if let Null = a {
        return Ok(Cmp::Less);
    }
    if cast_unknown_to_string && (stringish(a) || stringish(b)) {
        return Ok(Cmp::from_ordering(collation_compare(
            &a.string(),
            &b.string(),
        )));
    }
    if a.is_numeric() && b.is_numeric() {
        return Ok(compare_numbers(a, b));
    }
    if a.is_numeric() || b.is_numeric() {
        // The other side is a string or boolean, converted to a number.
        let (num, other, inverted) = if a.is_numeric() {
            (a, b, false)
        } else {
            (b, a, true)
        };
        let converted = match other {
            Bool(x) => Int(i64::from(*x)),
            _ => string_to_number(&other.string(), num)?,
        };
        let result = compare_numbers(num, &converted);
        return Ok(if inverted { result.invert() } else { result });
    }
    match (a, b) {
        (Bool(x), other) | (other, Bool(x)) => {
            let y = string_to_bool(&other.string())?;
            let result = Cmp::from_ordering(x.cmp(&y));
            Ok(if matches!(a, Bool(_)) {
                result
            } else {
                result.invert()
            })
        }
        _ => Ok(Cmp::from_ordering(collation_compare(
            &a.string(),
            &b.string(),
        ))),
    }
}

fn compare_arrays(a: &Item, b: &Item, cast: bool) -> XResult<Cmp> {
    let single = |i: &Item| -> XResult<Option<Item>> {
        match i {
            Item::Array(members) => {
                let mut items = members.iter().flatten();
                let first = items.next().cloned();
                if items.next().is_some() {
                    return err("XPTY0004: expected a singleton");
                }
                Ok(first)
            }
            other => Ok(Some(other.clone())),
        }
    };
    match (single(a)?, single(b)?) {
        (Some(a), Some(b)) => compare(&a, &b, cast),
        _ => Ok(Cmp::Empty),
    }
}

/// Converts a string compared with the number `like` (trimmed; `compareAsPossibleInt64` and
/// `compareAsBigDecimals`): an integer or decimal, a double when it holds `N` (`NaN`, `INF`),
/// otherwise an error (FORG0001).
fn string_to_number(s: &str, like: &Item) -> XResult<Item> {
    let s = trim(s);
    if matches!(like, Item::Dbl(_)) || s.contains('N') {
        return match parse_double(s) {
            Some(f) => Ok(Item::Dbl(f)),
            None => err(format!("FORG0001: cannot convert {s:?} to a number")),
        };
    }
    if let Ok(i) = s.parse::<i64>() {
        return Ok(Item::Int(i));
    }
    match parse_decimal(s) {
        Some(d) => Ok(Item::Dec(d)),
        None => err(format!("FORG0001: cannot convert {s:?} to a decimal")),
    }
}

/// `xs:boolean` from a string: `true`/`1` or `false`/`0`.
pub(crate) fn string_to_bool(s: &str) -> XResult<bool> {
    match trim(s) {
        "true" | "1" => Ok(true),
        "false" | "0" => Ok(false),
        other => err(format!("FORG0001: cannot convert {other:?} to a boolean")),
    }
}

fn compare_numbers(a: &Item, b: &Item) -> Cmp {
    match (a, b) {
        (Item::Int(x), Item::Int(y)) => Cmp::from_ordering(x.cmp(y)),
        (Item::Dbl(_), _) | (_, Item::Dbl(_)) => {
            let (x, y) = (a.to_double(), b.to_double());
            match x.partial_cmp(&y) {
                Some(o) => Cmp::from_ordering(o),
                None => Cmp::NaN,
            }
        }
        _ => match (to_decimal(a), to_decimal(b)) {
            (Some(x), Some(y)) => Cmp::from_ordering(x.cmp(&y)),
            _ => Cmp::NaN,
        },
    }
}

/// A numeric item as a decimal.
pub(crate) fn to_decimal(i: &Item) -> Option<Decimal> {
    match i {
        Item::Int(x) => Some(Decimal::from(*x)),
        Item::Dec(d) => Some(*d),
        Item::Dbl(f) => double_to_decimal(*f),
        _ => None,
    }
}
