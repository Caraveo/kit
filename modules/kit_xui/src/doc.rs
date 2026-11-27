//! The `.xui` document model: an HTML-like tree that remembers where every
//! element and attribute value sits in the source text, so visual edits can
//! rewrite just the value that changed (see [`crate::edit`]).

use anyhow::{Result, anyhow};
use std::ops::Range;

/// Where an attribute sits in the source.
#[derive(Clone, Debug, Default)]
pub struct AttrSpan {
    /// From the first character of the name to the end of the value (closing quote included).
    pub whole: Range<usize>,
    /// The value between the quotes (empty range right after `=` / name when there's no value).
    pub value: Range<usize>,
}

#[derive(Clone, Debug, Default)]
pub struct Node {
    pub tag: String,
    /// Attributes in source order. Text content is stored as a synthetic `_text` attribute.
    pub attributes: Vec<(String, String)>,
    pub children: Vec<Node>,
    /// The whole element in the source: `<` .. end of the closing tag (or `/>`).
    pub span: Range<usize>,
    /// Where a new attribute can be inserted: right after the last attribute (or the tag name).
    pub attr_insert: usize,
    /// Spans of the attributes, parallel to `attributes` (`None` for `_text` or synthesized ones).
    pub attr_spans: Vec<Option<AttrSpan>>,
    /// The `/>` of a self-closing element, or the `<` of the closing tag otherwise.
    pub close: Range<usize>,
    pub self_closing: bool,
}

/// Equality ignores source positions: two documents are equal when their
/// trees (tags, attributes, children) are equal.
impl PartialEq for Node {
    fn eq(&self, other: &Self) -> bool {
        self.tag == other.tag && self.attributes == other.attributes && self.children == other.children
    }
}

impl Node {
    pub fn new(tag: impl Into<String>) -> Self {
        Self { tag: tag.into(), ..Default::default() }
    }

    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attributes.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_str())
    }

    pub fn attr_span(&self, name: &str) -> Option<&AttrSpan> {
        let index = self.attributes.iter().position(|(key, _)| key == name)?;
        self.attr_spans.get(index)?.as_ref()
    }

    /// Classes from `class` (or the legacy `name`).
    pub fn classes(&self) -> Vec<&str> {
        self.attr("class").or_else(|| self.attr("name")).unwrap_or_default().split_whitespace().collect()
    }

    pub fn attr_mut(&mut self, name: &str) -> Option<&mut String> {
        self.attributes.iter_mut().find(|(key, _)| key == name).map(|(_, value)| value)
    }

    /// Set an attribute in the tree only (the source isn't touched; see [`crate::edit`]).
    pub fn set_attr(&mut self, name: impl Into<String>, value: impl Into<String>) {
        let name = name.into();
        if let Some(slot) = self.attr_mut(&name) {
            *slot = value.into();
        } else {
            self.attributes.push((name, value.into()));
            self.attr_spans.push(None);
        }
    }

    pub fn id(&self) -> Option<&str> {
        self.attr("id").filter(|id| !id.is_empty())
    }

    pub fn text(&self) -> Option<&str> {
        self.attr("text").or_else(|| self.attr("_text"))
    }

    pub fn find(&self, id: &str) -> Option<&Node> {
        if self.id() == Some(id) {
            return Some(self);
        }
        self.children.iter().find_map(|child| child.find(id))
    }

    pub fn find_mut(&mut self, id: &str) -> Option<&mut Node> {
        if self.id() == Some(id) {
            return Some(self);
        }
        self.children.iter_mut().find_map(|child| child.find_mut(id))
    }

    /// The path (child indexes from this node) of the element with `id`.
    pub fn path_of(&self, id: &str) -> Option<Vec<usize>> {
        if self.id() == Some(id) {
            return Some(Vec::new());
        }
        self.children.iter().enumerate().find_map(|(i, child)| {
            child.path_of(id).map(|mut p| {
                p.insert(0, i);
                p
            })
        })
    }

    pub fn at(&self, path: &[usize]) -> Option<&Node> {
        path.iter().try_fold(self, |node, &i| node.children.get(i))
    }

    pub fn at_mut(&mut self, path: &[usize]) -> Option<&mut Node> {
        path.iter().try_fold(self, |node, &i| node.children.get_mut(i))
    }

    pub fn remove(&mut self, id: &str) -> bool {
        if let Some(index) = self.children.iter().position(|child| child.id() == Some(id)) {
            self.children.remove(index);
            return true;
        }
        self.children.iter_mut().any(|child| child.remove(id))
    }

    /// Every id used in this subtree.
    pub fn ids(&self) -> Vec<String> {
        let mut out = Vec::new();
        fn walk(node: &Node, out: &mut Vec<String>) {
            if let Some(id) = node.id() {
                out.push(id.to_owned());
            }
            node.children.iter().for_each(|c| walk(c, out));
        }
        walk(self, &mut out);
        out
    }

    /// Is this a container that lays out children?
    pub fn is_container(&self) -> bool {
        matches!(self.tag.as_str(), "panel" | "row" | "column" | "grid" | "interface")
    }
}

