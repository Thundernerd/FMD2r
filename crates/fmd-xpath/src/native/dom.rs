//! The document tree: html5ever's parse, reshaped into the tree internettools' XPath sees.
//!
//! FMD2 parses with `pmHTML`, repairing missing tags, keeping whitespace, and dropping comments
//! and processing instructions (baseunits/XQueryEngineHTML.pas:384-400). html5ever's repair
//! follows the HTML5 algorithm, which internettools approximates (README.md, "Known
//! differences"). Comments and processing instructions never enter the tree, so the text around
//! a comment is one text node, as in internettools.

use std::borrow::Cow;
use std::cell::RefCell;

use html5ever::interface::{ElemName, ElementFlags, NodeOrText, QuirksMode, TreeSink};
use html5ever::tendril::{StrTendril, TendrilSink};
use html5ever::{Attribute, LocalName, Namespace, QualName, ns};

/// Index of a node in its [`Dom`]; ids follow document order (an element, its attributes, then
/// its children).
pub(crate) type NodeId = usize;

/// What a node is.
pub(crate) enum NodeKind {
    Document,
    Element(String),
    Attribute { name: String, value: String },
    Text(String),
}

/// One node of a [`Dom`].
pub(crate) struct Node {
    pub(crate) kind: NodeKind,
    pub(crate) parent: Option<NodeId>,
    pub(crate) attributes: Vec<NodeId>,
    pub(crate) children: Vec<NodeId>,
    /// The last id inside this node's subtree (itself when it has none).
    pub(crate) last: NodeId,
}

/// A parsed document. The document node is id 0.
pub(crate) struct Dom {
    nodes: Vec<Node>,
}

impl Dom {
    /// Parses `html` (invalid UTF-8 replaced) like `TTreeParser.parseTree`.
    pub(crate) fn parse(html: &[u8]) -> Dom {
        let sink = Sink::default();
        let sink = html5ever::parse_document(sink, Default::default())
            .from_utf8()
            .one(html);
        let prefix = leading_whitespace(html);
        if !prefix.is_empty() {
            let text = sink.new_node(SinkData::Text(prefix));
            let mut nodes = sink.nodes.borrow_mut();
            nodes[0].children.insert(0, text);
            nodes[text].parent = Some(0);
        }
        sink.into_dom()
    }

    pub(crate) fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id]
    }

    /// The text of every text node under `id`, in document order (`TTreeNode.deepNodeText`,
    /// internettools data/simplehtmltreeparser.pas:1141-1158).
    pub(crate) fn deep_text(&self, id: NodeId) -> String {
        match &self.nodes[id].kind {
            NodeKind::Text(text) => text.clone(),
            NodeKind::Attribute { value, .. } => value.clone(),
            NodeKind::Document | NodeKind::Element(_) => {
                let mut text = String::new();
                for node in &self.nodes[id + 1..=self.nodes[id].last] {
                    if let NodeKind::Text(t) = &node.kind {
                        text.push_str(t);
                    }
                }
                text
            }
        }
    }

    /// The element name, or the attribute name; empty for other nodes.
    pub(crate) fn name(&self, id: NodeId) -> &str {
        match &self.nodes[id].kind {
            NodeKind::Element(name) | NodeKind::Attribute { name, .. } => name,
            NodeKind::Document | NodeKind::Text(_) => "",
        }
    }

    /// The value of the attribute `name` (ASCII case-insensitive, `TTreeNode.getAttribute`,
    /// internettools data/simplehtmltreeparser.pas:1335-1338).
    pub(crate) fn attribute(&self, id: NodeId, name: &str) -> Option<&str> {
        self.nodes[id]
            .attributes
            .iter()
            .find_map(|&a| match &self.nodes[a].kind {
                NodeKind::Attribute { name: n, value } if n.eq_ignore_ascii_case(name) => {
                    Some(value.as_str())
                }
                _ => None,
            })
    }
}

