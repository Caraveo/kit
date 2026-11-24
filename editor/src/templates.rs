//! Starter content for new projects and scripts.

use anyhow::Result;
use kit::gamesys::{Camera, EntityData, Light, MeshKind, Model, SceneFile, Script};
use kit::math::{Quat, Vec3, Vec4};
use kit::resource::Project;
use kit::scene::Transform;
use std::path::Path;

pub const SPINNER_X: &str = r#"# Spins the object and bobs it up and down.
TAKE Transform, Time, Math

CREATE Spinner[
    EDIT FLOAT Speed: 45.0
    EDIT FLOAT Height: 0.25
    VECTOR Start: VECTOR()

    VOID CREATE Init()[
        Start: Transform.Position
    ]

    VOID CREATE Update(FLOAT dt)[
        Transform.Rotate(0, Speed * dt, 0)
        FLOAT Y: Math.Sin(Time.Elapsed * 2.0) * Height
        Transform.Position: Start + VECTOR(0, Y, 0)
    ]
]
"#;

/// Body of a new script whose class is named after the file.
pub fn script_template(class: &str) -> String {
    format!(
        r#"# {class}: attach this script to an entity in the inspector.
TAKE Transform, Input, Time

CREATE {class}[
    EDIT FLOAT Speed: 2.0

    VOID CREATE Init()[
        DISPLAY["{class} ready"]
    ]

    VOID CREATE Update(FLOAT dt)[
        # Move with the arrow keys.
        FLOAT X: Input.Axis("Left", "Right")
        FLOAT Y: Input.Axis("Down", "Up")
        Transform.Move(X * Speed * dt, Y * Speed * dt, 0)
    ]
]
"#
    )
}

/// Class name for a script file name: `enemy_ai.x` → `EnemyAi`.
pub fn class_name_for(file_stem: &str) -> String {
    let mut out = String::new();
    for part in file_stem.split(|c: char| !c.is_alphanumeric()).filter(|p| !p.is_empty()) {
        let mut chars = part.chars();
        if let Some(f) = chars.next() {
            out.extend(f.to_uppercase());
            out.push_str(chars.as_str());
        }
    }
    if out.is_empty() || out.chars().next().unwrap().is_ascii_digit() {
        out.insert_str(0, "Script");
    }
    out
}

pub fn starter_scene() -> SceneFile {
    let mut entities = Vec::new();
    let mut push = |name: &str, transform: Transform, f: &dyn Fn(&mut EntityData)| {
        let mut d = EntityData { id: entities.len() as u32, name: name.into(), transform, ..Default::default() };
        f(&mut d);
        entities.push(d);
    };
    let mut cam_t = Transform::from_xyz(0.0, 2.4, 7.0);
    cam_t.look_at(Vec3::new(0.0, 0.6, 0.0), Vec3::Y);
    push("Camera", cam_t, &|d| d.camera = Some(Camera::perspective()));
    let light_t = Transform::IDENTITY.with_rotation(Quat::from_euler(kit::math::EulerRot::YXZ, 0.6, -0.9, 0.0));
    push("Sun", light_t, &|d| d.light = Some(Light::default()));
    push("Cube", Transform::from_xyz(0.0, 0.75, 0.0).with_scale(Vec3::splat(1.2)), &|d| {
        d.model = Some(Model { mesh: MeshKind::Cube, color: Vec4::new(0.76, 0.66, 1.0, 1.0), ..Default::default() });
        d.script = Some(Script::new("scripts/spinner.x"));
    });
    SceneFile { entities, ..Default::default() }
}

/// Create a new project folder with a starter scene and script.
#[cfg(test)]
pub fn create_project(dir: &Path) -> Result<Project> {
    let title = dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "Kit Game".into());
    create_from(dir, &NewProject { title, ..Default::default() })
}

/// What a new project starts with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Template {
    /// Camera, light and a spinning cube with a script.
    #[default]
    Starter3D,
    /// Camera and light only.
    Empty3D,
    /// Rolling terrain and a fly-around camera.
    Explorer3D,
    /// A cube driven by a Diagram (visual logic, no code).
    Diagram3D,
    /// A 2D camera, a player square that moves with the arrow keys, and ground.
    Starter2D,
    /// A 2D camera only.
    Empty2D,
}

