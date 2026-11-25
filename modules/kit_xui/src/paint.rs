//! Painting: a [`Layout`] becomes epaint shapes. The editor draws these with
//! egui's painter; games tessellate them ([`UiFonts::tessellate`]) and draw the
//! triangles with the renderer's 2D UI pass, so both look identical.

use crate::layout::{Item, Layout, Measure};
use crate::style::{EM, Length, State, border, color, length, prop, prop_in, shadow};
use epaint::text::{FontDefinitions, Fonts, FontsView, TextOptions};
use epaint::{
    ClippedPrimitive, ClippedShape, Color32, CornerRadius, FontId, Pos2, Rect, RectShape, Shadow, Shape, Stroke, StrokeKind,
    TessellationOptions, Tessellator, TextureId, Vec2, pos2, vec2,
};

impl Measure for FontsView<'_> {
    fn text(&mut self, text: &str, size: f32, wrap: Option<f32>) -> Vec2 {
        self.layout(text.to_owned(), FontId::proportional(size), Color32::WHITE, wrap.unwrap_or(f32::INFINITY)).size()
    }
}

/// Per-element interaction state for painting.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ElementState {
    pub hovered: bool,
    pub pressed: bool,
    pub focused: bool,
    pub classes: Vec<String>,
}

pub const DEFAULT_TEXT: Color32 = Color32::from_rgb(244, 242, 255);
pub const DEFAULT_PANEL: Color32 = Color32::from_rgb(43, 38, 72);
pub const DEFAULT_BUTTON: Color32 = Color32::from_rgb(114, 85, 190);

/// Images for `<image src="…">`: texture and pixel size.
pub type ImageLookup<'a> = dyn FnMut(&str) -> Option<(TextureId, Vec2)> + 'a;

/// Paint every laid-out element.
pub fn paint(layout: &Layout, fonts: &mut FontsView<'_>, state: &dyn Fn(&Item) -> ElementState, image: &mut ImageLookup<'_>) -> Vec<ClippedShape> {
    let mut out = Vec::new();
    for item in &layout.items {
        paint_item(item, fonts, &state(item), image, &mut out);
    }
    out
}

fn resolve_radius(node: &crate::Node, rect: Rect, scale: f32) -> f32 {
    let r = match length(node, "border-radius") {
        Some(Length::Percent(p)) => p / 100.0 * rect.width().min(rect.height()),
        Some(l) => l.resolve(rect.width(), scale).unwrap_or(0.0),
        None => match node.tag.as_str() {
            "button" | "panel" => 8.0 * scale,
            _ => 0.0,
        },
    };
    r.clamp(0.0, (rect.width().min(rect.height()) / 2.0).min(255.0))
}

