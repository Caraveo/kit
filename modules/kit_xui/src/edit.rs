//! Minimal source edits. The designer never re-serializes a whole document:
//! each change rewrites only the characters it has to, so the author's
//! formatting, attribute order and comments survive visual editing.

use crate::doc::{Document, Node, escape_attr};
use anyhow::{Context, Result, bail};
use std::ops::Range;

/// One replacement in the source.
#[derive(Clone, Debug)]
struct Splice {
    range: Range<usize>,
    text: String,
}

fn apply(source: &str, mut splices: Vec<Splice>) -> String {
    splices.sort_by_key(|s| std::cmp::Reverse(s.range.start));
    let mut out = source.to_owned();
    for s in splices {
        out.replace_range(s.range, &s.text);
    }
    out
}

/// Find `name: value` inside a style string; returns (declaration range, value range) relative to `style`.
fn style_declaration(style: &str, name: &str) -> Option<(Range<usize>, Range<usize>)> {
    let mut start = 0;
    for part in style.split(';') {
        let end = start + part.len();
        if let Some((property, _)) = part.split_once(':') {
            if property.trim().eq_ignore_ascii_case(name) {
                let colon = start + part.find(':').unwrap();
                let raw = &style[colon + 1..end];
                let lead = raw.len() - raw.trim_start().len();
                let value = colon + 1 + lead..colon + 1 + raw.trim_end().len();
                return Some((start..(end + 1).min(style.len()), value));
            }
        }
        start = end + 1;
    }
    None
}

/// Set a property on the element at `path`. Updates the attribute if there is
/// one, else the `style` declaration, else adds to `style` (when the element
/// uses one) or adds a new attribute.
pub fn set_property(source: &str, doc: &Document, path: &[usize], name: &str, value: &str) -> Result<String> {
    let node = doc.root.at(path).context("element not found")?;
    let mut splices = Vec::new();
    let mut done = false;
    if let Some(span) = node.attr_span(name) {
        splices.push(Splice { range: span.value.clone(), text: escape_attr(value) });
        if span.value.is_empty() && !source[span.whole.clone()].contains('=') {
            // A bare attribute (`<x hidden>`): give it a value.
            splices.pop();
            splices.push(Splice { range: span.whole.clone(), text: format!("{name}=\"{}\"", escape_attr(value)) });
        } else if span.value.start > 0 && !matches!(&source[span.value.start - 1..span.value.start], "\"" | "'") {
            splices.pop();
            splices.push(Splice { range: span.value.clone(), text: format!("\"{}\"", escape_attr(value)) });
        }
        done = true;
    }
    if let Some(style_span) = node.attr_span("style") {
        let style = &source[style_span.value.clone()];
        if let Some((_, value_range)) = style_declaration(style, name) {
            let at = style_span.value.start;
            splices.push(Splice { range: at + value_range.start..at + value_range.end, text: escape_attr(value) });
            done = true;
        } else if !done {
            let trimmed = style.trim_end();
            let insert = style_span.value.start + trimmed.len();
            let text = if trimmed.is_empty() {
                format!("{name}: {}", escape_attr(value))
            } else if trimmed.ends_with(';') {
                format!(" {name}: {};", escape_attr(value))
            } else {
                format!("; {name}: {}", escape_attr(value))
            };
            splices.push(Splice { range: insert..insert, text });
            done = true;
        }
    }
    if !done {
        splices.push(Splice { range: node.attr_insert..node.attr_insert, text: format!(" {name}=\"{}\"", escape_attr(value)) });
    }
    Ok(apply(source, splices))
}

