//! Properties and value parsing. A property can be written as an attribute
//! (`width="50%"`) or in `style` (`style="width: 50%"`); the attribute wins.
//! State variants are prefixed: `hover-background`, `pressed-color`,
//! `focus-border`, or `<class>-<property>` for classes added at runtime.

use crate::doc::Node;
use epaint::Color32;

/// Look up a property (attribute first, then `style`).
pub fn prop<'a>(node: &'a Node, name: &str) -> Option<&'a str> {
    node.attr(name).or_else(|| {
        node.attr("style").and_then(|style| {
            style.split(';').find_map(|declaration| {
                let (property, value) = declaration.split_once(':')?;
                property.trim().eq_ignore_ascii_case(name).then_some(value.trim())
            })
        })
    })
}

/// Interaction state used to pick state variants.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct State<'a> {
    pub hovered: bool,
    pub pressed: bool,
    pub focused: bool,
    pub classes: &'a [String],
}

/// A property with state variants applied (`pressed-` > `hover-` > `focus-` > classes > base).
pub fn prop_in<'a>(node: &'a Node, name: &str, state: State<'_>) -> Option<&'a str> {
    let variant = |prefix: &str| prop(node, &format!("{prefix}-{name}"));
    if state.pressed {
        if let Some(v) = variant("pressed") {
            return Some(v);
        }
    }
    if state.hovered {
        if let Some(v) = variant("hover") {
            return Some(v);
        }
    }
    if state.focused {
        if let Some(v) = variant("focus") {
            return Some(v);
        }
    }
    for class in state.classes.iter().map(String::as_str).chain(node.classes()) {
        if let Some(v) = variant(class) {
            return Some(v);
        }
    }
    prop(node, name)
}

/// A length.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Length {
    Px(f32),
    Percent(f32),
    Em(f32),
    /// Share of the leftover space in a row/column (`fill`, `2fr`).
    Fill(f32),
    Auto,
}

impl Length {
    pub fn parse(value: &str) -> Option<Self> {
        let v = value.trim();
        if v.is_empty() {
            return None;
        }
        if v.eq_ignore_ascii_case("auto") {
            return Some(Self::Auto);
        }
        if v.eq_ignore_ascii_case("fill") {
            return Some(Self::Fill(1.0));
        }
        let num = |s: &str| s.trim().parse::<f32>().ok().filter(|n| n.is_finite());
        if let Some(n) = v.strip_suffix("fr") {
            return num(n).map(|n| Self::Fill(n.max(0.0)));
        }
        if let Some(n) = v.strip_suffix('%') {
            return num(n).map(Self::Percent);
        }
        if let Some(n) = v.strip_suffix("rem").or_else(|| v.strip_suffix("em")) {
            return num(n).map(Self::Em);
        }
        num(v.strip_suffix("px").unwrap_or(v)).map(Self::Px)
    }

    /// Resolve to pixels. `Fill`/`Auto` return `None`.
    pub fn resolve(self, parent: f32, scale: f32) -> Option<f32> {
        match self {
            Self::Px(n) => Some(n * scale),
            Self::Percent(p) => Some(p * parent / 100.0),
            Self::Em(e) => Some(e * EM * scale),
            Self::Fill(_) | Self::Auto => None,
        }
    }
}

/// Pixels per `em`.
pub const EM: f32 = 16.0;

pub fn length(node: &Node, name: &str) -> Option<Length> {
    prop(node, name).and_then(Length::parse)
}

/// `padding: 1em` / `1em 2em` / `1px 2px 3px 4px` → (top, right, bottom, left).
pub fn edges(value: &str, parent: f32, scale: f32) -> [f32; 4] {
    let parts: Vec<f32> = value
        .split_whitespace()
        .filter_map(|p| Length::parse(p).and_then(|l| l.resolve(parent, scale)))
        .collect();
    match parts.as_slice() {
        [a] => [*a; 4],
        [v, h] => [*v, *h, *v, *h],
        [t, h, b] => [*t, *h, *b, *h],
        [t, r, b, l, ..] => [*t, *r, *b, *l],
        _ => [0.0; 4],
    }
}

