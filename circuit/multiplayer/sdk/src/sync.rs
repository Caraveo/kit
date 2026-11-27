//! What gets networked, and how little of it.
//!
//! Clients render everything themselves; Realtime only carries the state other
//! clients need to reproduce what they see. Every Kit component has a default
//! network policy below. Most are **not networked**: they are in the scene
//! file every client already has, or they're derived locally (rendering,
//! physics shapes, terrain). A developer opts entities in with a Realtime Sync
//! component and can override a policy where a game needs it (e.g. terrain
//! sculpting in a building game).

use glam::{Quat, Vec3};
use serde::{Deserialize, Serialize};

use crate::protocol::NetTransform;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Policy {
    /// Streamed while it changes (transforms of synced entities).
    Stream,
    /// Sent once when the entity appears on other clients.
    OnSpawn,
    /// Sent when it changes, as compact state (animation state, script fields).
    OnChange,
    /// Only discrete events travel (play a sound, trigger an effect).
    Events,
    /// Never sent: every client has it or computes it.
    Local,
}

pub struct ComponentPolicy {
    /// The scene-file name of the component.
    pub component: &'static str,
    pub default: Policy,
    pub why: &'static str,
    /// What a developer can switch on, when anything.
    pub override_: Option<(&'static str, Policy)>,
}

/// Default network policy for every Kit component.
pub const COMPONENTS: &[ComponentPolicy] = &[
    ComponentPolicy {
        component: "transform",
        default: Policy::Stream,
        why: "Position and rotation of synced entities; scale is off unless it animates.",
        override_: Some(("sync scale", Policy::Stream)),
    },
    ComponentPolicy {
        component: "name",
        default: Policy::OnSpawn,
        why: "Sent with the spawn (kind), never again.",
        override_: None,
    },
    ComponentPolicy {
        component: "parent",
        default: Policy::OnSpawn,
        why: "Hierarchy comes from the spawned prefab or scene.",
        override_: None,
    },
    ComponentPolicy {
        component: "model",
        default: Policy::Local,
        why: "Rendering is local; meshes and materials are already on every client.",
        override_: None,
    },
    ComponentPolicy {
        component: "sprite",
        default: Policy::Local,
        why: "Rendering is local.",
        override_: Some(("sync color/visibility as state", Policy::OnChange)),
    },
    ComponentPolicy {
        component: "light",
        default: Policy::Local,
        why: "Rendering is local; drive shared lighting with game state (time of day).",
        override_: None,
    },
    ComponentPolicy {
        component: "camera",
        default: Policy::Local,
        why: "Each player has their own view.",
        override_: None,
    },
    ComponentPolicy { component: "post_volume", default: Policy::Local, why: "Rendering is local.", override_: None },
    ComponentPolicy {
        component: "terrain",
        default: Policy::Local,
        why: "Heights are in the scene file (or regenerated from the world seed).",
        override_: Some(("terrain sculpting → world modifications by chunk", Policy::OnChange)),
    },
    ComponentPolicy {
        component: "collider",
        default: Policy::Local,
        why: "Shapes are in the scene; each client runs its own physics.",
        override_: None,
    },
    ComponentPolicy {
        component: "rigid_body",
        default: Policy::Local,
        why: "The owner simulates; others see the streamed transform (velocity optional, for smoothing).",
        override_: Some(("sync velocity", Policy::Stream)),
    },
    ComponentPolicy {
        component: "path",
        default: Policy::Local,
        why: "Path followers are deterministic from the scene and shared time.",
        override_: Some(("stream instead", Policy::Stream)),
    },
    ComponentPolicy {
        component: "rig",
        default: Policy::OnChange,
        why: "Only the animation state, time and speed travel; skeletons are evaluated locally.",
        override_: None,
    },
    ComponentPolicy {
        component: "rig_volume",
        default: Policy::Local,
        why: "Editor-time skinning data.",
        override_: None,
    },
    ComponentPolicy {
        component: "audio",
        default: Policy::Events,
        why: "Sounds play locally; only play/stop events travel.",
        override_: None,
    },
    ComponentPolicy {
        component: "script",
        default: Policy::Local,
        why: "Scripts run on the owner; fields marked networked go as state.",
        override_: Some(("networked fields", Policy::OnChange)),
    },
    ComponentPolicy { component: "diagram", default: Policy::Local, why: "Logic runs on the owner.", override_: None },
];

pub fn policy(component: &str) -> Option<&'static ComponentPolicy> {
    COMPONENTS.iter().find(|c| c.component == component)
}

/// Which scope an entity belongs to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SyncScope {
    /// The current sector (scene). Most entities.
    #[default]
    Sector,
    /// The whole world (rare: things visible from every scene).
    World,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lifetime {
    /// Leaves with its owner (after the reconnect grace).
    #[default]
    Temporary,
    /// Stays in the scope; another client can claim it.
    Persistent,
}

