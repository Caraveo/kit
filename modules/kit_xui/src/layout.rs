//! Layout. Two kinds of placement:
//!
//! - **Free** elements (children of `<interface>`, or anything with an `anchor`
//!   or `position: absolute`) are pinned to their parent: `anchor` picks the
//!   point or edges (`top-left`, `center`, `bottom-right`, `stretch`,
//!   `stretch-top`, …), `x`/`y` offset from that point, and stretched axes use
//!   `left`/`right`/`top`/`bottom` insets.
//! - **Flow** elements sit in a `row` or a column (`panel`, `column`) and are
//!   arranged with `gap`, `padding`, `margin`, `align`, `justify`, and sizes in
//!   px, `em`, `%`, `fill` / `2fr`, or `auto` (fit the content).
//!
//! Sizes can scale with the screen: `<interface width="1920" height="1080">`
//! designs at 1080p and scales px/em to whatever the game window is.

use crate::doc::{Document, Node};
use crate::style::{Align, Anchor, EM, HAnchor, Justify, Length, VAnchor, edges, length, prop};
use epaint::{Pos2, Rect, Vec2, pos2, vec2};
use std::collections::HashMap;

/// Measures text (implemented with epaint fonts in [`crate::paint`]).
pub trait Measure {
    fn text(&mut self, text: &str, size: f32, wrap: Option<f32>) -> Vec2;
}

/// A rough measurer for tests and headless use (0.55em per character, 1.25em lines).
pub struct ApproxMeasure;

impl Measure for ApproxMeasure {
    fn text(&mut self, text: &str, size: f32, wrap: Option<f32>) -> Vec2 {
        let w = text.chars().count() as f32 * size * 0.55;
        match wrap {
            Some(max) if max > 0.0 && w > max => vec2(max, (w / max).ceil() * size * 1.25),
            _ => vec2(w, size * 1.25),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    Row,
    Column,
}

/// One placed element.
#[derive(Clone, Debug)]
pub struct Item {
    /// Path of the element in the document (child indexes from `<interface>`).
    /// Elements from an included interface carry the `<xui>` element's path.
    pub path: Vec<usize>,
    /// Content from an `<xui interface="…">` include (shown, not editable here).
    pub included: bool,
    /// A copy of the element without its children (for painting).
    pub node: Node,
    pub rect: Rect,
    /// The area drawing is clipped to.
    pub clip: Rect,
    /// Free elements: the frame they're anchored in. Flow elements: the
    /// parent's content box.
    pub frame: Rect,
    /// Pinned to its parent (true) or arranged in a row/column (false).
    pub free: bool,
    pub anchor: Anchor,
    /// For containers: the direction children flow and the content box.
    pub axis: Option<Axis>,
    pub content: Rect,
    pub depth: usize,
    pub scale: f32,
}

#[derive(Clone, Debug)]
pub struct Layout {
    /// In paint order (parents before children).
    pub items: Vec<Item>,
    pub scale: f32,
    pub screen: Rect,
}

impl Layout {
    pub fn get(&self, path: &[usize]) -> Option<&Item> {
        self.items.iter().find(|i| !i.included && i.path == path)
    }

    /// The topmost element under `pos` (optionally only editable ones).
    pub fn hit(&self, pos: Pos2, editable_only: bool) -> Option<&Item> {
        self.items.iter().rev().find(|i| (!editable_only || !i.included) && i.rect.contains(pos) && i.clip.contains(pos))
    }

    /// The deepest container under `pos` (drop target), excluding `except` and its descendants.
    pub fn container_at(&self, pos: Pos2, except: Option<&[usize]>) -> Option<&Item> {
        self.items.iter().rev().find(|i| {
            !i.included
                && i.axis.is_some()
                && i.rect.contains(pos)
                && i.clip.contains(pos)
                && except.is_none_or(|e| !i.path.starts_with(e))
        })
    }
}

/// Inputs besides the document.
pub struct Options<'a> {
    /// Visibility overrides by element id (runtime `SetVisible`).
    pub visible: Option<&'a HashMap<String, bool>>,
    /// Loads `<xui interface="…">` includes.
    pub include: &'a dyn Fn(&str) -> Option<Document>,
}

impl Default for Options<'_> {
    fn default() -> Self {
        Self { visible: None, include: &|_| None }
    }
}