impl Dom {
    /// The text nodes directly or indirectly under `id`, in document order.
    pub(crate) fn text_nodes(&self, id: NodeId) -> Vec<&str> {
        self.nodes[id + 1..=self.nodes[id].last]
            .iter()
            .filter_map(|node| match &node.kind {
                NodeKind::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect()
    }

    /// The node and its subtree as HTML (`TTreeNode.outerHTML`), or only its children
    /// (`innerHTML`), serialized like internettools' `serializeNodes`
    /// (internettools data/xquery__serialization_nodes.pas:386-700).
    pub(crate) fn html(&self, id: NodeId, outer: bool) -> String {
        let mut out = String::new();
        match &self.nodes[id].kind {
            NodeKind::Document => self.inner(id, false, &mut out),
            NodeKind::Element(_) if !outer => self.inner(id, false, &mut out),
            NodeKind::Element(_) => self.outer(id, false, &mut out),
            NodeKind::Text(text) if outer => escape_text(text, &mut out),
            NodeKind::Text(_) | NodeKind::Attribute { .. } => {}
        }
        out
    }

    fn inner(&self, id: NodeId, cdata: bool, out: &mut String) {
        for &child in &self.nodes[id].children {
            self.outer(child, cdata, out);
        }
    }

    fn outer(&self, id: NodeId, cdata: bool, out: &mut String) {
        let node = &self.nodes[id];
        match &node.kind {
            NodeKind::Text(text) if cdata => out.push_str(text),
            NodeKind::Text(text) => escape_text(text, out),
            NodeKind::Element(name) => {
                out.push('<');
                out.push_str(name);
                for &a in &node.attributes {
                    if let NodeKind::Attribute { name: attr, value } = &self.nodes[a].kind {
                        out.push(' ');
                        out.push_str(attr);
                        if attr.eq_ignore_ascii_case(value) && is_boolean_attribute(name, attr) {
                            continue;
                        }
                        out.push_str("=\"");
                        if cdata {
                            out.push_str(value);
                        } else {
                            escape_attribute(value, out);
                        }
                        out.push('"');
                    }
                }
                if node.children.is_empty() && is_childless(name) {
                    out.push('>');
                    return;
                }
                out.push('>');
                self.inner(id, cdata || is_raw_text(name), out);
                out.push_str("</");
                out.push_str(name);
                out.push('>');
            }
            NodeKind::Document => self.inner(id, cdata, out),
            NodeKind::Attribute { .. } => {}
        }
    }

    /// A human-readable text (`TTreeNode.innerText`, internettools
    /// data/simplehtmltreeparser.pas:1243-1320): whitespace collapsed, `<br>` and `<tr>` as line
    /// breaks, `<p>` as two, cells as tabs, hidden and non-content elements skipped.
    pub(crate) fn inner_text(&self, id: NodeId) -> String {
        match &self.nodes[id].kind {
            NodeKind::Text(text) => return trim_pascal(text).to_owned(),
            NodeKind::Attribute { .. } => return String::new(),
            NodeKind::Document | NodeKind::Element(_) => {}
        }
        let mut out = String::new();
        let mut trailing_space = false;
        self.inner_text_walk(id, &mut out, &mut trailing_space);
        trim_pascal(&out).to_owned()
    }

    fn inner_text_walk(&self, id: NodeId, out: &mut String, trailing_space: &mut bool) {
        let node = &self.nodes[id];
        match &node.kind {
            NodeKind::Text(text) => {
                if text.is_empty() {
                    return;
                }
                let normalized = trim_and_normalize(text);
                if normalized.is_empty() {
                    *trailing_space = true;
                    return;
                }
                let ends_with_space = out.as_bytes().last().is_some_and(|&b| b <= b' ');
                let starts_with_space = text.as_bytes().first().is_some_and(|&b| b <= b' ');
                if (*trailing_space || starts_with_space) && !ends_with_space {
                    out.push(' ');
                }
                out.push_str(&normalized);
                *trailing_space = text.as_bytes().last().is_some_and(|&b| b <= b' ');
            }
            NodeKind::Element(name) => {
                if self.skips_text(id, name) {
                    return;
                }
                let before = out.len();
                match name.to_ascii_lowercase().as_str() {
                    "br" => out.push('\n'),
                    "td" | "th" => out.push('\t'),
                    "tr" => out.push('\n'),
                    "p" => out.push_str("\n\n"),
                    _ => {}
                }
                if out.len() != before {
                    *trailing_space = false;
                }
                for &child in &node.children {
                    self.inner_text_walk(child, out, trailing_space);
                }
            }
            NodeKind::Document => {
                for &child in &node.children {
                    self.inner_text_walk(child, out, trailing_space);
                }
            }
            NodeKind::Attribute { .. } => {}
        }
    }

    fn skips_text(&self, id: NodeId, name: &str) -> bool {
        const SKIPPED: &[&str] = &[
            "area", "base", "basefont", "datalist", "head", "link", "meta", "param", "rp",
            "script", "source", "style", "template", "track", "title",
        ];
        SKIPPED.iter().any(|s| s.eq_ignore_ascii_case(name))
            || self.attribute(id, "style").is_some_and(css_hides)
    }
}

/// Pascal's `strTrim`: strips every character up to `' '`.
fn trim_pascal(s: &str) -> &str {
    s.trim_matches(|c: char| c <= ' ')
}

/// `strTrimAndNormalize`: trimmed, runs of characters up to `' '` as one space.
fn trim_and_normalize(s: &str) -> String {
    let mut out = String::new();
    for part in s.split(|c: char| c <= ' ').filter(|p| !p.is_empty()) {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(part);
    }
    out
}

/// Whether a `style` attribute hides the element (`CSSHasHiddenStyle`, internettools
/// data/simplehtmltreeparser.pas:1180-1241): `display: none` or `visibility: hidden`.
fn css_hides(style: &str) -> bool {
    let mut hidden = false;
    for declaration in style.split(';') {
        let Some((property, value)) = declaration.split_once(':') else {
            continue;
        };
        let property = property.trim().to_ascii_lowercase();
        let value = value.trim().to_ascii_lowercase();
        let first = value
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
            .next()
            .unwrap_or("");
        match property.as_str() {
            "visibility" if first == "hidden" => hidden = true,
            "visibility" if first == "visible" => hidden = false,
            "display" if first == "none" => hidden = true,
            _ => {}
        }
    }
    hidden
}

/// `appendHTMLText`: `&`, `<` and `>` escaped.
fn escape_text(text: &str, out: &mut String) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            c => out.push(c),
        }
    }
}

