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
    FLOAT Speed: 45.0
    FLOAT Height: 0.25
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
    FLOAT Speed: 2.0

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
        d.script = Some(Script { path: "scripts/spinner.x".into() });
    });
    SceneFile { entities, ..Default::default() }
}

/// Create a new project folder with a starter scene and script.
pub fn create_project(dir: &Path) -> Result<Project> {
    let title = dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "Kit Game".into());
    let project = Project::create(dir, &title)?;
    project.resources.write_text("scripts/spinner.x", SPINNER_X)?;
    project.resources.write_text(&project.config.project.main_scene, &starter_scene().to_json())?;
    Ok(project)
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
    fn templates_parse() {
        kit::x::parse(SPINNER_X).unwrap();
        kit::x::parse(&script_template("Hero")).unwrap();
    }
}