/// Parse a color: `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`, `rgb(…)`, `rgba(…)`, or a few names.
pub fn color(value: &str) -> Option<Color32> {
    let v = value.trim();
    if let Some(hex) = v.strip_prefix('#') {
        if !hex.is_ascii() {
            return None;
        }
        let digit = |s: &str| u8::from_str_radix(s, 16).ok();
        let short = |c: &str| digit(c).map(|d| d * 17);
        return match hex.len() {
            3 => Some(Color32::from_rgb(short(&hex[0..1])?, short(&hex[1..2])?, short(&hex[2..3])?)),
            4 => Some(Color32::from_rgba_unmultiplied(short(&hex[0..1])?, short(&hex[1..2])?, short(&hex[2..3])?, short(&hex[3..4])?)),
            6 => Some(Color32::from_rgb(digit(&hex[0..2])?, digit(&hex[2..4])?, digit(&hex[4..6])?)),
            8 => Some(Color32::from_rgba_unmultiplied(digit(&hex[0..2])?, digit(&hex[2..4])?, digit(&hex[4..6])?, digit(&hex[6..8])?)),
            _ => None,
        };
    }
    let lower = v.to_ascii_lowercase();
    if let Some(args) = lower.strip_prefix("rgba(").or_else(|| lower.strip_prefix("rgb(")).and_then(|s| s.strip_suffix(')')) {
        let parts: Vec<&str> = args.split(',').map(str::trim).collect();
        let channel = |s: &str| s.parse::<f32>().ok().map(|n| n.clamp(0.0, 255.0) as u8);
        let (r, g, b) = (channel(parts.first()?)?, channel(parts.get(1)?)?, channel(parts.get(2)?)?);
        let a = parts.get(3).and_then(|a| a.parse::<f32>().ok()).map_or(255, |a| (a.clamp(0.0, 1.0) * 255.0).round() as u8);
        return Some(Color32::from_rgba_unmultiplied(r, g, b, a));
    }
    Some(match lower.as_str() {
        "transparent" | "none" => Color32::TRANSPARENT,
        "white" => Color32::WHITE,
        "black" => Color32::BLACK,
        "red" => Color32::from_rgb(235, 64, 52),
        "green" => Color32::from_rgb(52, 199, 89),
        "blue" => Color32::from_rgb(10, 132, 255),
        "yellow" => Color32::from_rgb(255, 214, 10),
        "orange" => Color32::from_rgb(255, 149, 0),
        "purple" => Color32::from_rgb(175, 82, 222),
        "gray" | "grey" => Color32::from_rgb(142, 142, 147),
        _ => return None,
    })
}

pub fn to_hex(c: Color32) -> String {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    if a == 255 { format!("#{r:02x}{g:02x}{b:02x}") } else { format!("#{r:02x}{g:02x}{b:02x}{a:02x}") }
}

/// `1px solid #fff` → (width, color).
pub fn border(value: &str, parent: f32, scale: f32) -> Option<(f32, Color32)> {
    let mut width = None;
    let mut col = None;
    for part in value.split_whitespace() {
        if let Some(c) = color(part).filter(|_| part.starts_with('#') || part.starts_with("rgb") || part.chars().all(char::is_alphabetic) && part != "solid") {
            col = Some(c);
        } else if let Some(l) = Length::parse(part).and_then(|l| l.resolve(parent, scale)) {
            width = Some(l);
        }
    }
    if value.trim().eq_ignore_ascii_case("none") {
        return None;
    }
    Some((width.unwrap_or(1.0), col?))
}

/// `0 10px 24px #00000088` → (offset x, offset y, blur, spread, color).
pub fn shadow(value: &str, parent: f32, scale: f32) -> Option<(f32, f32, f32, f32, Color32)> {
    let mut nums = Vec::new();
    let mut col = None;
    for part in value.split_whitespace() {
        if let Some(c) = color(part).filter(|_| part.starts_with('#') || part.starts_with("rgb")) {
            col = Some(c);
        } else if let Some(l) = Length::parse(part).and_then(|l| l.resolve(parent, scale)) {
            nums.push(l);
        }
    }
    let n = |i: usize| nums.get(i).copied().unwrap_or(0.0);
    Some((n(0), n(1), n(2), n(3), col?))
}

/// Horizontal anchor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum HAnchor {
    #[default]
    Left,
    Center,
    Right,
    Stretch,
}

/// Vertical anchor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum VAnchor {
    #[default]
    Top,
    Middle,
    Bottom,
    Stretch,
}

/// Where a free (absolutely positioned) element is pinned in its parent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Anchor {
    pub h: HAnchor,
    pub v: VAnchor,
}

impl Anchor {
    pub const PRESETS: [(&'static str, &'static str); 16] = [
        ("top-left", "Top left"),
        ("top", "Top"),
        ("top-right", "Top right"),
        ("left", "Left"),
        ("center", "Center"),
        ("right", "Right"),
        ("bottom-left", "Bottom left"),
        ("bottom", "Bottom"),
        ("bottom-right", "Bottom right"),
        ("stretch-top", "Stretch along the top"),
        ("stretch-middle", "Stretch across the middle"),
        ("stretch-bottom", "Stretch along the bottom"),
        ("stretch-left", "Stretch down the left"),
        ("stretch-center", "Stretch down the center"),
        ("stretch-right", "Stretch down the right"),
        ("stretch", "Stretch to fill"),
    ];