/// `appendHTMLAttrib`: `&` (unless before `{`), `"` and `'` escaped.
fn escape_attribute(value: &str, out: &mut String) {
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '&' if chars.peek() != Some(&'{') => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c => out.push(c),
        }
    }
}

/// Elements serialized without an end tag when empty (`htmlElementIsChildless`, internettools
/// data/htmlinformation.pas:213-240).
fn is_childless(name: &str) -> bool {
    const CHILDLESS: &[&str] = &[
        "area", "base", "basefont", "bgsound", "br", "col", "command", "embed", "frame", "hr",
        "img", "input", "isindex", "keygen", "link", "meta", "param", "source", "track", "wbr",
    ];
    CHILDLESS.iter().any(|c| c.eq_ignore_ascii_case(name))
}

/// Elements whose text is serialized unescaped (`htmlElementIsImplicitCDATA`, internettools
/// data/htmlinformation.pas:190-205).
fn is_raw_text(name: &str) -> bool {
    const RAW: &[&str] = &[
        "style",
        "script",
        "xmp",
        "iframe",
        "noembed",
        "noframes",
        "plaintext",
    ];
    RAW.iter().any(|r| r.eq_ignore_ascii_case(name))
}

/// Boolean attributes written without a value when it equals the name
/// (`htmlAttributeIsBooleanAttribute`, internettools data/htmlinformation.pas:461-535).
fn is_boolean_attribute(element: &str, attribute: &str) -> bool {
    let e = element.to_ascii_lowercase();
    let media = e == "audio" || e == "video";
    match attribute.to_ascii_lowercase().as_str() {
        "disabled" => matches!(
            e.as_str(),
            "link" | "input" | "button" | "select" | "option" | "optgroup"
        ),
        "reversed" => e == "ol",
        "ismap" => e == "img",
        "allowfullscreen" => e == "iframe",
        "playsinline" => e == "video",
        "default" => e == "track",
        "selected" => e == "option",
        "loop" | "autoplay" | "controls" | "muted" => media,
        "checked" | "readonly" => e == "input",
        "novalidate" | "formnovalidate" => e == "form",
        "nomodule" | "async" | "defer" => e == "script",
        "open" => e == "details",
        "itemscope" | "hidden" => true,
        _ => false,
    }
}