/// The Realtime Sync component, stored per entity in the scene file under
/// `"realtime"` (Kit keeps unknown fields, so removing the Circuit never
/// damages a scene).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RealtimeSync {
    pub position: bool,
    pub rotation: bool,
    pub scale: bool,
    pub velocity: bool,
    pub animation: bool,
    /// Updates per second while moving (0 = the scope's rate).
    pub rate: u32,
    pub interpolate: bool,
    /// Predict ahead from velocity when updates are late (needs `velocity`).
    pub extrapolate: bool,
    pub scope: SyncScope,
    pub lifetime: Lifetime,
    /// Others may take ownership (e.g. a ball anyone can kick).
    pub transferable: bool,
    /// Terrain only: send sculpt strokes as world modifications.
    pub sculpt: bool,
    /// One copy per player (a player character): each game owns its own and
    /// sees other players' copies. Off = one shared object for everyone.
    pub per_player: bool,
}

impl Default for RealtimeSync {
    fn default() -> Self {
        Self {
            position: true,
            rotation: true,
            scale: false,
            velocity: false,
            animation: true,
            rate: 0,
            interpolate: true,
            extrapolate: false,
            scope: SyncScope::Sector,
            lifetime: Lifetime::Temporary,
            transferable: false,
            sculpt: false,
            per_player: false,
        }
    }
}

impl RealtimeSync {
    /// Read from a scene entity's extra fields.
    pub fn from_extra(extra: &serde_json::Map<String, serde_json::Value>) -> Option<Self> {
        extra.get("realtime").and_then(|v| serde_json::from_value(v.clone()).ok())
    }

    pub fn flags(&self) -> u8 {
        (matches!(self.lifetime, Lifetime::Persistent) as u8 * crate::protocol::ENTITY_PERSISTENT)
            | (self.transferable as u8 * crate::protocol::ENTITY_TRANSFERABLE)
    }
}

/// Decides what changed enough to send, so still things cost nothing.
#[derive(Clone, Debug, Default)]
pub struct DirtyTracker {
    sent: Option<(Vec3, Quat, Vec3, Vec3)>,
    last_at: Option<f64>,
}

/// Smaller changes than these aren't worth a message.
pub const POSITION_EPSILON: f32 = 0.001;
pub const ROTATION_EPSILON: f32 = 0.000_05; // 1 - |dot|, about 0.6°
pub const SCALE_EPSILON: f32 = 0.001;
pub const VELOCITY_EPSILON: f32 = 0.01;

impl DirtyTracker {
    /// What to send now (only the fields that changed), or `None`.
    pub fn update(
        &mut self,
        cfg: &RealtimeSync,
        now: f64,
        rate_hz: u32,
        pos: Vec3,
        rot: Quat,
        scale: Vec3,
        vel: Vec3,
    ) -> Option<NetTransform> {
        let rate = if cfg.rate > 0 { cfg.rate } else { rate_hz.max(1) };
        if self.last_at.is_some_and(|at| now - at < 1.0 / rate as f64 - 1e-6) {
            return None;
        }
        let mut t = NetTransform::default();
        match self.sent {
            None => {
                t.position = cfg.position.then_some(pos);
                t.rotation = cfg.rotation.then_some(rot);
                t.scale = cfg.scale.then_some(scale);
                t.velocity = cfg.velocity.then_some(vel);
            }
            Some((p, r, s, v)) => {
                if cfg.position && p.distance(pos) > POSITION_EPSILON {
                    t.position = Some(pos);
                }
                if cfg.rotation && 1.0 - r.dot(rot).abs() > ROTATION_EPSILON {
                    t.rotation = Some(rot);
                }
                if cfg.scale && s.distance(scale) > SCALE_EPSILON {
                    t.scale = Some(scale);
                }
                if cfg.velocity && v.distance(vel) > VELOCITY_EPSILON {
                    t.velocity = Some(vel);
                }
            }
        }
        if t.mask() == 0 {
            return None;
        }
        let prev = self.sent.unwrap_or((pos, rot, scale, vel));
        self.sent = Some((
            t.position.unwrap_or(prev.0),
            t.rotation.unwrap_or(prev.1),
            t.scale.unwrap_or(prev.2),
            t.velocity.unwrap_or(prev.3),
        ));
        self.last_at = Some(now);
        Some(t)
    }
}

/// Smooths a remote entity: renders slightly in the past between received
/// samples, optionally extrapolating a little from velocity.
#[derive(Clone, Debug, Default)]
pub struct Interpolator {
    samples: std::collections::VecDeque<(f64, Vec3, Quat, Vec3)>,
    current: NetTransform,
}

impl Interpolator {
    /// Delay behind the newest sample (about two updates at 20 Hz).
    pub const DELAY: f64 = 0.1;
    pub const MAX_EXTRAPOLATION: f64 = 0.25;

    pub fn push(&mut self, at: f64, t: &NetTransform) {
        self.current.merge(t);
        let pos = self.current.position.unwrap_or(Vec3::ZERO);
        let rot = self.current.rotation.unwrap_or(Quat::IDENTITY);
        let vel = self.current.velocity.unwrap_or(Vec3::ZERO);
        self.samples.push_back((at, pos, rot, vel));
        while self.samples.len() > 32 {
            self.samples.pop_front();
        }
    }

