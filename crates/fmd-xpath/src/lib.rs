//! `XPathEngine` trait and its backends: `fpc` (FFI to `libfmdxpath.so`) and `native` (pure Rust).
//!
//! The API is shaped after FMD2's `TXQueryEngineHTML` (baseunits/XQueryEngineHTML.pas) and the
//! `IXQValue` operations its Lua binding exposes (baseunits/lua/LuaIXQValue.pas:37-160). It is
//! object-safe, so callers hold a `dyn XPathEngine` and the backend is chosen once.

use std::any::Any;
use std::rc::Rc;

use corpus::{Entry, Origin, Step};
pub use diff::{Mismatch, Normalized, NormalizedItem, Report, diff, feature};

pub mod corpus;
mod diff;
#[cfg(feature = "fpc")]
pub mod fpc;
#[cfg(feature = "native")]
pub mod native;

/// An XPath backend, as the `xpath.backend` setting names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// FMD2's own engine (internettools) through `libfmdxpath.so`.
    Fpc,
    /// The pure-Rust engine.
    Native,
}

impl Backend {
    /// The backend's engine, or `None` when this build leaves it out (its cargo feature is off).
    pub fn engine(self) -> Option<Rc<dyn XPathEngine>> {
        match self {
            #[cfg(feature = "fpc")]
            Backend::Fpc => Some(Rc::new(fpc::FpcEngine)),
            #[cfg(feature = "native")]
            Backend::Native => Some(Rc::new(native::NativeEngine)),
            #[allow(unreachable_patterns)] // Unreachable when both backends are built.
            _ => None,
        }
    }
}

impl Default for Backend {
    /// `native`, since the differential corpus showed parity with `fpc` (T35,
    /// fixtures/xpath-corpus).
    fn default() -> Self {
        Backend::Native
    }
}

/// Errors raised by an XPath backend.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The backend could not parse the document at all (not a syntax problem: HTML is always
    /// repaired, baseunits/XQueryEngineHTML.pas:390-392).
    #[error("could not parse the HTML document: {0}")]
    Parse(String),
}

/// Result type of the `fmd-xpath` crate.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// An XPath/XQuery engine configured like FMD2's `TXQueryEngineHTML.Create`
/// (baseunits/XQueryEngineHTML.pas:384-400).
pub trait XPathEngine {
    /// Parses an HTML document: HTML model, missing start and end tags repaired, text not
    /// trimmed, no comments or processing instructions (baseunits/XQueryEngineHTML.pas:390-396).
    /// Empty input gives an empty document.
    fn parse(&self, html: &[u8]) -> Result<Box<dyn Document>>;
}

impl<E: XPathEngine + ?Sized> XPathEngine for Rc<E> {
    fn parse(&self, html: &[u8]) -> Result<Box<dyn Document>> {
        (**self).parse(html)
    }
}

/// A parsed document that expressions are evaluated against.
pub trait Document {
    /// Evaluates an XPath/XQuery expression, or a CSS selector when `css` is set, against the
    /// document or, when given, against `context`. Any error yields an empty value, as FMD2's
    /// `Eval` swallows every exception (baseunits/XQueryEngineHTML.pas:252-284).
    ///
    /// `context` must come from the same backend; any other value is an error (an empty value).
    fn eval(&self, expr: &str, context: Option<&dyn XPathValue>, css: bool) -> Box<dyn XPathValue>;
}

/// An `IXQValue`: a sequence, node, string, number, JSON object, ...
///
/// Strings come back as UTF-8; bytes that aren't valid UTF-8 are replaced.
pub trait XPathValue: Any {
    /// Number of items; 0 for the empty sequence, 1 for a single item (`IXQValue.Count`,
    /// baseunits/lua/LuaIXQValue.pas:74-78).
    fn count(&self) -> i64;
    /// The `index`-th item, 1-based; out of range gives an empty value (`IXQValue.get`,
    /// baseunits/lua/LuaIXQValue.pas:130).
    fn get(&self, index: i64) -> Box<dyn XPathValue>;
    /// Whether this is the empty sequence (`pvkUndefined`, baseunits/lua/LuaIXQValue.pas:101).
    fn is_undefined(&self) -> bool;
    /// The value's kind (`IXQValue.kind`, internettools data/xquery.pas:103-109, which the
    /// `Get()` iterator checks, baseunits/lua/LuaIXQValue.pas:101).
    fn kind(&self) -> Kind;
    /// `IXQValue.toString`; for nodes the text content, trimmed
    /// (baseunits/lua/LuaIXQValue.pas:37-41).
    fn string(&self) -> String;
    /// The node's inner HTML; empty on a non-node (baseunits/lua/LuaIXQValue.pas:56-60).
    fn inner_html(&self) -> String;
    /// The node's outer HTML; empty on a non-node (baseunits/lua/LuaIXQValue.pas:62-66).
    fn outer_html(&self) -> String;
    /// The node's human-readable text; empty on a non-node
    /// (baseunits/lua/LuaIXQValue.pas:68-72).
    fn inner_text(&self) -> String;
    /// A node attribute; empty when missing or on a non-node
    /// (baseunits/lua/LuaIXQValue.pas:43-48).
    fn attribute(&self, name: &str) -> String;
    /// A JSON object property; an empty value when missing or on a non-object
    /// (baseunits/lua/LuaIXQValue.pas:50-54).
    fn property(&self, name: &str) -> Box<dyn XPathValue>;
    /// The value as `Any`, so a backend can recognise its own values passed back as context.
    fn as_any(&self) -> &dyn Any;
}

