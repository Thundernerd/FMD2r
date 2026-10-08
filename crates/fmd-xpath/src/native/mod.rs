//! The `native` backend: a pure-Rust reimplementation of the parts of internettools FMD2's
//! modules use. HTML is parsed with html5ever into the tree internettools would build
//! (as close as practical, see README.md), and XPath is evaluated with internettools'
//! semantics and extensions (docs/xpath-extensions.md).

mod css;
mod dom;
mod eval;
mod functions;
mod json;
mod syntax;
mod value;

use std::any::Any;
use std::rc::Rc;

use crate::{Document, Result, XPathEngine, XPathValue};
use dom::Dom;
use eval::{Evaluator, Focus};
use value::{Item, NodeRef, Seq};

/// The `native` backend. Documents and values are neither `Send` nor `Sync`, like the `fpc`
/// backend's.
#[derive(Debug, Default, Clone, Copy)]
pub struct NativeEngine;

impl XPathEngine for NativeEngine {
    /// Empty input gives a document without a tree, as `TXQueryEngineHTML.Create('')` never
    /// parses (baseunits/XQueryEngineHTML.pas:397-398).
    fn parse(&self, html: &[u8]) -> Result<Box<dyn Document>> {
        let root = (!html.is_empty()).then(|| NodeRef {
            dom: Rc::new(Dom::parse(html)),
            id: 0,
        });
        Ok(Box::new(NativeDocument { root }))
    }
}

/// A parsed document.
struct NativeDocument {
    root: Option<NodeRef>,
}

impl Document for NativeDocument {
    fn eval(&self, expr: &str, context: Option<&dyn XPathValue>, css: bool) -> Box<dyn XPathValue> {
        let context = match context {
            None => self.root.clone().map(Item::Node),
            Some(value) => match value.as_any().downcast_ref::<NativeValue>() {
                // `IXQValue` as a context item: a sequence's first item.
                Some(value) => value.0.first().cloned(),
                None => return Box::new(NativeValue(Vec::new())),
            },
        };
        let parsed = if css {
            css::translate(expr)
        } else {
            syntax::parse(expr)
        };
        let result = parsed.and_then(|parsed| {
            let result = Evaluator::default().eval(&parsed, &Focus::of(context))?;
            Ok(if css {
                eval::document_order(result)
            } else {
                result
            })
        });
        Box::new(NativeValue(result.unwrap_or_default()))
    }
}

/// An `IXQValue`: a sequence.
struct NativeValue(Seq);

impl NativeValue {
    fn first_node(&self) -> Option<&NodeRef> {
        self.0.first().and_then(Item::as_node)
    }
}

impl XPathValue for NativeValue {
    fn count(&self) -> i64 {
        i64::try_from(self.0.len()).unwrap_or(i64::MAX)
    }

    fn get(&self, index: i64) -> Box<dyn XPathValue> {
        let item = usize::try_from(index)
            .ok()
            .and_then(|i| i.checked_sub(1))
            .and_then(|i| self.0.get(i));
        Box::new(NativeValue(item.cloned().into_iter().collect()))
    }

    fn is_undefined(&self) -> bool {
        self.0.is_empty()
    }

    fn string(&self) -> String {
        value::seq_string(&self.0)
    }

    fn inner_html(&self) -> String {
        self.first_node()
            .map(NodeRef::inner_html)
            .unwrap_or_default()
    }

    fn outer_html(&self) -> String {
        self.first_node()
            .map(NodeRef::outer_html)
            .unwrap_or_default()
    }

    fn inner_text(&self) -> String {
        self.first_node()
            .map(NodeRef::inner_text)
            .unwrap_or_default()
    }

    fn attribute(&self, name: &str) -> String {
        self.first_node()
            .and_then(|node| node.dom.attribute(node.id, name))
            .unwrap_or_default()
            .to_owned()
    }

    /// `IXQValue.getProperty` reads the first item's property; anything but an object has none.
    fn property(&self, name: &str) -> Box<dyn XPathValue> {
        let value = match self.0.first() {
            Some(Item::Object(object)) => object.get(name).cloned().unwrap_or_default(),
            _ => Vec::new(),
        };
        Box::new(NativeValue(value))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
