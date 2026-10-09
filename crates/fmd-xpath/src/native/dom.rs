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
    /// An element's or attribute's namespace, an index into [`Dom::namespaces`]; `None` for no
    /// namespace (`TTreeNode.namespace = nil`).
    pub(crate) namespace: Option<NsId>,
    /// The namespaces an element's `xmlns` and `xmlns:p` attributes declare, in order.
    declarations: Vec<NsId>,
    /// The default namespace in scope inside an element (the parser's `FCurrentNamespace`).
    default_namespace: Option<NsId>,
}

impl Node {
    /// A node with id `id` and no attributes, children or namespace yet.
    fn new(kind: NodeKind, parent: Option<NodeId>, id: NodeId) -> Node {
        Node {
            kind,
            parent,
            attributes: Vec::new(),
            children: Vec::new(),
            last: id,
            namespace: None,
            declarations: Vec::new(),
            default_namespace: None,
        }
    }
}

/// Index of a namespace in [`Dom::namespaces`].
pub(crate) type NsId = usize;

/// A namespace (internettools' `TNamespace`). Every declaration makes one, so nodes share a
/// namespace only when they got it from the same declaration.
pub(crate) struct Ns {
    pub(crate) prefix: String,
    pub(crate) url: String,
}

/// The XML namespace, bound to `xml:` without a declaration.
const XML_NAMESPACE: &str = "http://www.w3.org/XML/1998/namespace";
/// The XMLNS namespace of namespace declarations.
const XMLNS_NAMESPACE: &str = "http://www.w3.org/2000/xmlns/";
/// The XHTML namespace, whose elements serialize as HTML.
const XHTML_NAMESPACE: &str = "http://www.w3.org/1999/xhtml";

/// Whether an attribute named `name` declares a namespace (`TTreeAttribute.isNamespaceNode`,
/// internettools data/simplehtmltreeparser.pas:742-745).
pub(crate) fn is_declaration(name: &str) -> bool {
    name == "xmlns" || name.starts_with("xmlns:")
}

/// A parsed document. The document node is id 0.
pub(crate) struct Dom {
    nodes: Vec<Node>,
    namespaces: Vec<Ns>,
}