/// The primary type of a value: internettools' `TXQValueKind` (data/xquery.pas:103-109), as
/// `fx_kind` exports it (crates/xpath-fpc/fmdxpath.h).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// The empty sequence.
    Undefined,
    Boolean,
    Int64,
    /// JSON `null`.
    Null,
    Node,
    /// A sequence of more than one item.
    Sequence,
    /// A JSON array.
    Array,
    Double,
    String,
    Decimal,
    Binary,
    QName,
    DateTime,
    /// A JSON object.
    Object,
    Function,
}

impl std::fmt::Display for Kind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Kind::Undefined => "undefined",
            Kind::Boolean => "boolean",
            Kind::Int64 => "int64",
            Kind::Null => "null",
            Kind::Node => "node",
            Kind::Sequence => "sequence",
            Kind::Array => "array",
            Kind::Double => "double",
            Kind::String => "string",
            Kind::Decimal => "decimal",
            Kind::Binary => "binary",
            Kind::QName => "qname",
            Kind::DateTime => "datetime",
            Kind::Object => "object",
            Kind::Function => "function",
        })
    }
}

/// One evaluation, as seen by a [`LoggingEngine`] hook: the inputs of the XPath differential
/// corpus (docs/plan.md, "XPath differential tests").
#[derive(Debug, Clone, Copy)]
pub struct Query<'a> {
    /// The expression or CSS selector.
    pub expression: &'a str,
    /// Whether `expression` is a CSS selector.
    pub css: bool,
    /// [`document_hash`] of the document the expression ran against.
    pub document_hash: u64,
    /// The document's bytes, as parsed.
    pub document: &'a [u8],
    /// Where the context value came from, when the expression ran against one.
    pub context: Option<&'a Origin>,
}

/// A stable hash of a document's bytes (64-bit FNV-1a), the same across runs, builds and
/// platforms, so corpus entries can name the document they ran against.
pub fn document_hash(html: &[u8]) -> u64 {
    html.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// Wraps an engine and reports every evaluation to a hook, without changing any result. Its
/// values remember where they came from, so an evaluation against one can be reported with its
/// context's [`Origin`].
pub struct LoggingEngine<E> {
    inner: E,
    hook: Rc<dyn Fn(&Query)>,
}

impl<E: XPathEngine> LoggingEngine<E> {
    /// Logs every evaluation on documents `inner` parses to `hook`.
    pub fn new(inner: E, hook: impl Fn(&Query) + 'static) -> Self {
        LoggingEngine {
            inner,
            hook: Rc::new(hook),
        }
    }
}

impl<E: XPathEngine> XPathEngine for LoggingEngine<E> {
    fn parse(&self, html: &[u8]) -> Result<Box<dyn Document>> {
        Ok(Box::new(LoggingDocument {
            inner: self.inner.parse(html)?,
            html: Rc::from(html),
            hash: document_hash(html),
            hook: self.hook.clone(),
        }))
    }
}

/// A document of a [`LoggingEngine`].
struct LoggingDocument {
    inner: Box<dyn Document>,
    html: Rc<[u8]>,
    hash: u64,
    hook: Rc<dyn Fn(&Query)>,
}

impl Document for LoggingDocument {
    fn eval(&self, expr: &str, context: Option<&dyn XPathValue>, css: bool) -> Box<dyn XPathValue> {
        // Our own values are unwrapped for the inner engine; any other value goes through as is,
        // with no origin to report.
        let (context, origin) = match context {
            Some(value) => match value.as_any().downcast_ref::<LoggingValue>() {
                Some(logged) => (Some(logged.inner.as_ref()), Some(&logged.origin)),
                None => (Some(value), None),
            },
            None => (None, None),
        };
        (self.hook)(&Query {
            expression: expr,
            css,
            document_hash: self.hash,
            document: &self.html,
            context: origin.map(Rc::as_ref),
        });
        Box::new(LoggingValue {
            inner: self.inner.eval(expr, context, css),
            origin: Rc::new(Origin {
                entry: Entry {
                    document: self.hash,
                    expression: expr.to_owned(),
                    css,
                    context: origin.map(|o| Box::new(Origin::clone(o))),
                },
                path: Vec::new(),
            }),
        })
    }
}

/// A value of a [`LoggingEngine`]: the inner engine's value and where it came from.
struct LoggingValue {
    inner: Box<dyn XPathValue>,
    origin: Rc<Origin>,
}

impl LoggingValue {
    /// `value`, reached from this one by `step`.
    fn step(&self, value: Box<dyn XPathValue>, step: Step) -> Box<dyn XPathValue> {
        let mut origin = Origin::clone(&self.origin);
        origin.path.push(step);
        Box::new(LoggingValue {
            inner: value,
            origin: Rc::new(origin),
        })
    }
}

impl XPathValue for LoggingValue {
    fn count(&self) -> i64 {
        self.inner.count()
    }
    fn get(&self, index: i64) -> Box<dyn XPathValue> {
        self.step(self.inner.get(index), Step::Item(index))
    }
    fn is_undefined(&self) -> bool {
        self.inner.is_undefined()
    }
    fn kind(&self) -> Kind {
        self.inner.kind()
    }
    fn string(&self) -> String {
        self.inner.string()
    }
    fn inner_html(&self) -> String {
        self.inner.inner_html()
    }
    fn outer_html(&self) -> String {
        self.inner.outer_html()
    }
    fn inner_text(&self) -> String {
        self.inner.inner_text()
    }
    fn attribute(&self, name: &str) -> String {
        self.inner.attribute(name)
    }
    fn property(&self, name: &str) -> Box<dyn XPathValue> {
        self.step(self.inner.property(name), Step::Property(name.to_owned()))
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