impl Template {
    pub const ALL: [Template; 6] = [
        Template::Starter3D,
        Template::Explorer3D,
        Template::Diagram3D,
        Template::Starter2D,
        Template::Empty3D,
        Template::Empty2D,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Template::Starter3D => "3D Starter",
            Template::Empty3D => "Empty 3D",
            Template::Explorer3D => "3D Explorer",
            Template::Diagram3D => "Diagram Starter",
            Template::Starter2D => "2D Starter",
            Template::Empty2D => "Empty 2D",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Template::Starter3D => "◆",
            Template::Empty3D => "◇",
            Template::Explorer3D => "⛰",
            Template::Diagram3D => "◫",
            Template::Starter2D => "▣",
            Template::Empty2D => "□",
        }
    }

    pub fn blurb(self) -> &'static str {
        match self {
            Template::Starter3D => {
                "A camera, a sun and a spinning cube with an x script. The quickest way to start in 3D."
            }
            Template::Empty3D => "A camera and a sun. Build everything yourself.",
            Template::Explorer3D => {
                "Rolling hills and a camera you fly with WASD and the mouse (FreeCamera.x). Great for worlds and levels."
            }
            Template::Diagram3D => "A cube that rises and changes color, built with a Diagram using nodes and wires.",
            Template::Starter2D => "A 2D camera, ground, and a player square that moves with the arrow keys.",
            Template::Empty2D => "A 2D (orthographic) camera only.",
        }
    }

    pub fn two_d(self) -> bool {
        matches!(self, Template::Starter2D | Template::Empty2D)
    }

    /// Name ideas for the wizard's 🎲 button.
    pub fn suggestions(self) -> &'static [&'static str] {
        match self {
            Template::Explorer3D => &["Skyward Isles", "Valley of Echoes", "Lost Meadow", "Far Horizon", "Mossy Peaks"],
            Template::Diagram3D => &["Clockwork Lab", "Signal Garden", "Gear Works", "Logic Lane", "Switchyard"],
            Template::Starter2D | Template::Empty2D => {
                &["Pixel Dash", "Tiny Knight", "Neon Hopper", "Paper Quest", "Box Runner"]
            }
            _ => &["Violet Run", "Starlight Drift", "Cube Odyssey", "Moonlit Arena", "Ember Trail", "Orbit Garden"],
        }
    }
}

/// Settings from the New Project wizard.
#[derive(Clone, Debug)]
pub struct NewProject {
    pub title: String,
    pub template: Template,
    pub width: u32,
    pub height: u32,
    pub physics: bool,
}

impl Default for NewProject {
    fn default() -> Self {
        Self { title: "Kit Game".into(), template: Template::Starter3D, width: 1280, height: 720, physics: true }
    }
}

/// Create a project in `dir` (made if missing; must be empty) from a template.
pub fn create_from(dir: &Path, opts: &NewProject) -> Result<Project> {
    if dir.exists() && std::fs::read_dir(dir)?.next().is_some() {
        anyhow::bail!("{} isn't empty. Pick a new folder name.", dir.display());
    }
    let mut project = Project::create(dir, opts.title.trim())?;
    project.config.display.width = opts.width;
    project.config.display.height = opts.height;
    project.config.physics.enabled = opts.physics;
    project.save_config()?;
    let r = &project.resources;
    let scene = match opts.template {
        Template::Starter3D => {
            r.write_text("scripts/spinner.x", SPINNER_X)?;
            starter_scene()
        }
        Template::Empty3D => scene_3d(|_| {}),
        Template::Explorer3D => {
            r.write_text("scripts/FreeCamera.x", FREE_CAMERA_X)?;
            explorer_scene()
        }
        Template::Diagram3D => {
            r.write_text("diagrams/lift.kdiagram", &lift_diagram().to_json())?;
            scene_3d(|push| {
                push(EntityData {
                    name: "Lift".into(),
                    transform: Transform::from_xyz(0.0, 0.5, 0.0),
                    model: Some(Model {
                        mesh: MeshKind::Cube,
                        color: Vec4::new(0.76, 0.66, 1.0, 1.0),
                        ..Default::default()
                    }),
                    diagram: Some(kit::gamesys::Diagram { graph: "diagrams/lift.kdiagram".into() }),
                    ..Default::default()
                });
                push(EntityData {
                    name: "Floor".into(),
                    transform: Transform::from_xyz(0.0, 0.0, 0.0).with_scale(Vec3::new(8.0, 1.0, 8.0)),
                    model: Some(Model {
                        mesh: MeshKind::Plane,
                        color: Vec4::new(0.25, 0.22, 0.32, 1.0),
                        ..Default::default()
                    }),
                    ..Default::default()
                });
            })
        }
        Template::Starter2D => {
            r.write_text("scripts/player.x", &script_template("Player"))?;
            scene_2d(true)
        }
        Template::Empty2D => scene_2d(false),
    };
    r.write_text(&project.config.project.main_scene, &scene.to_json())?;
    Ok(project)
}

const FREE_CAMERA_X: &str = include_str!("../../examples/hello/scripts/FreeCamera.x");

/// Camera + sun, plus whatever `extra` adds.
fn scene_3d(extra: impl FnOnce(&mut dyn FnMut(EntityData))) -> SceneFile {
    let mut entities: Vec<EntityData> = Vec::new();
    let mut push = |mut d: EntityData| {
        d.id = entities.len() as u32;
        entities.push(d);
    };
    let mut cam_t = Transform::from_xyz(0.0, 2.4, 7.0);
    cam_t.look_at(Vec3::new(0.0, 0.6, 0.0), Vec3::Y);
    push(EntityData {
        name: "Camera".into(),
        transform: cam_t,
        camera: Some(Camera::perspective()),
        ..Default::default()
    });
    let light_t = Transform::IDENTITY.with_rotation(Quat::from_euler(kit::math::EulerRot::YXZ, 0.6, -0.9, 0.0));
    push(EntityData { name: "Sun".into(), transform: light_t, light: Some(Light::default()), ..Default::default() });
    extra(&mut push);
    SceneFile { entities, ..Default::default() }
}