/// The interface's scale for a screen: `width`/`height` on `<interface>` are its design size.
pub fn scale_for(root: &Node, screen: Vec2) -> f32 {
    if let Some(s) = root.attr("scale").and_then(|s| s.parse::<f32>().ok()).filter(|s| *s > 0.0) {
        return s;
    }
    let w = root.attr("width").and_then(|v| v.trim_end_matches("px").parse::<f32>().ok()).filter(|v| *v > 0.0);
    let h = root.attr("height").and_then(|v| v.trim_end_matches("px").parse::<f32>().ok()).filter(|v| *v > 0.0);
    match (w, h) {
        (Some(w), Some(h)) => (screen.x / w).min(screen.y / h),
        (Some(w), None) => screen.x / w,
        (None, Some(h)) => screen.y / h,
        _ => 1.0,
    }
    .max(0.05)
}

pub fn layout(doc: &Document, screen: Rect, measure: &mut dyn Measure, options: &Options<'_>) -> Layout {
    let scale = scale_for(&doc.root, screen.size());
    let mut ctx = Ctx { measure, options, scale, items: Vec::new(), depth_guard: 0 };
    for (i, child) in doc.root.children.iter().enumerate() {
        ctx.free(child, vec![i], false, screen, screen, 0);
    }
    Layout { items: ctx.items, scale, screen }
}

struct Ctx<'a, 'b> {
    measure: &'a mut dyn Measure,
    options: &'a Options<'b>,
    scale: f32,
    items: Vec<Item>,
    depth_guard: usize,
}

fn axis_of(node: &Node) -> Option<Axis> {
    if !node.is_container() && !node.children.iter().any(|c| c.tag != "logic" && c.tag != "add-class") {
        return None;
    }
    let display = prop(node, "display").unwrap_or("");
    let dir = prop(node, "flex-direction").or_else(|| prop(node, "direction"));
    Some(match (node.tag.as_str(), dir) {
        (_, Some(d)) if d.trim().starts_with("row") && (display.is_empty() || display == "flex" || node.tag == "row") => Axis::Row,
        (_, Some(d)) if d.trim().starts_with("column") => Axis::Column,
        ("row" | "grid", _) => Axis::Row,
        _ => Axis::Column,
    })
}

fn is_free(node: &Node) -> bool {
    prop(node, "anchor").is_some() || prop(node, "position").is_some_and(|p| p.eq_ignore_ascii_case("absolute"))
}

/// Not laid out (logic, hidden).
fn skipped(node: &Node, options: &Options<'_>) -> bool {
    if matches!(node.tag.as_str(), "logic" | "add-class") {
        return true;
    }
    if let Some(v) = node.id().and_then(|id| options.visible.and_then(|m| m.get(id))) {
        return !*v;
    }
    prop(node, "visible").is_some_and(|v| v.eq_ignore_ascii_case("false")) || prop(node, "display").is_some_and(|v| v == "none")
}

fn default_padding(node: &Node) -> &'static str {
    match node.tag.as_str() {
        "button" => "0.5em 1em",
        "panel" => "1em",
        _ => "0",
    }
}

fn font_size(node: &Node, scale: f32) -> f32 {
    length(node, "font-size").and_then(|l| l.resolve(EM, scale)).unwrap_or(18.0 * scale).clamp(1.0, 512.0)
}

impl Ctx<'_, '_> {
    fn padding(&self, node: &Node, parent_w: f32) -> [f32; 4] {
        edges(prop(node, "padding").unwrap_or(default_padding(node)), parent_w, self.scale)
    }

    fn margin(&self, node: &Node, parent_w: f32) -> [f32; 4] {
        prop(node, "margin").map_or([0.0; 4], |m| edges(m, parent_w, self.scale))
    }

