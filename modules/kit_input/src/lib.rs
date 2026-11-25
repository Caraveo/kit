//! # kit_input
//!
//! Input state as a resource (Defold's `hid` + `input`). The windowing layer
//! (kit_engine / the editor) feeds raw events in; game code and scripts read
//! [`Input`] with friendly key names: `"A"`, `"1"`, `"Space"`, `"Left"`,
//! `"Shift"`, `"MouseLeft"` ... (case-insensitive).

use glam::Vec2;
use kit_core::{App, Module, Stage};
use std::collections::HashSet;

#[derive(Default, Debug)]
pub struct Input {
    down: HashSet<String>,
    pressed: HashSet<String>,
    released: HashSet<String>,
    /// Cursor position in window pixels (top-left origin).
    pub mouse: Vec2,
    /// Cursor movement since last frame.
    pub mouse_delta: Vec2,
    /// Scroll since last frame (lines).
    pub scroll: f32,
}

/// Canonical (lowercase) key name, so scripts can say `"a"`, `"A"` or `"KeyA"`,
/// `"Left"` or `"ArrowLeft"`, `"Shift"` or `"ShiftLeft"`.
pub fn normalize_key(name: &str) -> String {
    let n = name.trim();
    let n = n.strip_prefix("Key").filter(|r| r.len() == 1).unwrap_or(n);
    let n = n.strip_prefix("Digit").filter(|r| r.len() == 1).unwrap_or(n);
    let n = n.strip_prefix("Arrow").unwrap_or(n);
    let lower = n.to_ascii_lowercase();
    match lower.as_str() {
        "shiftleft" | "shiftright" => "shift".into(),
        "controlleft" | "controlright" | "control" => "ctrl".into(),
        "altleft" | "altright" => "alt".into(),
        "superleft" | "superright" | "meta" | "cmd" => "super".into(),
        "esc" => "escape".into(),
        "return" => "enter".into(),
        " " => "space".into(),
        _ => lower,
    }
}

impl Input {
    pub fn press(&mut self, key: &str) {
        let k = normalize_key(key);
        if self.down.insert(k.clone()) {
            self.pressed.insert(k);
        }
    }

    pub fn release(&mut self, key: &str) {
        let k = normalize_key(key);
        if self.down.remove(&k) {
            self.released.insert(k);
        }
    }

    /// Is the key held this frame?
    pub fn down(&self, key: &str) -> bool {
        self.down.contains(&normalize_key(key))
    }
    /// Did the key go down this frame?
    pub fn pressed(&self, key: &str) -> bool {
        self.pressed.contains(&normalize_key(key))
    }
    /// Did the key go up this frame?
    pub fn released(&self, key: &str) -> bool {
        self.released.contains(&normalize_key(key))
    }

    /// -1, 0 or +1 from a pair of keys, e.g. `axis("Left", "Right")`.
    pub fn axis(&self, negative: &str, positive: &str) -> f32 {
        (self.down(positive) as i32 - self.down(negative) as i32) as f32
    }

    /// Keys pressed this frame (canonical names).
    pub fn pressed_keys(&self) -> Vec<String> {
        self.pressed.iter().cloned().collect()
    }

    pub fn set_mouse(&mut self, pos: Vec2) {
        self.mouse_delta += pos - self.mouse;
        self.mouse = pos;
    }

    /// Release everything (window lost focus).
    pub fn clear(&mut self) {
        let held: Vec<String> = self.down.drain().collect();
        self.released.extend(held);
    }

    /// Clear per-frame edges. Called at the end of every frame.
    pub fn end_frame(&mut self) {
        self.pressed.clear();
        self.released.clear();
        self.mouse_delta = Vec2::ZERO;
        self.scroll = 0.0;
    }
}

pub struct InputModule;

impl Module for InputModule {
    fn name(&self) -> &'static str {
        "input"
    }
    fn build(&self, app: &mut App) {
        app.world.insert_resource(Input::default());
        app.add_system(Stage::Last, "input_end_frame", |w| w.resource_mut::<Input>().end_frame());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_names() {
        assert_eq!(normalize_key("KeyA"), normalize_key("a"));
        assert_eq!(normalize_key("Digit1"), "1");
        assert_eq!(normalize_key("ArrowLeft"), normalize_key("left"));
        assert_eq!(normalize_key("ShiftLeft"), normalize_key("Shift"));
        assert_eq!(normalize_key("Space"), "space");
        assert_eq!(normalize_key("MouseLeft"), "mouseleft");
    }

    #[test]
    fn edges() {
        let mut i = Input::default();
        i.press("KeyW");
        assert!(i.down("w") && i.pressed("W"));
        i.press("KeyW"); // key repeat is not a new press
        i.end_frame();
        assert!(i.down("W") && !i.pressed("W"));
        i.release("KeyW");
        assert!(!i.down("W") && i.released("W"));
        assert_eq!(i.axis("A", "W"), 0.0);
    }
}