pub fn paint_item(item: &Item, fonts: &mut FontsView<'_>, st: &ElementState, image: &mut ImageLookup<'_>, out: &mut Vec<ClippedShape>) {
    let node = &item.node;
    let rect = item.rect;
    if !rect.is_positive() || !item.clip.intersects(rect) {
        return;
    }
    let s = item.scale;
    let state = State { hovered: st.hovered, pressed: st.pressed, focused: st.focused, classes: &st.classes };
    let opacity = prop_in(node, "opacity", state).and_then(|v| v.trim().parse::<f32>().ok()).filter(|v| v.is_finite()).unwrap_or(1.0).clamp(0.0, 1.0);
    let fade = |c: Color32| c.gamma_multiply(opacity);
    let radius = resolve_radius(node, rect, s);
    let corner = CornerRadius::same(radius.round() as u8);
    let mut push = |shape: Shape| out.push(ClippedShape { clip_rect: item.clip, shape });

    // Shadow, fill, image, border, text.
    if let Some((dx, dy, blur, spread, c)) = prop_in(node, "box-shadow", state).and_then(|v| shadow(v, rect.width(), s)) {
        let sh = Shadow {
            offset: [dx.clamp(-127.0, 127.0) as i8, dy.clamp(-127.0, 127.0) as i8],
            blur: blur.clamp(0.0, 255.0) as u8,
            spread: spread.clamp(0.0, 255.0) as u8,
            color: fade(c),
        };
        push(Shape::Rect(sh.as_shape(rect, corner)));
    }
    let fill = prop_in(node, "background", state)
        .or_else(|| prop_in(node, "background-color", state))
        .and_then(color)
        .or(match node.tag.as_str() {
            "button" => Some(DEFAULT_BUTTON),
            "panel" => Some(DEFAULT_PANEL),
            _ => None,
        });
    if let Some(fill) = fill.filter(|c| c.a() > 0) {
        push(Shape::Rect(RectShape::new(rect, corner, fade(fill), Stroke::NONE, StrokeKind::Inside)));
    }
    if node.tag == "image" {
        let src = node.attr("src").or_else(|| node.attr("sprite")).filter(|v| !v.is_empty());
        match src.and_then(|src| image(src)) {
            Some((texture, size)) => {
                let fit = prop(node, "fit").unwrap_or("stretch");
                let (dst, uv) = fit_image(rect, size, fit);
                let tint = prop_in(node, "tint", state).and_then(color).unwrap_or(Color32::WHITE);
                push(Shape::image(texture, dst, uv, fade(tint)));
            }
            None => {
                push(Shape::Rect(RectShape::new(rect, corner, Color32::from_rgba_unmultiplied(255, 255, 255, 14), Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 60)), StrokeKind::Inside)));
            }
        }
    }
    if let Some((w, c)) = prop_in(node, "border", state).and_then(|v| border(v, rect.width(), s)) {
        push(Shape::Rect(RectShape::new(rect, corner, Color32::TRANSPARENT, Stroke::new(w, fade(c)), StrokeKind::Inside)));
    }
    if let Some((w, c)) = prop_in(node, "border-bottom", state).and_then(|v| border(v, rect.width(), s)) {
        push(Shape::line_segment([pos2(rect.left(), rect.bottom() - w / 2.0), pos2(rect.right(), rect.bottom() - w / 2.0)], Stroke::new(w, fade(c))));
    }
    if matches!(node.tag.as_str(), "label" | "button") {
        let text = node.text().unwrap_or("");
        if !text.is_empty() {
            let size = length(node, "font-size").and_then(|l| l.resolve(EM, s)).unwrap_or(18.0 * s).clamp(1.0, 512.0);
            let col = prop_in(node, "text-color", state)
                .or_else(|| prop_in(node, "color", state))
                .and_then(color)
            .unwrap_or(DEFAULT_TEXT);
            let pad = crate::style::edges(prop(node, "padding").unwrap_or(if node.tag == "button" { "0.5em 1em" } else { "0" }), rect.width(), s);
            let inner = Rect::from_min_max(pos2(rect.left() + pad[3], rect.top() + pad[0]), pos2(rect.right() - pad[1], rect.bottom() - pad[2]));
            let galley = fonts.layout(text.to_owned(), FontId::proportional(size), fade(col), inner.width().max(1.0));
            let align = prop(node, "text-align").unwrap_or(if node.tag == "button" { "center" } else { "left" });
            let x = match align {
                "center" => inner.center().x - galley.size().x / 2.0,
                "right" | "end" => inner.right() - galley.size().x,
                _ => inner.left(),
            };
            let y = inner.center().y - galley.size().y / 2.0;
            let clip = item.clip.intersect(rect.expand(1.0));
            out.push(ClippedShape { clip_rect: clip, shape: Shape::galley(pos2(x, y), galley, fade(col)) });
        }
    }
}

