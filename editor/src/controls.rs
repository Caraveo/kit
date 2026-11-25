//! User-local editor controls. These bindings never change a game's input.

use egui::{InputState, Key, Modifiers, PointerButton};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

macro_rules! shortcuts {
    ($( $action:ident: $label:literal => $(($key:ident, $mods:expr)),+; )+) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum Shortcut { $($action),+ }

        impl Shortcut {
            pub const ALL: &'static [Self] = &[$(Self::$action),+];

            pub fn label(self) -> &'static str {
                match self { $(Self::$action => $label),+ }
            }

            fn defaults(self) -> Vec<Binding> {
                match self { $(Self::$action => vec![$(Binding::new(Key::$key, $mods)),+]),+ }
            }
        }
    };
}

use Modifiers as M;
shortcuts! {
    Save: "Save scene or script" => (S, M::COMMAND);
    Undo: "Undo" => (Z, M::COMMAND);
    Redo: "Redo" => (Z, M::COMMAND.plus(M::SHIFT)), (Y, M::COMMAND);
    Play: "Play / stop" => (F5, M::NONE);
    Help: "Show shortcuts" => (F1, M::NONE);
    Release: "Release game input / exit Canvas" => (Escape, M::NONE);
    Frame: "Frame selection" => (F, M::NONE);
    Wireframe: "Toggle wireframe" => (Num1, M::NONE);
    Lighting: "Toggle lighting" => (Num2, M::NONE);
    Grid: "Toggle grid" => (Num3, M::NONE);
    Colliders: "Toggle collider outlines" => (Num4, M::NONE);
    Canvas: "Toggle Canvas" => (Num5, M::NONE);
    Volumes: "Toggle post-processing preview" => (Num6, M::NONE);
    Diagram: "Toggle Diagram" => (Num7, M::NONE);
    Rex: "Toggle Rex (AI puppy)" => (Num8, M::NONE);
    Rig: "Toggle Rig mode" => (Num9, M::NONE);
    Rotate: "Toggle auto-rotate" => (R, M::NONE);
    Dimension: "Switch 2D / 3D" => (T, M::NONE);
    Console: "Toggle Console" => (Backtick, M::NONE);
    Delete: "Delete selection" => (Delete, M::NONE), (Backspace, M::NONE);
    Duplicate: "Duplicate selection" => (D, M::COMMAND);
    Rename: "Rename selected object" => (F2, M::NONE);
    Left: "Camera left" => (A, M::NONE);
    Right: "Camera right" => (D, M::NONE);
    Down: "Camera down" => (Q, M::NONE);
    Up: "Camera up" => (E, M::NONE);
    Forward: "Camera forward / zoom in" => (W, M::NONE);
    Back: "Camera back / zoom out" => (S, M::NONE);
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Binding {
    key: String,
    #[serde(default)]
    shift: bool,
    #[serde(default)]
    alt: bool,
    #[serde(default)]
    command: bool,
    #[serde(default)]
    ctrl: bool,
}

impl Binding {
    pub fn new(key: Key, modifiers: Modifiers) -> Self {
        let command = modifiers.command || modifiers.mac_cmd || (modifiers.ctrl && !cfg!(target_os = "macos"));
        Self {
            key: key.name().into(),
            shift: modifiers.shift,
            alt: modifiers.alt,
            command,
            ctrl: modifiers.ctrl && cfg!(target_os = "macos"),
        }
    }

    pub fn label(&self) -> String {
        let mut parts = Vec::new();
        if self.command {
            parts.push(if cfg!(target_os = "macos") { "Cmd" } else { "Ctrl" });
        }
        if self.ctrl {
            parts.push("Ctrl");
        }
        if self.alt {
            parts.push("Alt");
        }
        if self.shift {
            parts.push("Shift");
        }
        parts.push(&self.key);
        parts.join("+")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Button {
    Left,
    Middle,
    Right,
}

impl Button {
    pub const ALL: [Self; 3] = [Self::Left, Self::Middle, Self::Right];
    pub fn label(self) -> &'static str {
        match self {
            Self::Left => "Left",
            Self::Middle => "Middle",
            Self::Right => "Right",
        }
    }
    pub fn egui(self) -> PointerButton {
        match self {
            Self::Left => PointerButton::Primary,
            Self::Middle => PointerButton::Middle,
            Self::Right => PointerButton::Secondary,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Modifier {
    None,
    Shift,
    Command,
    Alt,
}

impl Modifier {
    const ALL: [Self; 4] = [Self::None, Self::Shift, Self::Command, Self::Alt];
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "Off",
            Self::Shift => "Shift",
            Self::Command => {
                if cfg!(target_os = "macos") {
                    "Cmd"
                } else {
                    "Ctrl"
                }
            }
            Self::Alt => "Alt",
        }
    }
    pub fn held(self, state: Modifiers) -> bool {
        match self {
            Self::None => false,
            Self::Shift => state.shift,
            Self::Command => state.command,
            Self::Alt => state.alt,
        }
    }

    fn remove(self, state: &mut Modifiers) {
        match self {
            Self::None => {}
            Self::Shift => state.shift = false,
            Self::Alt => state.alt = false,
            Self::Command => {
                state.command = false;
                state.mac_cmd = false;
                if !cfg!(target_os = "macos") {
                    state.ctrl = false;
                }
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Gestures {
    pub select: Button,
    pub pan: Button,
    pub alternate_pan: Button,
    pub orbit: Button,
    pub alternate_orbit: Modifier,
    pub fast_camera: Modifier,
    pub canvas_draw: Button,
    pub canvas_restart: Modifier,
    pub sculpt: Button,
    pub reverse_sculpt: Modifier,
    pub gizmo: Button,
    pub snap_gizmo: Modifier,
    pub diagram_multiselect: Modifier,
    pub diagram_picker: Button,
    pub diagram_pan_left: bool,
    pub diagram_pan_middle: bool,
    pub diagram_pan_right: bool,
    pub close_script_tab: Button,
    pub scroll_zoom: bool,
    pub invert_scroll: bool,
}

impl Default for Gestures {
    fn default() -> Self {
        Self {
            select: Button::Left,
            pan: Button::Left,
            alternate_pan: Button::Middle,
            orbit: Button::Right,
            alternate_orbit: Modifier::Shift,
            fast_camera: Modifier::Shift,
            canvas_draw: Button::Left,
            canvas_restart: Modifier::Shift,
            sculpt: Button::Left,
            reverse_sculpt: Modifier::Command,
            gizmo: Button::Left,
            snap_gizmo: Modifier::Command,
            diagram_multiselect: Modifier::Shift,
            diagram_picker: Button::Right,
            diagram_pan_left: true,
            diagram_pan_middle: true,
            diagram_pan_right: true,
            close_script_tab: Button::Middle,
            scroll_zoom: true,
            invert_scroll: false,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
struct Bindings {
    primary: Option<Binding>,
    alternate: Option<Binding>,
}

impl Bindings {
    fn values(&self) -> impl Iterator<Item = &Binding> {
        self.primary.iter().chain(self.alternate.iter())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Controls {
    bindings: BTreeMap<Shortcut, Bindings>,
    pub gestures: Gestures,
}

impl Controls {
    pub fn load() -> Self {
        crate::config_dir()
            .and_then(|dir| std::fs::read_to_string(dir.join("controls.toml")).ok())
            .and_then(|text| toml::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let dir = crate::config_dir().ok_or_else(|| anyhow::anyhow!("No editor settings folder is available"))?;
        std::fs::create_dir_all(&dir)?;
        let mut temporary = tempfile::NamedTempFile::new_in(&dir)?;
        use std::io::Write as _;
        temporary.write_all(toml::to_string_pretty(self)?.as_bytes())?;
        temporary.persist(dir.join("controls.toml"))?;
        Ok(())
    }

    fn bindings(&self, action: Shortcut) -> Bindings {
        self.bindings.get(&action).cloned().unwrap_or_else(|| {
            let mut values = action.defaults().into_iter();
            Bindings { primary: values.next(), alternate: values.next() }
        })
    }

    pub fn label(&self, action: Shortcut) -> String {
        let keys: Vec<_> = self.bindings(action).values().map(Binding::label).collect();
        if keys.is_empty() { "Unbound".into() } else { keys.join(" / ") }
    }

    fn binding(&self, action: Shortcut, slot: usize) -> Option<Binding> {
        let bindings = self.bindings(action);
        if slot == 0 { bindings.primary } else { bindings.alternate }
    }

    pub fn set(&mut self, action: Shortcut, slot: usize, binding: Option<Binding>) -> anyhow::Result<()> {
        if let Some(binding) = &binding {
            if let Some(other) = Shortcut::ALL
                .iter()
                .copied()
                .find(|&other| other != action && self.bindings(other).values().any(|existing| existing == binding))
            {
                anyhow::bail!(
                    "{} is already assigned to {}. Clear that binding first.",
                    binding.label(),
                    other.label()
                );
            }
        }
        let mut values = self.bindings(action);
        if slot == 0 {
            values.primary = binding;
        } else {
            values.alternate = binding;
        }
        self.bindings.insert(action, values);
        Ok(())
    }

    pub fn take(&self, input: &mut InputState, action: Shortcut) -> bool {
        let bindings = self.bindings(action);
        let mut pressed = false;
        input.events.retain(|event| {
            if let egui::Event::Key { key, modifiers, pressed: true, repeat, .. } = event {
                if (!repeat || matches!(action, Shortcut::Undo | Shortcut::Redo))
                    && bindings.values().any(|binding| *binding == Binding::new(*key, *modifiers))
                {
                    pressed = true;
                    return false;
                }
            }
            true
        });
        pressed
    }

    pub fn held(&self, input: &InputState, action: Shortcut) -> f32 {
        let mut modifiers = input.modifiers;
        self.gestures.fast_camera.remove(&mut modifiers);
        self.bindings(action).values().any(|binding| {
            Key::from_name(&binding.key).is_some_and(|key| {
                input.key_down(key)
                    && (*binding == Binding::new(key, input.modifiers) || *binding == Binding::new(key, modifiers))
            })
        }) as u8 as f32
    }
}

#[derive(Default)]
pub struct BindingEditor {
    pub capture: Option<(Shortcut, usize)>,
    error: Option<String>,
}

impl BindingEditor {
    pub fn show(&mut self, ui: &mut egui::Ui, controls: &mut Controls) {
        ui.label("These controls apply to the Kit editor on this computer.");
        if let Some((action, slot)) = self.capture {
            ui.horizontal(|ui| {
                ui.label(format!("Press a key combination for {}.", action.label()));
                if ui.button("Cancel capture").clicked() {
                    self.capture = None;
                }
            });
            if let Some(binding) = ui.input_mut(|input| {
                let index = input
                    .events
                    .iter()
                    .position(|e| matches!(e, egui::Event::Key { pressed: true, repeat: false, .. }))?;
                match input.events.remove(index) {
                    egui::Event::Key { key, modifiers, .. } => Some(Binding::new(key, modifiers)),
                    _ => None,
                }
            }) {
                self.error = controls.set(action, slot, Some(binding)).err().map(|error| error.to_string());
                self.capture = None;
            }
        }
        if let Some(error) = &self.error {
            ui.colored_label(crate::theme::ERROR(), error);
        }
        if ui.button("Reset all controls").clicked() {
            *controls = Controls::default();
            *self = Self::default();
        }
        egui::ScrollArea::vertical().max_height((ui.ctx().content_rect().height() - 260.0).clamp(100.0, 420.0)).show(
            ui,
            |ui| {
                egui::Grid::new("editor_key_bindings").striped(true).show(ui, |ui| {
                    ui.strong("Action");
                    ui.strong("Primary");
                    ui.strong("Alternate");
                    ui.end_row();
                    for &action in Shortcut::ALL {
                        ui.label(action.label());
                        for slot in 0..2 {
                            ui.horizontal(|ui| {
                                let binding = controls.binding(action, slot);
                                let text = binding.as_ref().map_or_else(|| "Set…".into(), Binding::label);
                                if ui.button(text).clicked() {
                                    self.capture = Some((action, slot));
                                    self.error = None;
                                }
                                if binding.is_some() && ui.small_button("×").on_hover_text("Clear binding").clicked() {
                                    let _ = controls.set(action, slot, None);
                                    self.error = None;
                                }
                            });
                        }
                        ui.end_row();
                    }
                });
                ui.separator();
                ui.strong("Pointer and modifiers");
                let g = &mut controls.gestures;
                egui::Grid::new("editor_gestures").striped(true).show(ui, |ui| {
                    for (label, button) in [
                        ("Select / focus viewport", &mut g.select),
                        ("Pan", &mut g.pan),
                        ("Alternate pan", &mut g.alternate_pan),
                        ("Orbit", &mut g.orbit),
                        ("Draw Canvas paths", &mut g.canvas_draw),
                        ("Sculpt terrain", &mut g.sculpt),
                        ("Drag transform gizmo", &mut g.gizmo),
                        ("Open Diagram node picker", &mut g.diagram_picker),
                        ("Close script tab", &mut g.close_script_tab),
                    ] {
                        ui.label(label);
                        egui::ComboBox::from_id_salt(label).selected_text(button.label()).show_ui(ui, |ui| {
                            for value in Button::ALL {
                                ui.selectable_value(button, value, value.label());
                            }
                        });
                        ui.end_row();
                    }
                    for (label, modifier) in [
                        ("Alternate orbit", &mut g.alternate_orbit),
                        ("Fast camera", &mut g.fast_camera),
                        ("Restart Canvas path", &mut g.canvas_restart),
                        ("Reverse terrain brush", &mut g.reverse_sculpt),
                        ("Snap transform gizmo", &mut g.snap_gizmo),
                        ("Diagram multi-select", &mut g.diagram_multiselect),
                    ] {
                        ui.label(label);
                        egui::ComboBox::from_id_salt(label).selected_text(modifier.label()).show_ui(ui, |ui| {
                            for value in Modifier::ALL {
                                ui.selectable_value(modifier, value, value.label());
                            }
                        });
                        ui.end_row();
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Pan Diagram with");
                    ui.checkbox(&mut g.diagram_pan_left, "Left");
                    ui.checkbox(&mut g.diagram_pan_middle, "Middle");
                    ui.checkbox(&mut g.diagram_pan_right, "Right");
                });
                ui.checkbox(&mut g.scroll_zoom, "Scroll wheel zoom (Game View)");
                ui.add_enabled(g.scroll_zoom, egui::Checkbox::new(&mut g.invert_scroll, "Invert scroll zoom"));
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bindings_round_trip_with_an_alternate_or_no_key() {
        let mut controls = Controls::default();
        assert!(controls.set(Shortcut::Play, 0, Some(Binding::new(Key::F1, M::NONE))).is_err());
        controls.set(Shortcut::Play, 0, None).unwrap();
        assert_eq!(controls.label(Shortcut::Play), "Unbound");
        controls.set(Shortcut::Rename, 1, Some(Binding::new(Key::F8, M::SHIFT))).unwrap();
        let text = toml::to_string(&controls).unwrap();
        let restored: Controls = toml::from_str(&text).unwrap();
        assert_eq!(restored, controls);
        assert_eq!(restored.label(Shortcut::Play), "Unbound");
        assert_eq!(restored.label(Shortcut::Rename), "F2 / Shift+F8");
    }
}