impl Dom {
    /// Parses `html` (invalid UTF-8 replaced) like `TTreeParser.parseTree`.
    pub(crate) fn parse(html: &[u8]) -> Dom {
        let sink = Sink::default();
        // Closer to internettools: `<noscript>` content is markup (scripting off), and there is
        // no quirks mode, which an `iframe srcdoc` document never enters (`<p>` closes before
        // `<table>`).
        let options = html5ever::ParseOpts {
            tree_builder: html5ever::tree_builder::TreeBuilderOpts {
                scripting_enabled: false,
                iframe_srcdoc: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let sink = html5ever::parse_document(sink, options)
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

    /// The URL of the node's namespace; empty when it has none (`fn:namespace-uri`, internettools
    /// data/xquery__functions.pas:3290).
    pub(crate) fn namespace_url(&self, id: NodeId) -> &str {
        self.nodes[id]
            .namespace
            .map_or("", |ns| self.namespaces[ns].url.as_str())
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
    /// (`innerHTML`), serialized like internettools' `serializeNodes` with `html` set
    /// (internettools data/xquery__serialization_nodes.pas:386-700): elements outside the HTML
    /// namespaces as XML, and the first element with the namespace declarations in scope.
    pub(crate) fn html(&self, id: NodeId, outer: bool) -> String {
        let mut serializer = Serializer {
            dom: self,
            out: String::new(),
            known: None,
        };
        match &self.nodes[id].kind {
            NodeKind::Element(_) if outer => serializer.nodes(id, id, true),
            // `inner(base, elementIsHTML(base))`; the document node has no namespace.
            NodeKind::Element(_) => serializer.nodes(id, id + 1, self.is_html_namespace(id)),
            // `outer` of the document is `inner(n, false)`.
            NodeKind::Document => serializer.nodes(id, id + 1, !outer),
            NodeKind::Text(text) if outer => escape_text(text, &mut serializer.out),
            NodeKind::Text(_) | NodeKind::Attribute { .. } => {}
        }
        serializer.out
    }

    /// `elementIsHTML`: no namespace, the empty one, or XHTML's (internettools
    /// data/xquery__serialization_nodes.pas:397-402).
    fn is_html_namespace(&self, id: NodeId) -> bool {
        self.nodes[id].namespace.is_none_or(|ns| {
            let url = &self.namespaces[ns].url;
            url.is_empty() || url == XHTML_NAMESPACE
        })
    }

    /// The element's own namespaces (`TTreeNode.getOwnNamespaces`, internettools
    /// data/simplehtmltreeparser.pas:1562-1580): its declarations, its namespace, and its
    /// attributes'.
    fn own_namespaces<'a>(&'a self, id: NodeId, list: &mut Vec<(&'a str, &'a str)>) {
        let node = &self.nodes[id];
        let attributes = node
            .attributes
            .iter()
            .filter_map(|&a| self.nodes[a].namespace);
        for ns in node
            .declarations
            .iter()
            .copied()
            .chain(node.namespace)
            .chain(attributes)
        {
            let ns = &self.namespaces[ns];
            add_if_new_prefix_url(list, (&ns.prefix, &ns.url));
        }
    }

    /// The namespaces in scope at the element (`TTreeNode.getAllNamespaces`, internettools
    /// data/simplehtmltreeparser.pas:1582-1601): its own, then its ancestors' declarations of
    /// prefixes not seen yet.
    fn all_namespaces(&self, id: NodeId) -> Vec<(&str, &str)> {
        let mut list = Vec::new();
        self.own_namespaces(id, &mut list);
        let mut ancestor = self.nodes[id].parent;
        while let Some(a) = ancestor {
            for &ns in &self.nodes[a].declarations {
                let ns = &self.namespaces[ns];
                if !is_reserved(&ns.url) && !list.iter().any(|(p, _)| *p == ns.prefix) {
                    list.push((&ns.prefix, &ns.url));
                }
            }
            ancestor = self.nodes[a].parent;
        }
        list
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
        let mut i = id;
        while i <= self.nodes[id].last {
            let node = &self.nodes[i];
            match &node.kind {
                NodeKind::Text(text) if !text.is_empty() => {
                    let normalized = trim_and_normalize(text);
                    if normalized.is_empty() {
                        trailing_space = true;
                    } else {
                        let ends_with_space = out.as_bytes().last().is_some_and(|&b| b <= b' ');
                        let starts_with_space = text.as_bytes().first().is_some_and(|&b| b <= b' ');
                        if (trailing_space || starts_with_space) && !ends_with_space {
                            out.push(' ');
                        }
                        out.push_str(&normalized);
                        trailing_space = text.as_bytes().last().is_some_and(|&b| b <= b' ');
                    }
                }
                NodeKind::Element(name) => {
                    if self.skips_text(i, name) {
                        i = node.last + 1;
                        continue;
                    }
                    let marker = match name.to_ascii_lowercase().as_str() {
                        "br" | "tr" => "\n",
                        "td" | "th" => "\t",
                        "p" => "\n\n",
                        _ => "",
                    };
                    if !marker.is_empty() {
                        out.push_str(marker);
                        trailing_space = false;
                    }
                }
                NodeKind::Text(_) | NodeKind::Document | NodeKind::Attribute { .. } => {}
            }
            i += 1;
        }
        trim_pascal(&out).to_owned()
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

/// `addIfNewPrefixUrl` (internettools data/xquery.namespaces.pas:266-275): adds `ns` unless
/// the last namespace with its prefix has its URL too.
fn add_if_new_prefix_url<'a>(list: &mut Vec<(&'a str, &'a str)>, ns: (&'a str, &'a str)) {
    if is_reserved(ns.1) {
        return;
    }
    if list
        .iter()
        .rev()
        .find(|(p, _)| *p == ns.0)
        .is_none_or(|(_, url)| *url != ns.1)
    {
        list.push(ns);
    }
}

/// The XML and XMLNS namespaces, which are never declared.
fn is_reserved(url: &str) -> bool {
    url == XML_NAMESPACE || url == XMLNS_NAMESPACE
}

/// One run of `serializeNodes`.
struct Serializer<'a> {
    dom: &'a Dom,
    out: String,
    /// The namespaces declared so far on the open elements (`known`); `None` until the first
    /// element, which declares all those in scope.
    known: Option<Vec<(&'a str, &'a str)>>,
}

/// An element being serialized.
struct Open<'a> {
    id: NodeId,
    name: &'a str,
    is_html: bool,
    /// Whether it made its content raw text (`inCDATAElement`).
    raw: bool,
    /// How many namespaces were known before it.
    known: usize,
}

impl<'a> Serializer<'a> {
    /// Serializes the nodes `first..=last of base` in document order, without recursion;
    /// `base_is_html` is whether their parent counts as an HTML element.
    fn nodes(&mut self, base: NodeId, first: NodeId, base_is_html: bool) {
        let dom = self.dom;
        let mut open: Vec<Open<'a>> = Vec::new();
        let mut raw = false;
        for i in first..=dom.nodes[base].last {
            while let Some(element) = open.last() {
                if dom.nodes[element.id].last >= i {
                    break;
                }
                self.close(element.name, element.known);
                raw &= !element.raw;
                open.pop();
            }
            let parent_is_html = open.last().map_or(base_is_html, |o| o.is_html);
            match &dom.nodes[i].kind {
                NodeKind::Text(text) if !parent_is_html => escape_xml(text, false, &mut self.out),
                NodeKind::Text(text) if raw => self.out.push_str(text),
                NodeKind::Text(text) => escape_text(text, &mut self.out),
                NodeKind::Element(name) => {
                    if let Some(element) = self.start_tag(i, name, parent_is_html, raw) {
                        raw |= element.raw;
                        open.push(element);
                    }
                }
                NodeKind::Document | NodeKind::Attribute { .. } => {}
            }
        }
        while let Some(element) = open.pop() {
            self.close(element.name, element.known);
        }
    }

    /// Writes the element's start tag, or the whole element when it has no children; returns
    /// it when it stays open.
    fn start_tag(
        &mut self,
        id: NodeId,
        name: &'a str,
        parent_is_html: bool,
        raw: bool,
    ) -> Option<Open<'a>> {
        let dom = self.dom;
        let node = &dom.nodes[id];
        let same_namespace = node
            .parent
            .is_some_and(|p| dom.nodes[p].namespace == node.namespace);
        let is_html = (parent_is_html && same_namespace) || dom.is_html_namespace(id);
        let known = match &mut self.known {
            Some(known) => {
                let before = known.len();
                dom.own_namespaces(id, known);
                before
            }
            None => {
                self.known = Some(dom.all_namespaces(id));
                0
            }
        };
        let list = self.known.get_or_insert_with(Vec::new);
        self.out.push('<');
        self.out.push_str(name);
        for k in known..list.len() {
            let (prefix, url) = list[k];
            let undeclares = list[..known].iter().any(|(p, _)| *p == prefix);
            if !url.is_empty() || undeclares {
                declaration(prefix, url, &mut self.out);
            }
        }
        // `requireNamespace`, or `xmlns=""` for an element without a namespace inside a default
        // one (internettools data/xquery__serialization_nodes.pas:584-591).
        match node.namespace {
            // Already declared above.
            Some(_) => {}
            None => {
                if list
                    .iter()
                    .rev()
                    .find(|(p, _)| p.is_empty())
                    .is_some_and(|(_, url)| !url.is_empty())
                {
                    list.push(("", ""));
                    self.out.push_str(" xmlns=\"\"");
                }
            }
        }
        for &a in &node.attributes {
            let NodeKind::Attribute { name: attr, value } = &dom.nodes[a].kind else {
                continue;
            };
            if is_declaration(attr) {
                continue;
            }
            self.out.push(' ');
            self.out.push_str(attr);
            if is_html && attr.eq_ignore_ascii_case(value) && is_boolean_attribute(name, attr) {
                continue;
            }
            self.out.push_str("=\"");
            if raw {
                self.out.push_str(value);
            } else if is_html {
                escape_attribute(value, &mut self.out);
            } else {
                escape_xml(value, true, &mut self.out);
            }
            self.out.push('"');
        }
        if node.children.is_empty() {
            if !is_html {
                self.out.push_str("/>");
            } else if is_childless(name) {
                self.out.push('>');
            } else {
                self.out.push_str("></");
                self.out.push_str(name);
                self.out.push('>');
            }
            list.truncate(known);
            return None;
        }
        self.out.push('>');
        Some(Open {
            id,
            name,
            is_html,
            raw: !raw && is_html && is_raw_text(name),
            known,
        })
    }

    /// Writes an end tag and forgets the element's namespaces.
    fn close(&mut self, name: &str, known: usize) {
        close_tag(name, &mut self.out);
        if let Some(list) = &mut self.known {
            list.truncate(known);
        }
    }
}

/// ` xmlns="url"` or ` xmlns:prefix="url"` (`TNamespace.serialize`, internettools
/// data/xquery.namespaces.pas:451-455).
fn declaration(prefix: &str, url: &str, out: &mut String) {
    out.push_str(" xmlns");
    if !prefix.is_empty() {
        out.push(':');
        out.push_str(prefix);
    }
    out.push_str("=\"");
    escape_xml(url, true, out);
    out.push('"');
}

/// `appendXMLText` and, with `attribute`, `appendXMLAttrib` (internettools
/// data/xquery.internals.common.pas:1528-1587).
fn escape_xml(text: &str, attribute: bool, out: &mut String) {
    for c in text.chars() {
        match c {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '\'' => out.push_str("&apos;"),
            '"' => out.push_str("&quot;"),
            '\r' => out.push_str("&#xD;"),
            '\n' if attribute => out.push_str("&#xA;"),
            '\t' if attribute => out.push_str("&#x9;"),
            '\n' | '\t' => out.push(c),
            '\0'..='\x1f' | '\x7f'..='\u{9f}' | '\u{2028}' => {
                out.push_str(&format!("&#x{:X};", u32::from(c)));
            }
            c => out.push(c),
        }
    }
}

fn close_tag(name: &str, out: &mut String) {
    out.push_str("</");
    out.push_str(name);
    out.push('>');
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
        let mut dom = Dom {
            nodes: Vec::new(),
            namespaces: Vec::new(),
        };
        freeze(&nodes, &mut dom);
        dom
    }
}

