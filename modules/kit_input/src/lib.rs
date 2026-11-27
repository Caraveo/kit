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
    /// Size of the game's view in pixels (set by the editor or player window).
    pub screen: Vec2,
    /// Top-left of the game's view inside the window, in pixels (the editor's
    /// viewport); mouse and touch positions are relative to it.
    pub view_origin: Vec2,
    /// A script asked to capture the mouse (hidden, unlimited movement).
    pub mouse_locked: bool,
    /// A script asked to hide the pointer.
    pub mouse_hidden: bool,
    /// Gamepad sticks and triggers: `"leftx"`, `"lefty"`, `"rightx"`,
    /// `"righty"` (-1..1, up is +1), `"lefttrigger"`, `"righttrigger"` (0..1).
    analog: std::collections::HashMap<String, f32>,
    /// Fingers on a touch screen: (finger id, position in pixels).
    pub touches: Vec<(u64, Vec2)>,
    /// Named actions → the keys / buttons that trigger them (`Input.Bind`).
    bindings: std::collections::HashMap<String, Vec<String>>,
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

    /// The keys a name stands for: an action's bound keys, or the key itself.
    fn keys_for(&self, key: &str) -> Vec<String> {
        let k = normalize_key(key);
        match self.bindings.get(&k) {
            Some(keys) => keys.clone(),
            None => vec![k],
        }
    }

    /// Is the key (or any key bound to this action) held this frame?
    pub fn down(&self, key: &str) -> bool {
        self.keys_for(key).iter().any(|k| self.down.contains(k))
    }
    /// Did the key (or action) go down this frame?
    pub fn pressed(&self, key: &str) -> bool {
        self.keys_for(key).iter().any(|k| self.pressed.contains(k))
    }
    /// Did the key (or action) go up this frame?
    pub fn released(&self, key: &str) -> bool {
        self.keys_for(key).iter().any(|k| self.released.contains(k))
    }

    /// Give an action a set of keys/buttons (replacing what it had):
    /// `bind("Jump", ["Space", "PadA"])`. Actions work anywhere a key name does.
    pub fn bind(&mut self, action: &str, keys: &[String]) {
        self.bindings.insert(normalize_key(action), keys.iter().map(|k| normalize_key(k)).collect());
    }
    /// Forget an action's keys.
    pub fn unbind(&mut self, action: &str) {
        self.bindings.remove(&normalize_key(action));
    }
    /// The keys bound to an action (empty when it isn't bound).
    pub fn bound(&self, action: &str) -> Vec<String> {
        self.bindings.get(&normalize_key(action)).cloned().unwrap_or_default()
    }

    /// A gamepad stick axis or trigger (see [`Input::analog`] names).
    pub fn analog(&self, name: &str) -> f32 {
        self.analog.get(&normalize_key(name)).copied().unwrap_or(0.0)
    }
    pub fn set_analog(&mut self, name: &str, value: f32) {
        self.analog.insert(normalize_key(name), value);
    }

    /// -1, 0 or +1 from a pair of keys, e.g. `axis("Left", "Right")`.
    pub fn axis(&self, negative: &str, positive: &str) -> f32 {
        (self.down(positive) as i32 - self.down(negative) as i32) as f32
    }

    /// The last key, mouse or gamepad button pressed this frame, for "press a
    /// key to rebind" screens.
    pub fn any_pressed(&self) -> Option<String> {
        self.pressed.iter().next().cloned()
    }

    /// Keys pressed this frame (canonical names).
    pub fn pressed_keys(&self) -> Vec<String> {
        self.pressed.iter().cloned().collect()
    }

    /// The pointer moved to `pos` (window pixels).
    pub fn set_mouse(&mut self, pos: Vec2) {
        let pos = pos - self.view_origin;
        // While locked, movement comes from `add_mouse_motion` (raw device motion).
        if !self.mouse_locked {
            self.mouse_delta += pos - self.mouse;
        }
        self.mouse = pos;
    }

    /// Raw mouse movement (used while the mouse is locked).
    pub fn add_mouse_motion(&mut self, delta: Vec2) {
        if self.mouse_locked {
            self.mouse_delta += delta;
        }
    }

    /// A finger touched, moved or lifted (window pixels).
    pub fn touch(&mut self, id: u64, pos: Vec2, down: bool) {
        let pos = pos - self.view_origin;
        self.touches.retain(|(t, _)| *t != id);
        if down {
            self.touches.push((id, pos));
        }
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