fn explorer_scene() -> SceneFile {
    let mut scene = scene_3d(|push| {
        let mut t = kit::gamesys::Terrain::default();
        t.generate(7, 7.0, 0.55);
        push(EntityData {
            name: "Terrain".into(),
            transform: Transform::from_xyz(0.0, -0.5, 0.0),
            terrain: Some(t),
            ..Default::default()
        });
    });
    let cam = &mut scene.entities[0];
    cam.transform = Transform::from_xyz(0.0, 9.0, 22.0);
    cam.transform.look_at(Vec3::new(0.0, 2.0, 0.0), Vec3::Y);
    cam.script = Some(Script::new("scripts/FreeCamera.x"));
    scene
}

fn scene_2d(starter: bool) -> SceneFile {
    let mut entities: Vec<EntityData> = Vec::new();
    let mut push = |mut d: EntityData| {
        d.id = entities.len() as u32;
        entities.push(d);
    };
    push(EntityData {
        name: "Camera".into(),
        transform: Transform::from_xyz(0.0, 0.0, 10.0),
        camera: Some(Camera::orthographic(10.0)),
        ..Default::default()
    });
    if starter {
        let sprite = |w: f32, h: f32, color: Vec4| kit::gamesys::Sprite {
            size: kit::math::Vec2::new(w, h),
            color,
            ..Default::default()
        };
        push(EntityData {
            name: "Ground".into(),
            transform: Transform::from_xyz(0.0, -3.0, 0.0),
            sprite: Some(sprite(16.0, 1.0, Vec4::new(0.3, 0.26, 0.42, 1.0))),
            ..Default::default()
        });
        push(EntityData {
            name: "Player".into(),
            transform: Transform::from_xyz(0.0, -2.0, 0.0),
            sprite: Some(sprite(1.0, 1.0, Vec4::new(0.76, 0.66, 1.0, 1.0))),
            script: Some(Script::new("scripts/player.x")),
            ..Default::default()
        });
    }
    SceneFile { entities, ..Default::default() }
}

/// On Start → rise over 2 s → turn green.
fn lift_diagram() -> kit::diagram::Graph {
    use kit::diagram::{Graph, NodeKind, PinRef};
    use kit::gamedata::Value;
    let mut g = Graph::default();
    let start = g.add(NodeKind::OnStart, [0.0, 0.0]);
    let rise = g.add(NodeKind::MoveOverTime, [320.0, 0.0]);
    let tint = g.add(NodeKind::SetTint, [640.0, 80.0]);
    let n = g.node_mut(rise).unwrap();
    n.inputs.insert("destination".into(), Value::Vector3([0.0, 2.5, 0.0]));
    n.inputs.insert("duration".into(), Value::Float(2.0));
    g.node_mut(tint).unwrap().inputs.insert("color".into(), Value::Color([0.4, 0.95, 0.6, 1.0]));
    g.connect(PinRef::new(start, "then"), PinRef::new(rise, "exec"), true);
    g.connect(PinRef::new(rise, "completed"), PinRef::new(tint, "exec"), true);
    g
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_names() {
        assert_eq!(class_name_for("enemy_ai"), "EnemyAi");
        assert_eq!(class_name_for("player"), "Player");
        assert_eq!(class_name_for("2d-thing"), "Script2dThing");
    }

    #[test]
    fn every_template_creates_a_project_that_opens() {
        for t in Template::ALL {
            let dir = std::env::temp_dir().join(format!("kit_tpl_{:?}_{}", t, std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            let p = create_from(
                &dir,
                &NewProject {
                    title: format!("Test {}", t.name()),
                    template: t,
                    width: 1920,
                    height: 1080,
                    physics: false,
                },
            )
            .unwrap();
            assert_eq!((p.config.display.width, p.config.physics.enabled), (1920, false));
            let reopened = Project::open(&dir).unwrap();
            assert_eq!(reopened.config.project.title, format!("Test {}", t.name()));
            let scene = SceneFile::from_json(&reopened.resources.read_text("main.kscene").unwrap()).unwrap();
            assert!(scene.entities.iter().any(|e| e.camera.is_some()), "{t:?} has a camera");
            for e in &scene.entities {
                if let Some(s) = &e.script {
                    kit::x::parse(&reopened.resources.read_text(&s.path).unwrap()).unwrap();
                }
                if let Some(d) = &e.diagram {
                    let g = kit::diagram::Graph::from_json(&reopened.resources.read_text(&d.graph).unwrap()).unwrap();
                    assert!(!kit::diagram::validate::has_errors(&kit::diagram::validate(&g, &Default::default())));
                }
            }
            // A non-empty folder is refused.
            assert!(create_from(&dir, &NewProject::default()).is_err());
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn templates_parse() {
        kit::x::parse(SPINNER_X).unwrap();
        kit::x::parse(&script_template("Hero")).unwrap();
    }
}