    /// The size an element wants for its content, given the width available.
    fn intrinsic(&mut self, node: &Node, avail_w: f32) -> Vec2 {
        let s = self.scale;
        let pad = self.padding(node, avail_w);
        let explicit_w = length(node, "width").and_then(|l| l.resolve(avail_w, s));
        let inner_w = (explicit_w.unwrap_or(avail_w) - pad[1] - pad[3]).max(0.0);
        let content = match node.tag.as_str() {
            "label" | "button" => {
                let text = node.text().unwrap_or("");
                let size = font_size(node, s);
                let t = self.measure.text(if text.is_empty() { " " } else { text }, size, Some(inner_w));
                if node.tag == "button" { vec2(t.x, t.y.max(size * 1.4)) } else { t }
            }
            "image" => vec2(64.0 * s, 64.0 * s),
            _ => match axis_of(node) {
                Some(axis) => {
                    let gap = length(node, "gap").and_then(|l| l.resolve(inner_w, s)).unwrap_or(8.0 * s);
                    let mut main: f32 = 0.0;
                    let mut cross: f32 = 0.0;
                    let mut count: usize = 0;
                    for child in node.children.iter().filter(|c| !skipped(c, self.options) && !is_free(c)) {
                        let m = self.margin(child, inner_w);
                        let size = self.preferred(child, inner_w, (inner_w, f32::INFINITY));
                        let (a, b) = if axis == Axis::Row { (size.x + m[1] + m[3], size.y + m[0] + m[2]) } else { (size.y + m[0] + m[2], size.x + m[1] + m[3]) };
                        main += a;
                        cross = cross.max(b);
                        count += 1;
                    }
                    main += gap * count.saturating_sub(1) as f32;
                    if axis == Axis::Row { vec2(main, cross) } else { vec2(cross, main) }
                }
                None => Vec2::ZERO,
            },
        };
        vec2(content.x + pad[1] + pad[3], content.y + pad[0] + pad[2])
    }

    /// Explicit size where given, else intrinsic.
    fn preferred(&mut self, node: &Node, avail_w: f32, parent: (f32, f32)) -> Vec2 {
        let s = self.scale;
        let w = length(node, "width").and_then(|l| l.resolve(parent.0, s));
        let h = length(node, "height").and_then(|l| l.resolve(if parent.1.is_finite() { parent.1 } else { 0.0 }, s));
        if let (Some(w), Some(h)) = (w, h) {
            return vec2(w, h);
        }
        let i = self.intrinsic(node, w.unwrap_or(avail_w));
        vec2(w.unwrap_or(i.x), h.unwrap_or(i.y))
    }

    /// Place a free (anchored) element inside `frame`.
    fn free(&mut self, node: &Node, path: Vec<usize>, included: bool, frame: Rect, clip: Rect, depth: usize) {
        if skipped(node, self.options) {
            return;
        }
        if node.tag == "xui" {
            return self.include(node, path, frame, clip, depth);
        }
        let s = self.scale;
        let anchor = prop(node, "anchor").map(Anchor::parse).unwrap_or_default();
        let len = |name: &str, parent: f32| length(node, name).and_then(|l| l.resolve(parent, s));
        let container = axis_of(node).is_some();
        let (w, h) = {
            let w = match anchor.h {
                HAnchor::Stretch => (frame.width() - len("left", frame.width()).unwrap_or(0.0) - len("right", frame.width()).unwrap_or(0.0)).max(0.0),
                _ => match length(node, "width") {
                    Some(Length::Fill(_)) => frame.width(),
                    Some(l) => l.resolve(frame.width(), s).unwrap_or(0.0),
                    None if container => frame.width(),
                    None => self.intrinsic(node, frame.width()).x,
                },
            };
            let h = match anchor.v {
                VAnchor::Stretch => (frame.height() - len("top", frame.height()).unwrap_or(0.0) - len("bottom", frame.height()).unwrap_or(0.0)).max(0.0),
                _ => match length(node, "height") {
                    Some(Length::Fill(_)) => frame.height(),
                    Some(l) => l.resolve(frame.height(), s).unwrap_or(0.0),
                    None => self.intrinsic(node, w).y,
                },
            };
            (w, h)
        };
        let x_off = len("x", frame.width());
        let y_off = len("y", frame.height());
        let x = match anchor.h {
            HAnchor::Left => frame.left() + x_off.or_else(|| len("left", frame.width())).unwrap_or(0.0),
            HAnchor::Center => frame.center().x + x_off.unwrap_or(0.0) - w / 2.0,
            HAnchor::Right => match len("right", frame.width()) {
                Some(r) if x_off.is_none() => frame.right() - r - w,
                _ => frame.right() + x_off.unwrap_or(0.0) - w,
            },
            HAnchor::Stretch => frame.left() + len("left", frame.width()).unwrap_or(0.0),
        };
        let y = match anchor.v {
            VAnchor::Top => frame.top() + y_off.or_else(|| len("top", frame.height())).unwrap_or(0.0),
            VAnchor::Middle => frame.center().y + y_off.unwrap_or(0.0) - h / 2.0,
            VAnchor::Bottom => match len("bottom", frame.height()) {
                Some(b) if y_off.is_none() => frame.bottom() - b - h,
                _ => frame.bottom() + y_off.unwrap_or(0.0) - h,
            },
            VAnchor::Stretch => frame.top() + len("top", frame.height()).unwrap_or(0.0),
        };
        let rect = Rect::from_min_size(pos2(x, y), vec2(w.max(0.0), h.max(0.0)));
        self.place(node, path, included, rect, frame, clip, true, anchor, depth);
    }