/// The whitespace before the first element, around any doctype and comments: internettools
/// keeps it as a text node of the document, where HTML5 parsing drops it.
fn leading_whitespace(html: &[u8]) -> String {
    let mut rest = html.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(html);
    let mut whitespace = String::new();
    loop {
        let spaces = rest
            .iter()
            .take_while(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r' | b'\x0c'))
            .count();
        whitespace.extend(rest[..spaces].iter().map(|&b| char::from(b)));
        rest = &rest[spaces..];
        let end = if rest.starts_with(b"<!--") {
            find(rest, b"-->").map(|i| i + 3)
        } else if rest.starts_with(b"<!") || rest.starts_with(b"<?") {
            find(rest, b">").map(|i| i + 1)
        } else {
            None
        };
        match end {
            Some(end) => rest = &rest[end..],
            None => return whitespace.replace("\r\n", "\n").replace('\r', "\n"),
        }
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// A node while html5ever builds the tree.
struct SinkNode {
    data: SinkData,
    parent: Option<usize>,
    children: Vec<usize>,
}

enum SinkData {
    Document,
    Element {
        name: QualName,
        attrs: Vec<Attribute>,
        /// The template contents fragment, whose children become the template's children.
        template: Option<usize>,
    },
    Text(String),
    /// A comment or processing instruction, never attached.
    Dropped,
}

/// html5ever's tree sink. html5ever calls it through `&self`, hence the `RefCell`.
struct Sink {
    nodes: RefCell<Vec<SinkNode>>,
}

impl Default for Sink {
    fn default() -> Self {
        Sink {
            nodes: RefCell::new(vec![SinkNode {
                data: SinkData::Document,
                parent: None,
                children: Vec::new(),
            }]),
        }
    }
}

/// An element name handed back to html5ever.
#[derive(Debug)]
struct Name(QualName);

impl ElemName for Name {
    fn ns(&self) -> &Namespace {
        &self.0.ns
    }
    fn local_name(&self) -> &LocalName {
        &self.0.local
    }
}

impl Sink {
    fn new_node(&self, data: SinkData) -> usize {
        let mut nodes = self.nodes.borrow_mut();
        nodes.push(SinkNode {
            data,
            parent: None,
            children: Vec::new(),
        });
        nodes.len() - 1
    }

    fn detach(&self, node: usize) {
        let mut nodes = self.nodes.borrow_mut();
        if let Some(parent) = nodes[node].parent.take() {
            nodes[parent].children.retain(|&c| c != node);
        }
    }

    /// Inserts `child` into `parent` at `index`, merging text with a text neighbour before it.
    fn insert(&self, parent: usize, index: usize, child: NodeOrText<usize>) {
        let node = match child {
            NodeOrText::AppendText(text) => {
                let mut nodes = self.nodes.borrow_mut();
                let previous = index
                    .checked_sub(1)
                    .and_then(|i| nodes[parent].children.get(i).copied());
                if let Some(previous) = previous
                    && let SinkData::Text(existing) = &mut nodes[previous].data
                {
                    existing.push_str(&text);
                    return;
                }
                drop(nodes);
                self.new_node(SinkData::Text(text.to_string()))
            }
            NodeOrText::AppendNode(node) => {
                if matches!(self.nodes.borrow()[node].data, SinkData::Dropped) {
                    return;
                }
                self.detach(node);
                node
            }
        };
        let mut nodes = self.nodes.borrow_mut();
        let index = index.min(nodes[parent].children.len());
        nodes[parent].children.insert(index, node);
        nodes[node].parent = Some(parent);
    }

    fn into_dom(self) -> Dom {
        let nodes = self.nodes.into_inner();
        let mut dom = Dom { nodes: Vec::new() };
        freeze(&nodes, 0, None, &mut dom);
        dom
    }
}

/// Copies `id`'s subtree into `dom` in document order.
fn freeze(nodes: &[SinkNode], id: usize, parent: Option<NodeId>, dom: &mut Dom) {
    let new = dom.nodes.len();
    let (kind, children) = match &nodes[id].data {
        SinkData::Document => (NodeKind::Document, &nodes[id].children),
        SinkData::Element { name, template, .. } => (
            NodeKind::Element(qualified(name)),
            match template {
                Some(contents) => &nodes[*contents].children,
                None => &nodes[id].children,
            },
        ),
        SinkData::Text(text) => (NodeKind::Text(text.clone()), &nodes[id].children),
        SinkData::Dropped => return,
    };
    dom.nodes.push(Node {
        kind,
        parent,
        attributes: Vec::new(),
        children: Vec::new(),
        last: new,
    });
    if let SinkData::Element { attrs, .. } = &nodes[id].data {
        for attr in attrs {
            let a = dom.nodes.len();
            dom.nodes.push(Node {
                kind: NodeKind::Attribute {
                    name: qualified(&attr.name),
                    value: attr.value.to_string(),
                },
                parent: Some(new),
                attributes: Vec::new(),
                children: Vec::new(),
                last: a,
            });
            dom.nodes[new].attributes.push(a);
        }
    }
    for &child in children {
        let before = dom.nodes.len();
        freeze(nodes, child, Some(new), dom);
        if dom.nodes.len() > before {
            dom.nodes[new].children.push(before);
        }
    }
    dom.nodes[new].last = dom.nodes.len() - 1;
}

/// `prefix:local`, as internettools names nodes from the source.
fn qualified(name: &QualName) -> String {
    match &name.prefix {
        Some(prefix) => format!("{prefix}:{}", name.local),
        None => name.local.to_string(),
    }
}

impl TreeSink for Sink {
    type Handle = usize;
    type Output = Self;
    type ElemName<'a> = Name;

    fn finish(self) -> Self {
        self
    }

    fn parse_error(&self, _msg: Cow<'static, str>) {}

    fn get_document(&self) -> usize {
        0
    }

    fn elem_name<'a>(&'a self, target: &'a usize) -> Name {
        match &self.nodes.borrow()[*target].data {
            SinkData::Element { name, .. } => Name(name.clone()),
            // html5ever only asks for elements' names.
            _ => Name(QualName::new(None, ns!(), LocalName::from(""))),
        }
    }

    fn create_element(&self, name: QualName, attrs: Vec<Attribute>, flags: ElementFlags) -> usize {
        let template = flags.template.then(|| self.new_node(SinkData::Document));
        self.new_node(SinkData::Element {
            name,
            attrs,
            template,
        })
    }

    fn create_comment(&self, _text: StrTendril) -> usize {
        self.new_node(SinkData::Dropped)
    }

    fn create_pi(&self, _target: StrTendril, _data: StrTendril) -> usize {
        self.new_node(SinkData::Dropped)
    }

    fn append(&self, parent: &usize, child: NodeOrText<usize>) {
        let index = self.nodes.borrow()[*parent].children.len();
        self.insert(*parent, index, child);
    }

    fn append_based_on_parent_node(
        &self,
        element: &usize,
        prev_element: &usize,
        child: NodeOrText<usize>,
    ) {
        if self.nodes.borrow()[*element].parent.is_some() {
            self.append_before_sibling(element, child);
        } else {
            self.append(prev_element, child);
        }
    }

    fn append_doctype_to_document(&self, _: StrTendril, _: StrTendril, _: StrTendril) {}

    fn get_template_contents(&self, target: &usize) -> usize {
        match &self.nodes.borrow()[*target].data {
            SinkData::Element {
                template: Some(contents),
                ..
            } => *contents,
            // html5ever only asks for templates' contents.
            _ => *target,
        }
    }

    fn same_node(&self, x: &usize, y: &usize) -> bool {
        x == y
    }

    fn set_quirks_mode(&self, _mode: QuirksMode) {}

    fn append_before_sibling(&self, sibling: &usize, child: NodeOrText<usize>) {
        let parent = self.nodes.borrow()[*sibling].parent;
        if let Some(parent) = parent {
            let index = self.nodes.borrow()[parent]
                .children
                .iter()
                .position(|c| c == sibling)
                .unwrap_or(0);
            self.insert(parent, index, child);
        }
    }

    fn add_attrs_if_missing(&self, target: &usize, attrs: Vec<Attribute>) {
        if let SinkData::Element {
            attrs: existing, ..
        } = &mut self.nodes.borrow_mut()[*target].data
        {
            for attr in attrs {
                if !existing.iter().any(|e| e.name == attr.name) {
                    existing.push(attr);
                }
            }
        }
    }

    fn remove_from_parent(&self, target: &usize) {
        self.detach(*target);
    }

    fn reparent_children(&self, node: &usize, new_parent: &usize) {
        let children = std::mem::take(&mut self.nodes.borrow_mut()[*node].children);
        for child in children {
            self.nodes.borrow_mut()[child].parent = None;
            self.append(new_parent, NodeOrText::AppendNode(child));
        }
    }
}