/// Copies the tree under the sink's document into `dom` in document order, without
/// recursion (pages can nest arbitrarily deep).
fn freeze(nodes: &[SinkNode], dom: &mut Dom) {
    let mut pending: Vec<(usize, Option<NodeId>)> = vec![(0, None)];
    while let Some((id, parent)) = pending.pop() {
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
            SinkData::Dropped => continue,
        };
        let new = dom.nodes.len();
        dom.nodes.push(Node::new(kind, parent, new));
        if let Some(parent) = parent {
            dom.nodes[parent].children.push(new);
        }
        if let SinkData::Element { attrs, .. } = &nodes[id].data {
            for attr in attrs {
                let a = dom.nodes.len();
                let kind = NodeKind::Attribute {
                    name: qualified(&attr.name),
                    value: attr.value.to_string(),
                };
                dom.nodes.push(Node::new(kind, Some(new), a));
                dom.nodes[new].attributes.push(a);
            }
        }
        pending.extend(children.iter().rev().map(|&child| (child, Some(new))));
    }
    // Children have larger ids than their parents, so going backwards sees them first.
    for id in (0..dom.nodes.len()).rev() {
        let node = &dom.nodes[id];
        let last = match (node.children.last(), node.attributes.last()) {
            (Some(&child), _) => dom.nodes[child].last,
            (None, Some(&attribute)) => attribute,
            (None, None) => id,
        };
        dom.nodes[id].last = last;
    }
    resolve_namespaces(dom);
}