    fn include(&mut self, node: &Node, path: Vec<usize>, frame: Rect, clip: Rect, depth: usize) {
        let Some(asset) = node.attr("interface") else { return };
        if self.depth_guard >= 8 {
            return;
        }
        let Some(doc) = (self.options.include)(asset) else { return };
        self.depth_guard += 1;
        for child in &doc.root.children {
            self.free(child, path.clone(), true, frame, clip, depth + 1);
        }
        self.depth_guard -= 1;
    }

    /// Record an element at `rect` and lay out its children.
    #[allow(clippy::too_many_arguments)]
    fn place(&mut self, node: &Node, path: Vec<usize>, included: bool, rect: Rect, frame: Rect, clip: Rect, free: bool, anchor: Anchor, depth: usize) {
        let axis = axis_of(node);
        let pad = self.padding(node, rect.width());
        let content = Rect::from_min_max(
            pos2(rect.left() + pad[3], rect.top() + pad[0]),
            pos2((rect.right() - pad[1]).max(rect.left() + pad[3]), (rect.bottom() - pad[2]).max(rect.top() + pad[0])),
        );
        let own_clip = if prop(node, "overflow").is_some_and(|o| o == "hidden" || o == "clip") { clip.intersect(rect) } else { clip };
        self.items.push(Item {
            path: path.clone(),
            included,
            node: Node { children: Vec::new(), ..node.clone() },
            rect,
            clip,
            frame,
            free,
            anchor,
            axis,
            content,
            depth,
            scale: self.scale,
        });
        let Some(axis) = axis else { return };
        // Free children are anchored to this element's box.
        let mut flow = Vec::new();
        for (i, child) in node.children.iter().enumerate() {
            if skipped(child, self.options) {
                continue;
            }
            let mut p = path.clone();
            p.push(i);
            if is_free(child) || child.tag == "xui" {
                self.free(child, p, included, rect, own_clip, depth + 1);
            } else {
                flow.push((child, p));
            }
        }
        self.flow(node, &flow, axis, content, own_clip, included, depth + 1);
    }