/// A parse error with the line and column where it happened.
#[derive(Clone, Debug, PartialEq)]
pub struct ParseError {
    pub message: String,
    pub offset: usize,
    pub line: usize,
    pub column: usize,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "line {}, column {}: {}", self.line, self.column, self.message)
    }
}

impl std::error::Error for ParseError {}

pub fn line_col(source: &str, offset: usize) -> (usize, usize) {
    let before = &source[..offset.min(source.len())];
    let line = before.matches('\n').count() + 1;
    let column = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
    (line, column)
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Document {
    pub root: Node,
}

impl Document {
    pub fn parse(source: &str) -> Result<Self> {
        Self::parse_detailed(source).map_err(|e| anyhow!(e))
    }

    /// Parse, returning where the error is.
    pub fn parse_detailed(source: &str) -> std::result::Result<Self, ParseError> {
        parse(source).map_err(|(offset, message)| {
            let (line, column) = line_col(source, offset);
            ParseError { message, offset, line, column }
        })
    }

    /// Re-serialize in Kit's standard formatting (used for new documents; the
    /// designer edits the original source in place instead).
    pub fn to_source(&self) -> String {
        let mut out = String::new();
        write_node(&self.root, 0, &mut out);
        out
    }
}

pub fn escape_attr(value: &str) -> String {
    value.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;").replace('>', "&gt;")
}

fn escape_text(value: &str) -> String {
    value.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

pub fn unescape(value: &str) -> String {
    value.replace("&quot;", "\"").replace("&apos;", "'").replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&")
}

/// Write one element (and its children) in standard formatting.
pub fn write_node(node: &Node, depth: usize, out: &mut String) {
    let indent = "  ".repeat(depth);
    out.push_str(&indent);
    out.push('<');
    out.push_str(&node.tag);
    for (key, value) in &node.attributes {
        if key == "_text" {
            continue;
        }
        out.push(' ');
        out.push_str(key);
        out.push_str("=\"");
        out.push_str(&escape_attr(value));
        out.push('"');
    }
    if node.children.is_empty() && node.attr("_text").is_none() {
        out.push_str(" />\n");
        return;
    }
    out.push_str(">\n");
    if let Some(text) = node.attr("_text") {
        out.push_str(&indent);
        out.push_str("  ");
        out.push_str(&escape_text(text));
        out.push('\n');
    }
    for child in &node.children {
        write_node(child, depth + 1, out);
    }
    out.push_str(&indent);
    out.push_str("</");
    out.push_str(&node.tag);
    out.push_str(">\n");
}

type PResult<T> = std::result::Result<T, (usize, String)>;

fn parse(source: &str) -> PResult<Document> {
    let mut stack: Vec<Node> = Vec::new();
    let mut root: Option<Node> = None;
    let mut cursor = 0;
    while let Some(found) = source[cursor..].find('<') {
        let open = cursor + found;
        let text = source[cursor..open].trim();
        if !text.is_empty() {
            let Some(parent) = stack.last_mut() else {
                return Err((
                    cursor + source[cursor..].len() - source[cursor..].trim_start().len(),
                    "text outside an element".into(),
                ));
            };
            let text = unescape(text);
            if let Some(previous) = parent.attr_mut("_text") {
                previous.push(' ');
                previous.push_str(&text);
            } else {
                parent.attributes.push(("_text".into(), text));
                parent.attr_spans.push(None);
            }
        }
        if source[open..].starts_with("<!--") {
            let end = source[open + 4..].find("-->").ok_or((open, "unterminated comment".to_string()))?;
            cursor = open + 4 + end + 3;
            continue;
        }
        // Find the end of the tag, skipping `>` inside quotes.
        let mut quote = None;
        let mut close = None;
        for (index, ch) in source[open + 1..].char_indices() {
            match quote {
                Some(active) if active == ch => quote = None,
                Some(_) => {}
                None if ch == '"' || ch == '\'' => quote = Some(ch),
                None if ch == '>' => {
                    close = Some(open + 1 + index);
                    break;
                }
                None if ch == '<' => return Err((open, "unterminated tag (missing `>`)".into())),
                None => {}
            }
        }
        let close = close.ok_or((open, "unterminated tag (missing `>`)".to_string()))?;
        let inner = &source[open + 1..close];
        if inner.starts_with('!') || inner.starts_with('?') {
            cursor = close + 1;
            continue;
        }
        if let Some(name) = inner.strip_prefix('/') {
            let name = name.trim().to_ascii_lowercase();
            let mut node = stack.pop().ok_or((open, format!("unexpected closing tag </{name}>")))?;
            if node.tag != name {
                return Err((open, format!("closing tag </{name}> does not match <{}>", node.tag)));
            }
            node.span.end = close + 1;
            node.close = open..close + 1;
            attach(&mut stack, &mut root, node, open)?;
        } else {
            let self_closing = inner.trim_end().ends_with('/');
            let body_end = if self_closing { open + 1 + inner.trim_end().len() - 1 } else { close };
            let mut node = parse_open_tag(source, open + 1, body_end)?;
            node.span = open..close + 1;
            node.self_closing = self_closing;
            if self_closing {
                // The `/` of `/>` (keep any whitespace before it in the attribute area).
                node.close = body_end..close + 1;
                attach(&mut stack, &mut root, node, open)?;
            } else {
                stack.push(node);
            }
        }
        cursor = close + 1;
    }
    let rest = source[cursor..].trim();
    if !rest.is_empty() {
        return Err((cursor, "text outside an element".into()));
    }
    if let Some(open) = stack.last() {
        return Err((open.span.start, format!("unclosed tag <{}>", open.tag)));
    }
    let root = root.ok_or((0, "interface is empty".to_string()))?;
    Ok(Document { root })
}

fn attach(stack: &mut [Node], root: &mut Option<Node>, node: Node, at: usize) -> PResult<()> {
    if let Some(parent) = stack.last_mut() {
        parent.children.push(node);
    } else if root.replace(node).is_some() {
        return Err((at, "an interface can only have one root element".into()));
    }
    Ok(())
}

/// Parse `tag attr="v" …` between `start` (just after `<`) and `end` (the `>` or `/`).
fn parse_open_tag(source: &str, start: usize, end: usize) -> PResult<Node> {
    let s = &source[..end];
    let mut i = start;
    let skip_ws = |i: &mut usize| {
        while *i < end && s[*i..].starts_with(|c: char| c.is_whitespace()) {
            *i += s[*i..].chars().next().unwrap().len_utf8();
        }
    };
    skip_ws(&mut i);
    let tag_start = i;
    while i < end && !s[i..].starts_with(|c: char| c.is_whitespace()) {
        i += s[i..].chars().next().unwrap().len_utf8();
    }
    let tag = s[tag_start..i].to_ascii_lowercase();
    if tag.is_empty() {
        return Err((start, "empty tag".into()));
    }
    let mut node = Node::new(tag);
    node.attr_insert = i;
    loop {
        skip_ws(&mut i);
        if i >= end {
            break;
        }
        let key_start = i;
        while i < end && !s[i..].starts_with(|c: char| c.is_whitespace() || c == '=') {
            i += s[i..].chars().next().unwrap().len_utf8();
        }
        let key = s[key_start..i].to_owned();
        if key.is_empty() {
            return Err((key_start, "invalid attribute".into()));
        }
        let mut j = i;
        skip_ws(&mut j);
        let (value, value_span, whole_end) = if j < end && s[j..].starts_with('=') {
            j += 1;
            skip_ws(&mut j);
            if j < end && (s[j..].starts_with('"') || s[j..].starts_with('\'')) {
                let quote = s[j..].chars().next().unwrap();
                let value_start = j + 1;
                let value_end = s[value_start..end]
                    .find(quote)
                    .map(|p| value_start + p)
                    .ok_or((key_start, format!("attribute `{key}` is missing its closing quote")))?;
                (unescape(&s[value_start..value_end]), value_start..value_end, value_end + 1)
            } else {
                let value_start = j;
                while j < end && !s[j..].starts_with(|c: char| c.is_whitespace()) {
                    j += s[j..].chars().next().unwrap().len_utf8();
                }
                (unescape(&s[value_start..j]), value_start..j, j)
            }
        } else {
            (String::new(), i..i, i)
        };
        node.attributes.push((key, value));
        node.attr_spans.push(Some(AttrSpan { whole: key_start..whole_end, value: value_span }));
        i = whole_end;
        node.attr_insert = whole_end;
    }
    Ok(node)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_point_at_values() {
        let src = r#"<interface>
  <panel id="menu" style="width: 50%; background: #112233">
    <label id="title" text="Hi &amp; bye" />
  </panel>
</interface>"#;
        let doc = Document::parse(src).unwrap();
        let panel = &doc.root.children[0];
        assert_eq!(&src[panel.attr_span("id").unwrap().value.clone()], "menu");
        assert_eq!(&src[panel.attr_span("style").unwrap().value.clone()], "width: 50%; background: #112233");
        let label = &panel.children[0];
        assert_eq!(label.attr("text"), Some("Hi & bye"));
        assert_eq!(&src[label.span.clone()], r#"<label id="title" text="Hi &amp; bye" />"#);
        assert!(src[panel.span.clone()].ends_with("</panel>"));
        assert_eq!(&src[panel.close.clone()], "</panel>");
        assert_eq!(&src[label.close.clone()], "/>");
    }

    #[test]
    fn errors_have_line_and_column() {
        let err = Document::parse_detailed("<interface>\n  <panel>\n</interface>").unwrap_err();
        assert_eq!(err.line, 3);
        assert!(err.message.contains("does not match"));
    }

    #[test]
    fn path_and_lookup() {
        let doc = Document::parse(r#"<interface><panel><row><button id="go"/></row></panel></interface>"#).unwrap();
        assert_eq!(doc.root.path_of("go"), Some(vec![0, 0, 0]));
        assert_eq!(doc.root.at(&[0, 0, 0]).unwrap().tag, "button");
    }
}