/// Gives elements and attributes their namespaces as internettools' HTML parser does
/// (`TTreeParser.enterTagCommon`, internettools data/simplehtmltreeparser.pas:2436-2479):
/// `xmlns` and `xmlns:p` attributes declare namespaces for the element and its descendants,
/// an unprefixed element is in the default namespace in scope, and a prefixed name is in its
/// prefix's namespace, or, when the prefix isn't declared, loses the prefix and is in none.
fn resolve_namespaces(dom: &mut Dom) {
    // Parents come before their children.
    for id in 0..dom.nodes.len() {
        if !matches!(dom.nodes[id].kind, NodeKind::Element(_)) {
            continue;
        }
        let mut default = dom.nodes[id]
            .parent
            .and_then(|p| dom.nodes[p].default_namespace);
        for a in dom.nodes[id].attributes.clone() {
            let NodeKind::Attribute { name, value } = &dom.nodes[a].kind else {
                continue;
            };
            let prefix = match name.strip_prefix("xmlns") {
                Some("") => "",
                Some(rest) => match rest.strip_prefix(':') {
                    Some(prefix) => prefix,
                    None => continue,
                },
                None => continue,
            };
            let declared = Ns {
                prefix: prefix.to_owned(),
                url: collapse_whitespace(value),
            };
            let ns = dom.namespaces.len();
            let is_default = declared.prefix.is_empty();
            dom.namespaces.push(declared);
            dom.nodes[id].declarations.push(ns);
            if is_default {
                default = Some(ns);
            }
            dom.nodes[a].namespace = Some(xmlns_namespace(dom));
        }
        dom.nodes[id].default_namespace = default;
        dom.nodes[id].namespace = default;
        resolve_prefix(dom, id, id);
        for a in dom.nodes[id].attributes.clone() {
            if !is_declaration(dom.name(a)) {
                resolve_prefix(dom, id, a);
            }
        }
    }
}