/// Remove a property from both the attributes and the `style`.
pub fn remove_property(source: &str, doc: &Document, path: &[usize], name: &str) -> Result<String> {
    let node = doc.root.at(path).context("element not found")?;
    let mut splices = Vec::new();
    if let Some(span) = node.attr_span(name) {
        // Remove the attribute and the whitespace before it.
        let lead = source[..span.whole.start].len() - source[..span.whole.start].trim_end().len();
        splices.push(Splice { range: span.whole.start - lead..span.whole.end, text: String::new() });
    }
    if let Some(style_span) = node.attr_span("style") {
        let style = &source[style_span.value.clone()];
        if let Some((decl, _)) = style_declaration(style, name) {
            let at = style_span.value.start;
            // Also drop the space after the `;` we remove.
            let mut end = decl.end;
            while end < style.len() && style[end..].starts_with(' ') {
                end += 1;
            }
            let mut start = decl.start;
            if end >= style.len() {
                // Last declaration: remove the `;` before it instead.
                start = style[..decl.start].trim_end().trim_end_matches(';').len();
                end = style.len();
            }
            splices.push(Splice { range: at + start..at + end, text: String::new() });
        }
    }
    Ok(apply(source, splices))
}

/// Set (or with `None`, remove) several properties in one edit.
pub fn set_properties(source: &str, path: &[usize], props: &[(&str, Option<String>)]) -> Result<String> {
    let mut source = source.to_owned();
    for (name, value) in props {
        let doc = Document::parse(&source)?;
        source = match value {
            Some(v) => set_property(&source, &doc, path, name, v)?,
            None => remove_property(&source, &doc, path, name)?,
        };
    }
    Ok(source)
}

fn line_start(source: &str, at: usize) -> usize {
    source[..at].rfind('\n').map_or(0, |p| p + 1)
}

/// The indentation of the line an element starts on.
fn indent_of(source: &str, node: &Node) -> String {
    let start = line_start(source, node.span.start);
    source[start..node.span.start].chars().take_while(|c| c.is_whitespace()).collect()
}