fn fit_image(rect: Rect, size: Vec2, fit: &str) -> (Rect, Rect) {
    let full = Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0));
    if size.x <= 0.0 || size.y <= 0.0 {
        return (rect, full);
    }
    let aspect = size.x / size.y;
    let box_aspect = rect.width() / rect.height().max(1e-3);
    match fit {
        "contain" => {
            let s = if aspect > box_aspect { vec2(rect.width(), rect.width() / aspect) } else { vec2(rect.height() * aspect, rect.height()) };
            (Rect::from_center_size(rect.center(), s), full)
        }
        "cover" => {
            // Crop the texture instead of the rect.
            if aspect > box_aspect {
                let w = box_aspect / aspect;
                (rect, Rect::from_min_max(pos2((1.0 - w) / 2.0, 0.0), pos2((1.0 + w) / 2.0, 1.0)))
            } else {
                let h = aspect / box_aspect;
                (rect, Rect::from_min_max(pos2(0.0, (1.0 - h) / 2.0), pos2(1.0, (1.0 + h) / 2.0)))
            }
        }
        _ => (rect, full),
    }
}

/// Fonts and tessellation for drawing interfaces in a game.
pub struct UiFonts {
    pub fonts: Fonts,
}

impl Default for UiFonts {
    fn default() -> Self {
        Self::new()
    }
}

impl UiFonts {
    pub fn new() -> Self {
        Self { fonts: Fonts::new(TextOptions::default(), FontDefinitions::default()) }
    }

    pub fn view(&mut self, pixels_per_point: f32) -> FontsView<'_> {
        self.fonts.with_pixels_per_point(pixels_per_point)
    }

    /// Turn shapes into triangles. The font atlas is `TextureId::Managed(0)`;
    /// changes to it are in [`Self::atlas_delta`].
    pub fn tessellate(&mut self, shapes: Vec<ClippedShape>, pixels_per_point: f32) -> Vec<ClippedPrimitive> {
        let atlas = self.fonts.texture_atlas();
        let mut t = Tessellator::new(pixels_per_point, TessellationOptions::default(), atlas.size(), atlas.prepared_discs());
        t.tessellate_shapes(shapes)
    }

    /// New font-atlas pixels since the last call (upload to texture 0).
    pub fn atlas_delta(&mut self) -> Option<epaint::ImageDelta> {
        self.fonts.font_image_delta()
    }

    /// Call once per frame before laying out.
    pub fn begin_frame(&mut self) {
        self.fonts.begin_pass(TextOptions::default());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{Options, layout};

    #[test]
    fn paints_and_tessellates_the_example_interfaces() {
        let mut fonts = UiFonts::new();
        for src in [include_str!("../../../examples/hello/interfaces/player.xui"), include_str!("../../../examples/hello/interfaces/settings.xui")] {
            fonts.begin_frame();
            let doc = crate::Document::parse(src).unwrap();
            let screen = Rect::from_min_size(Pos2::ZERO, vec2(1280.0, 720.0));
            let mut view = fonts.view(1.0);
            let l = layout(&doc, screen, &mut view, &Options::default());
            let shapes = paint(&l, &mut view, &|_| ElementState::default(), &mut |_| Some((TextureId::User(1), vec2(64.0, 64.0))));
            assert!(shapes.len() > 20);
            let prims = fonts.tessellate(shapes, 1.0);
            assert!(!prims.is_empty());
        }
        assert!(fonts.atlas_delta().is_some());
    }

    #[test]
    fn hover_state_changes_the_fill() {
        let doc = crate::Document::parse(r##"<interface><button width="100" height="40" background="#111111" hover-background="#222222" /></interface>"##).unwrap();
        let mut fonts = UiFonts::new();
        let mut view = fonts.view(1.0);
        let l = layout(&doc, Rect::from_min_size(Pos2::ZERO, vec2(400.0, 300.0)), &mut view, &Options::default());
        let fill = |hovered: bool, view: &mut FontsView<'_>| {
            let shapes = paint(&l, view, &|_| ElementState { hovered, ..Default::default() }, &mut |_| None);
            shapes.iter().find_map(|s| if let Shape::Rect(r) = &s.shape { Some(r.fill) } else { None }).unwrap()
        };
        assert_eq!(fill(false, &mut view), Color32::from_rgb(0x11, 0x11, 0x11));
        assert_eq!(fill(true, &mut view), Color32::from_rgb(0x22, 0x22, 0x22));
    }
}