/// Puts the node `id` with a prefixed name in the namespace its prefix names at the element
/// `scope` (`TTreeParser.findNamespace`, internettools data/simplehtmltreeparser.pas:
/// 2736-2747). An unknown prefix is dropped from the name, leaving it in no namespace.
fn resolve_prefix(dom: &mut Dom, scope: NodeId, id: NodeId) {
    let Some((prefix, local)) = dom.name(id).split_once(':') else {
        return;
    };
    let (prefix, local) = (prefix.to_owned(), local.to_owned());
    let namespace = find_namespace(dom, scope, &prefix);
    if namespace.is_none() {
        match &mut dom.nodes[id].kind {
            NodeKind::Element(name) | NodeKind::Attribute { name, .. } => *name = local,
            NodeKind::Document | NodeKind::Text(_) => {}
        }
    }
    dom.nodes[id].namespace = namespace;
}

/// The namespace `prefix` names at the element `scope`: the innermost declaration on it or
/// its ancestors; `xml` needs none (internettools data/simplehtmltreeparser.pas:2736-2747).
fn find_namespace(dom: &mut Dom, scope: NodeId, prefix: &str) -> Option<NsId> {
    let mut element = Some(scope);
    while let Some(e) = element {
        let node = &dom.nodes[e];
        if let Some(&ns) = node
            .declarations
            .iter()
            .rev()
            .find(|&&ns| dom.namespaces[ns].prefix == prefix)
        {
            return Some(ns);
        }
        element = node.parent;
    }
    if prefix == "xml" {
        return Some(reserved_namespace(dom, "xml", XML_NAMESPACE));
    }
    None
}

/// The namespace of namespace declarations.
fn xmlns_namespace(dom: &mut Dom) -> NsId {
    reserved_namespace(dom, "xmlns", XMLNS_NAMESPACE)
}

/// The one namespace object for a reserved URL.
fn reserved_namespace(dom: &mut Dom, prefix: &str, url: &str) -> NsId {
    if let Some(ns) = dom.namespaces.iter().position(|ns| ns.url == url) {
        return ns;
    }
    dom.namespaces.push(Ns {
        prefix: prefix.to_owned(),
        url: url.to_owned(),
    });
    dom.namespaces.len() - 1
}

/// `xmlStrWhitespaceCollapse`, which a declared URL goes through (internettools
/// data/simplehtmltreeparser.pas:2453): trimmed, runs of whitespace as one space.
fn collapse_whitespace(s: &str) -> String {
    s.split([' ', '\t', '\n', '\r'])
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// `prefix:local`, as internettools names nodes from the source.
fn qualified(name: &QualName) -> String {
    match &name.prefix {
        // html5ever gives a foreign element's `xmlns` the empty prefix; internettools keeps the
        // name as written (data/simplehtmltreeparser.pas:2451-2457).
        Some(prefix) if !prefix.is_empty() => format!("{prefix}:{}", name.local),
        _ => name.local.to_string(),
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