    /// The latest known (unsmoothed) state.
    pub fn latest(&self) -> &NetTransform {
        &self.current
    }

    /// Position and rotation to show at `now`.
    pub fn sample(&self, now: f64, extrapolate: bool) -> Option<(Vec3, Quat)> {
        let target = now - Self::DELAY;
        let (first, last) = (self.samples.front()?, self.samples.back()?);
        if target <= first.0 {
            return Some((first.1, first.2));
        }
        if target >= last.0 {
            let ahead = (target - last.0).min(if extrapolate { Self::MAX_EXTRAPOLATION } else { 0.0 });
            return Some((last.1 + last.3 * ahead as f32, last.2));
        }
        let i = self.samples.iter().position(|s| s.0 > target).unwrap();
        let (a, b) = (&self.samples[i - 1], &self.samples[i]);
        let k = ((target - a.0) / (b.0 - a.0).max(1e-6)) as f32;
        Some((a.1.lerp(b.1, k), a.2.slerp(b.2, k)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_scene_component_has_a_policy() {
        for c in [
            "transform",
            "model",
            "sprite",
            "light",
            "camera",
            "script",
            "collider",
            "rigid_body",
            "path",
            "terrain",
            "audio",
            "post_volume",
            "diagram",
            "rig",
            "rig_volume",
        ] {
            assert!(policy(c).is_some(), "{c}");
        }
        assert_eq!(policy("terrain").unwrap().default, Policy::Local);
        assert_eq!(policy("model").unwrap().default, Policy::Local);
        assert_eq!(policy("rig").unwrap().default, Policy::OnChange);
    }

    #[test]
    fn still_entities_send_nothing() {
        let cfg = RealtimeSync::default();
        let mut d = DirtyTracker::default();
        let first = d.update(&cfg, 0.0, 20, Vec3::ONE, Quat::IDENTITY, Vec3::ONE, Vec3::ZERO).unwrap();
        assert_eq!(first.mask(), crate::protocol::f::POS | crate::protocol::f::ROT, "scale is off by default");
        assert!(d.update(&cfg, 1.0, 20, Vec3::ONE, Quat::IDENTITY, Vec3::ONE, Vec3::ZERO).is_none());
        let moved = d.update(&cfg, 1.1, 20, Vec3::new(2.0, 1.0, 1.0), Quat::IDENTITY, Vec3::ONE, Vec3::ZERO).unwrap();
        assert_eq!(moved.mask(), crate::protocol::f::POS, "only what changed");
        assert!(
            d.update(&cfg, 1.11, 20, Vec3::new(3.0, 1.0, 1.0), Quat::IDENTITY, Vec3::ONE, Vec3::ZERO).is_none(),
            "rate limit"
        );
    }

    #[test]
    fn interpolates_between_samples() {
        let mut i = Interpolator::default();
        i.push(0.0, &NetTransform { position: Some(Vec3::ZERO), ..Default::default() });
        i.push(0.1, &NetTransform { position: Some(Vec3::new(10.0, 0.0, 0.0)), ..Default::default() });
        let (p, _) = i.sample(0.15, false).unwrap();
        assert!((p.x - 5.0).abs() < 1e-4);
        let (p, _) = i.sample(1.0, false).unwrap();
        assert_eq!(p.x, 10.0, "no extrapolation unless asked");
    }

    /// The editor builds new Realtime Sync components from circuit.toml's
    /// defaults; they must mean exactly what the SDK's defaults mean.
    #[test]
    fn editor_defaults_match_the_sdk() {
        let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../circuit.toml")).unwrap();
        let doc: toml::Table = toml::from_str(&text).unwrap();
        let comp = doc["component"].as_array().unwrap().iter().find(|c| c["id"].as_str() == Some("realtime")).unwrap();
        let mut obj = serde_json::Map::new();
        for f in comp["fields"].as_array().unwrap() {
            obj.insert(f["key"].as_str().unwrap().to_owned(), serde_json::to_value(&f["default"]).unwrap());
        }
        let from_editor: RealtimeSync = serde_json::from_value(serde_json::Value::Object(obj.clone())).unwrap();
        assert_eq!(from_editor, RealtimeSync::default());
        let sdk_keys: Vec<String> =
            serde_json::to_value(RealtimeSync::default()).unwrap().as_object().unwrap().keys().cloned().collect();
        let mut editor_keys: Vec<String> = obj.keys().cloned().collect();
        let mut sdk_sorted = sdk_keys.clone();
        sdk_sorted.sort();
        editor_keys.sort();
        assert_eq!(editor_keys, sdk_sorted, "every SDK field is editable and nothing extra");
    }

    #[test]
    fn reads_the_component_from_scene_extras() {
        let extra = serde_json::json!({ "realtime": { "scale": true, "lifetime": "persistent" } });
        let s = RealtimeSync::from_extra(extra.as_object().unwrap()).unwrap();
        assert!(s.scale && s.position);
        assert_eq!(s.flags(), crate::protocol::ENTITY_PERSISTENT);
    }
}