    pub fn parse(value: &str) -> Self {
        let v = value.trim().to_ascii_lowercase();
        let (h, vv) = match v.as_str() {
            "stretch" | "fill" => (HAnchor::Stretch, VAnchor::Stretch),
            "stretch-top" => (HAnchor::Stretch, VAnchor::Top),
            "stretch-middle" => (HAnchor::Stretch, VAnchor::Middle),
            "stretch-bottom" => (HAnchor::Stretch, VAnchor::Bottom),
            "stretch-left" => (HAnchor::Left, VAnchor::Stretch),
            "stretch-center" => (HAnchor::Center, VAnchor::Stretch),
            "stretch-right" => (HAnchor::Right, VAnchor::Stretch),
            _ => {
                let mut h = None;
                let mut vv = None;
                for token in v.split(['-', ' ']) {
                    match token {
                        "left" => h = Some(HAnchor::Left),
                        "right" => h = Some(HAnchor::Right),
                        "top" => vv = Some(VAnchor::Top),
                        "bottom" => vv = Some(VAnchor::Bottom),
                        "center" | "middle" => {
                            if h.is_none() {
                                h = Some(HAnchor::Center)
                            }
                            if vv.is_none() {
                                vv = Some(VAnchor::Middle)
                            }
                        }
                        _ => {}
                    }
                }
                // "top" alone centers horizontally; "left" alone centers vertically.
                let h = h.unwrap_or(if vv.is_some() && v != "top-left" { HAnchor::Center } else { HAnchor::Left });
                let vv = vv.unwrap_or(if h != HAnchor::Left || v == "left" { VAnchor::Middle } else { VAnchor::Top });
                (h, vv)
            }
        };
        Self { h, v: vv }
    }

    pub fn name(self) -> &'static str {
        use HAnchor as H;
        use VAnchor as V;
        match (self.h, self.v) {
            (H::Left, V::Top) => "top-left",
            (H::Center, V::Top) => "top",
            (H::Right, V::Top) => "top-right",
            (H::Left, V::Middle) => "left",
            (H::Center, V::Middle) => "center",
            (H::Right, V::Middle) => "right",
            (H::Left, V::Bottom) => "bottom-left",
            (H::Center, V::Bottom) => "bottom",
            (H::Right, V::Bottom) => "bottom-right",
            (H::Stretch, V::Top) => "stretch-top",
            (H::Stretch, V::Middle) => "stretch-middle",
            (H::Stretch, V::Bottom) => "stretch-bottom",
            (H::Left, V::Stretch) => "stretch-left",
            (H::Center, V::Stretch) => "stretch-center",
            (H::Right, V::Stretch) => "stretch-right",
            (H::Stretch, V::Stretch) => "stretch",
        }
    }
}

/// Cross-axis alignment of children in a row/column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Start,
    Center,
    End,
    Stretch,
}

impl Align {
    pub fn parse(v: &str) -> Self {
        match v.trim() {
            "start" | "flex-start" | "left" | "top" => Self::Start,
            "center" | "middle" => Self::Center,
            "end" | "flex-end" | "right" | "bottom" => Self::End,
            _ => Self::Stretch,
        }
    }
}

/// Main-axis distribution of children in a row/column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Justify {
    Start,
    Center,
    End,
    SpaceBetween,
}

impl Justify {
    pub fn parse(v: &str) -> Self {
        match v.trim() {
            "center" | "middle" => Self::Center,
            "end" | "flex-end" | "right" | "bottom" => Self::End,
            "space-between" | "between" => Self::SpaceBetween,
            _ => Self::Start,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lengths_and_colors() {
        assert_eq!(Length::parse("50%"), Some(Length::Percent(50.0)));
        assert_eq!(Length::parse("2em").unwrap().resolve(0.0, 1.0), Some(32.0));
        assert_eq!(Length::parse("3.5"), Some(Length::Px(3.5)));
        assert_eq!(Length::parse("2fr"), Some(Length::Fill(2.0)));
        assert_eq!(color("#fff"), Some(Color32::WHITE));
        assert_eq!(color("#00000080").unwrap().a(), 128);
        assert_eq!(color("rgba(255, 0, 0, 0.5)").unwrap().a(), 128);
        assert_eq!(edges("1px 2px", 0.0, 1.0), [1.0, 2.0, 1.0, 2.0]);
        assert_eq!(border("1px solid #ffffff22", 0.0, 1.0).unwrap().0, 1.0);
        assert_eq!(shadow("0 10px 24px #00000088", 0.0, 1.0).unwrap().2, 24.0);
    }

    #[test]
    fn anchors_round_trip() {
        for (name, _) in Anchor::PRESETS {
            assert_eq!(Anchor::parse(name).name(), name, "{name}");
        }
    }

    #[test]
    fn style_and_state_variants() {
        let doc = crate::Document::parse(r##"<interface><button style="background: #111; hover-background: #222" pressed-background="#333" /></interface>"##).unwrap();
        let b = &doc.root.children[0];
        assert_eq!(prop_in(b, "background", State::default()), Some("#111"));
        assert_eq!(prop_in(b, "background", State { hovered: true, ..Default::default() }), Some("#222"));
        assert_eq!(prop_in(b, "background", State { hovered: true, pressed: true, ..Default::default() }), Some("#333"));
    }
}