/// Re-indent a snippet so its first line has `indent` and nested lines keep their relative indentation.
fn reindent(snippet: &str, indent: &str) -> String {
    let lines: Vec<&str> = snippet.trim_matches('\n').lines().collect();
    let common = lines
        .iter()
        .skip(1)
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);
    let first_extra = 0;
    lines
        .iter()
        .enumerate()
        .map(|(i, l)| {
            if i == 0 {
                format!("{indent}{}", l.trim_start())
            } else if l.trim().is_empty() {
                String::new()
            } else {
                // Nested lines: keep their offset relative to the snippet's common indentation, plus 2.
                let rel = (l.len() - l.trim_start().len()).saturating_sub(common);
                let base = if common > 0 { 2 } else { first_extra };
                format!("{indent}{}{}", " ".repeat(rel + base), l.trim_start())
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Insert `snippet` (one element, any formatting) as child `index` of the
/// element at `parent`. Returns the new source.
pub fn insert_child(source: &str, doc: &Document, parent: &[usize], index: usize, snippet: &str) -> Result<String> {
    let node = doc.root.at(parent).context("parent not found")?;
    let parent_indent = indent_of(source, node);
    let child_indent = node
        .children
        .first()
        .map(|c| indent_of(source, c))
        .filter(|i| i.len() > parent_indent.len())
        .unwrap_or_else(|| format!("{parent_indent}  "));
    let block = reindent(snippet, &child_indent);
    if node.self_closing {
        // `<panel ... />` → `<panel ...>\n  child\n</panel>`
        let range = node.close.start..node.close.end;
        let before = &source[..range.start];
        let trim = before.len() - before.trim_end().len();
        let text = format!(">\n{block}\n{parent_indent}</{}>", node.tag);
        return Ok(apply(source, vec![Splice { range: range.start - trim..range.end, text }]));
    }
    let index = index.min(node.children.len());
    let at = if let Some(next) = node.children.get(index) {
        let ls = line_start(source, next.span.start);
        if source[ls..next.span.start].trim().is_empty() {
            return Ok(apply(source, vec![Splice { range: ls..ls, text: format!("{block}\n") }]));
        }
        next.span.start
    } else {
        let ls = line_start(source, node.close.start);
        if source[ls..node.close.start].trim().is_empty() {
            return Ok(apply(source, vec![Splice { range: ls..ls, text: format!("{block}\n") }]));
        }
        node.close.start
    };
    let text = format!("\n{block}\n{}", if index >= node.children.len() { parent_indent.as_str() } else { child_indent.as_str() });
    Ok(apply(source, vec![Splice { range: at..at, text }]))
}

/// The element's text, including nested children, exactly as written.
pub fn element_source<'a>(source: &'a str, doc: &Document, path: &[usize]) -> Result<&'a str> {
    let node = doc.root.at(path).context("element not found")?;
    Ok(&source[node.span.clone()])
}

/// Remove the element at `path` (and its line, when it's alone on it).
pub fn remove_element(source: &str, doc: &Document, path: &[usize]) -> Result<String> {
    if path.is_empty() {
        bail!("can't remove the root element");
    }
    let node = doc.root.at(path).context("element not found")?;
    let ls = line_start(source, node.span.start);
    let after = &source[node.span.end..];
    let rest_of_line = after.split('\n').next().unwrap_or("");
    let range = if source[ls..node.span.start].trim().is_empty() && rest_of_line.trim().is_empty() {
        let end = node.span.end + rest_of_line.len() + usize::from(after.len() > rest_of_line.len());
        ls..end
    } else {
        node.span.clone()
    };
    Ok(apply(source, vec![Splice { range, text: String::new() }]))
}

/// Move the element at `from` to be child `index` of `to_parent` (a path in
/// the document *before* the move). Returns the new source and the element's new path.
pub fn move_element(source: &str, doc: &Document, from: &[usize], to_parent: &[usize], index: usize) -> Result<(String, Vec<usize>)> {
    if from.is_empty() {
        bail!("can't move the root element");
    }
    if to_parent.starts_with(from) {
        bail!("can't move an element into itself");
    }
    let snippet = element_source(source, doc, from)?.to_owned();
    let without = remove_element(source, doc, from)?;
    // Adjust the destination for the removal.
    let mut parent = to_parent.to_vec();
    let mut index = index;
    let (from_parent, from_index) = from.split_at(from.len() - 1);
    let from_index = from_index[0];
    if parent.len() > from_parent.len() && parent.starts_with(from_parent) && parent[from_parent.len()] > from_index {
        parent[from_parent.len()] -= 1;
    }
    if parent == from_parent && index > from_index {
        index -= 1;
    }
    let doc2 = Document::parse(&without)?;
    let out = insert_child(&without, &doc2, &parent, index, &snippet)?;
    let count = doc2.root.at(&parent).map_or(0, |p| p.children.len());
    let mut new_path = parent;
    new_path.push(index.min(count));
    Ok((out, new_path))
}

/// Duplicate the element at `path` right after itself, giving every id in the
/// copy a fresh unique one. Returns the new source and the copy's path.
pub fn duplicate_element(source: &str, doc: &Document, path: &[usize]) -> Result<(String, Vec<usize>)> {
    let node = doc.root.at(path).context("element not found")?;
    let mut snippet = source[node.span.clone()].to_owned();
    let mut taken = doc.root.ids();
    for id in node.ids() {
        let fresh = unique_id(&id, &taken);
        taken.push(fresh.clone());
        for quote in ['"', '\''] {
            snippet = snippet.replace(&format!("id={quote}{id}{quote}"), &format!("id={quote}{fresh}{quote}"));
        }
    }
    let (parent, index) = path.split_at(path.len() - 1);
    let out = insert_child(source, doc, parent, index[0] + 1, &snippet)?;
    let mut new_path = parent.to_vec();
    new_path.push(index[0] + 1);
    Ok((out, new_path))
}

/// A unique id based on `base` (`button` → `button-2`, `button-3`, …).
pub fn unique_id(base: &str, taken: &[String]) -> String {
    let stem = base.trim_end_matches(|c: char| c.is_ascii_digit()).trim_end_matches('-');
    let stem = if stem.is_empty() { "item" } else { stem };
    if !taken.iter().any(|t| t == stem) {
        return stem.to_owned();
    }
    (2..).map(|n| format!("{stem}-{n}")).find(|c| !taken.iter().any(|t| t == c)).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = r#"<interface>
  <!-- the main menu -->
  <panel id="menu" style="width: 50%;  background: #112233">
    <label id="title" text="Hi" />
    <button id="go"
            text="Go" />
  </panel>
</interface>
"#;

    fn doc(s: &str) -> Document {
        Document::parse(s).unwrap()
    }

    #[test]
    fn set_updates_style_value_in_place() {
        let out = set_property(SRC, &doc(SRC), &[0], "width", "60%").unwrap();
        assert_eq!(out, SRC.replace("width: 50%;", "width: 60%;"));
    }

    #[test]
    fn set_updates_attribute_in_place_and_keeps_layout() {
        let out = set_property(SRC, &doc(SRC), &[0, 1], "text", "Start \"now\"").unwrap();
        assert!(out.contains("<button id=\"go\"\n            text=\"Start &quot;now&quot;\" />"));
        assert!(out.contains("<!-- the main menu -->"));
    }

    #[test]
    fn set_adds_to_style_or_attribute() {
        let out = set_property(SRC, &doc(SRC), &[0], "padding", "1em").unwrap();
        assert!(out.contains(r##"style="width: 50%;  background: #112233; padding: 1em""##));
        let out = set_property(SRC, &doc(SRC), &[0, 0], "color", "#fff").unwrap();
        assert!(out.contains(r##"<label id="title" text="Hi" color="#fff" />"##));
    }

    #[test]
    fn remove_property_from_style_and_attrs() {
        let out = remove_property(SRC, &doc(SRC), &[0], "background").unwrap();
        assert!(out.contains(r#"style="width: 50%""#), "{out}");
        let out = remove_property(SRC, &doc(SRC), &[0], "width").unwrap();
        assert!(out.contains(r##"style="background: #112233""##), "{out}");
        let out = remove_property(SRC, &doc(SRC), &[0, 0], "text").unwrap();
        assert!(out.contains(r#"<label id="title" />"#), "{out}");
    }

    #[test]
    fn insert_remove_move_duplicate() {
        let d = doc(SRC);
        let out = insert_child(SRC, &d, &[0], 1, r#"<image id="pic" />"#).unwrap();
        assert!(out.contains("<label id=\"title\" text=\"Hi\" />\n    <image id=\"pic\" />\n    <button"), "{out}");
        let out = insert_child(SRC, &d, &[0], 9, r#"<row id="r"><label id="a"/></row>"#).unwrap();
        assert_eq!(doc(&out).root.at(&[0, 2]).unwrap().tag, "row");
        let out = insert_child(SRC, &d, &[0, 0], 0, r#"<label id="inner" />"#).unwrap();
        assert!(out.contains("<label id=\"title\" text=\"Hi\">\n      <label id=\"inner\" />\n    </label>"), "{out}");

        let out = remove_element(SRC, &d, &[0, 0]).unwrap();
        assert!(!out.contains("title") && out.contains("<!-- the main menu -->"));
        assert!(!out.contains("\n\n"), "{out}");

        let (out, path) = move_element(SRC, &d, &[0, 1], &[0], 0).unwrap();
        assert_eq!(path, vec![0, 0]);
        assert_eq!(doc(&out).root.at(&[0, 0]).unwrap().id(), Some("go"));
        assert_eq!(doc(&out).root.at(&[0, 1]).unwrap().id(), Some("title"));

        let (out, path) = move_element(SRC, &d, &[0, 0], &[], 0).unwrap();
        assert_eq!(path, vec![0]);
        assert_eq!(doc(&out).root.children.len(), 2);

        let (out, path) = duplicate_element(SRC, &d, &[0, 0]).unwrap();
        assert_eq!(path, vec![0, 1]);
        assert_eq!(doc(&out).root.at(&[0, 1]).unwrap().id(), Some("title-2"));
    }

    #[test]
    fn unique_ids() {
        let taken = vec!["button".to_string(), "button-2".to_string()];
        assert_eq!(unique_id("button", &taken), "button-3");
        assert_eq!(unique_id("label", &taken), "label");
    }
}