    #[allow(clippy::too_many_arguments)]
    fn flow(&mut self, parent: &Node, children: &[(&Node, Vec<usize>)], axis: Axis, content: Rect, clip: Rect, included: bool, depth: usize) {
        if children.is_empty() {
            return;
        }
        let s = self.scale;
        let row = axis == Axis::Row;
        let (main_total, cross_total) = if row { (content.width(), content.height()) } else { (content.height(), content.width()) };
        let gap = length(parent, "gap").and_then(|l| l.resolve(main_total, s)).unwrap_or(8.0 * s);
        let align = prop(parent, "align").or_else(|| prop(parent, "align-items")).map_or(Align::Stretch, Align::parse);
        let justify = prop(parent, "justify").or_else(|| prop(parent, "justify-content")).map_or(Justify::Start, Justify::parse);
        struct Slot {
            main: f32,
            fill: f32,
            cross: Option<f32>,
            margin: [f32; 4],
        }
        let mut slots = Vec::new();
        for (child, _) in children {
            let margin = self.margin(child, content.width());
            let (main_name, cross_name) = if row { ("width", "height") } else { ("height", "width") };
            let main_len = length(child, main_name);
            let cross_len = length(child, cross_name);
            let cross = match cross_len {
                Some(Length::Fill(_)) => None,
                Some(l) => l.resolve(cross_total, s),
                None => None,
            };
            let (main, fill) = match main_len {
                Some(Length::Fill(w)) => (0.0, w),
                Some(Length::Auto) | None if row && axis_of(child).is_some() => (0.0, 1.0),
                Some(l @ (Length::Px(_) | Length::Percent(_) | Length::Em(_))) => (l.resolve(main_total, s).unwrap_or(0.0), 0.0),
                _ => {
                    let avail_w = if row { main_total } else { cross.unwrap_or(cross_total - margin[1] - margin[3]) };
                    let i = self.intrinsic(child, avail_w.max(0.0));
                    (if row { i.x } else { i.y }, 0.0)
                }
            };
            slots.push(Slot { main, fill, cross, margin });
        }
        let margins_main: f32 = slots.iter().map(|sl| if row { sl.margin[1] + sl.margin[3] } else { sl.margin[0] + sl.margin[2] }).sum();
        let fixed: f32 = slots.iter().map(|sl| sl.main).sum();
        let gaps = gap * (slots.len() - 1) as f32;
        let leftover = (main_total - fixed - gaps - margins_main).max(0.0);
        let weights: f32 = slots.iter().map(|sl| sl.fill).sum();
        if weights > 0.0 {
            for sl in &mut slots {
                if sl.fill > 0.0 {
                    sl.main = leftover * sl.fill / weights;
                }
            }
        }
        let used = slots.iter().map(|sl| sl.main).sum::<f32>() + gaps + margins_main;
        let free_space = (main_total - used).max(0.0);
        let (mut cursor, between) = match (justify, weights > 0.0) {
            (_, true) | (Justify::Start, _) => (0.0, 0.0),
            (Justify::Center, _) => (free_space / 2.0, 0.0),
            (Justify::End, _) => (free_space, 0.0),
            (Justify::SpaceBetween, _) if slots.len() > 1 => (0.0, free_space / (slots.len() - 1) as f32),
            (Justify::SpaceBetween, _) => (0.0, 0.0),
        };
        for ((child, path), sl) in children.iter().zip(&slots) {
            let (m_before, m_after, c_before, c_after) =
                if row { (sl.margin[3], sl.margin[1], sl.margin[0], sl.margin[2]) } else { (sl.margin[0], sl.margin[2], sl.margin[3], sl.margin[1]) };
            cursor += m_before;
            let cross_space = (cross_total - c_before - c_after).max(0.0);
            let child_align = prop(child, "align-self").map_or(align, Align::parse);
            let cross = match (sl.cross, child_align) {
                (Some(c), _) => c,
                (None, Align::Stretch) => cross_space,
                (None, _) => {
                    let avail = if row { sl.main } else { cross_space };
                    let i = self.intrinsic(child, avail);
                    (if row { i.y } else { i.x }).min(cross_space)
                }
            };
            let cross_off = c_before
                + match child_align {
                    Align::Center => (cross_space - cross) / 2.0,
                    Align::End => cross_space - cross,
                    _ => 0.0,
                };
            let rect = if row {
                Rect::from_min_size(pos2(content.left() + cursor, content.top() + cross_off), vec2(sl.main.max(0.0), cross.max(0.0)))
            } else {
                Rect::from_min_size(pos2(content.left() + cross_off, content.top() + cursor), vec2(cross.max(0.0), sl.main.max(0.0)))
            };
            cursor += sl.main + m_after + gap + between;
            if child.tag == "xui" {
                self.include(child, path.clone(), rect, clip, depth);
            } else {
                self.place(child, path.clone(), included, rect, content, clip, false, Anchor::default(), depth);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lay(src: &str, w: f32, h: f32) -> Layout {
        let doc = Document::parse(src).unwrap();
        layout(&doc, Rect::from_min_size(Pos2::ZERO, vec2(w, h)), &mut ApproxMeasure, &Options::default())
    }

    #[test]
    fn anchors_pin_to_edges_and_stretch() {
        let l = lay(
            r#"<interface>
  <panel id="a" anchor="bottom-right" right="10" bottom="20" width="100" height="50" />
  <panel id="b" anchor="center" width="200" height="100" />
  <panel id="c" anchor="stretch-top" left="5" right="5" height="40" />
  <panel id="d" x="30" y="40" width="10" height="10" />
</interface>"#,
            800.0,
            600.0,
        );
        assert_eq!(l.get(&[0]).unwrap().rect, Rect::from_min_size(pos2(690.0, 530.0), vec2(100.0, 50.0)));
        assert_eq!(l.get(&[1]).unwrap().rect, Rect::from_min_size(pos2(300.0, 250.0), vec2(200.0, 100.0)));
        assert_eq!(l.get(&[2]).unwrap().rect, Rect::from_min_size(pos2(5.0, 0.0), vec2(790.0, 40.0)));
        assert_eq!(l.get(&[3]).unwrap().rect.min, pos2(30.0, 40.0));
        // Stretching follows the screen size.
        let l = lay(r#"<interface><panel anchor="stretch-top" left="5" right="5" height="40" /></interface>"#, 1000.0, 600.0);
        assert_eq!(l.get(&[0]).unwrap().rect.width(), 990.0);
    }

    #[test]
    fn rows_share_fill_and_respect_gap_padding() {
        let l = lay(
            r#"<interface>
  <row id="r" width="320" height="40" padding="10" gap="10">
    <button id="a" width="100" />
    <button id="b" width="fill" />
    <button id="c" width="2fr" />
  </row>
</interface>"#,
            800.0,
            600.0,
        );
        let a = l.get(&[0, 0]).unwrap().rect;
        let b = l.get(&[0, 1]).unwrap().rect;
        let c = l.get(&[0, 2]).unwrap().rect;
        assert_eq!(a.min, pos2(10.0, 10.0));
        assert_eq!(a.width(), 100.0);
        // 300 content - 100 - 20 gaps = 180 → 60 / 120
        assert!((b.width() - 60.0).abs() < 0.01 && (c.width() - 120.0).abs() < 0.01);
        assert_eq!(b.min.x, 120.0);
        assert_eq!(a.height(), 20.0);
    }

    #[test]
    fn columns_size_to_content_and_center() {
        let l = lay(
            r#"<interface>
  <panel id="menu" anchor="center" width="300" padding="0" gap="0" align="center" justify="center" height="400">
    <label id="t" text="Paused" height="30" width="100" />
    <button id="b" text="Resume" height="50" width="200" />
  </panel>
</interface>"#,
            800.0,
            600.0,
        );
        let menu = l.get(&[0]).unwrap().rect;
        let t = l.get(&[0, 0]).unwrap().rect;
        let b = l.get(&[0, 1]).unwrap().rect;
        assert_eq!(menu.center(), pos2(400.0, 300.0));
        assert_eq!(t.center().x, 400.0);
        assert_eq!(t.top(), menu.top() + (400.0 - 80.0) / 2.0);
        assert_eq!(b.top(), t.bottom());
        // A panel with no height fits its content.
        let l = lay(r#"<interface><panel padding="10" gap="5"><button height="20"/><button height="20"/></panel></interface>"#, 800.0, 600.0);
        assert_eq!(l.get(&[0]).unwrap().rect.height(), 65.0);
    }

    #[test]
    fn design_size_scales_to_screen() {
        let l = lay(r#"<interface width="1920" height="1080"><panel x="100" y="100" width="960" height="540" /></interface>"#, 960.0, 540.0);
        assert_eq!(l.get(&[0]).unwrap().rect, Rect::from_min_size(pos2(50.0, 50.0), vec2(480.0, 270.0)));
    }

    #[test]
    fn hit_testing_finds_the_topmost() {
        let l = lay(r#"<interface><panel width="200" height="200"><button id="b" height="50" /></panel></interface>"#, 800.0, 600.0);
        assert_eq!(l.hit(pos2(30.0, 30.0), true).unwrap().path, vec![0, 0]);
        assert_eq!(l.hit(pos2(150.0, 150.0), true).unwrap().path, vec![0]);
        assert!(l.hit(pos2(500.0, 500.0), true).is_none());
    }

    #[test]
    fn legacy_files_still_lay_out() {
        for src in [include_str!("../../../examples/hello/interfaces/player.xui"), include_str!("../../../examples/hello/interfaces/settings.xui")] {
            let l = lay(src, 1280.0, 720.0);
            assert!(l.items.len() > 10);
            let screen = l.get(&[0]).unwrap().rect;
            assert_eq!(screen, Rect::from_min_size(Pos2::ZERO, vec2(1280.0, 720.0)));
        }
    }
}
