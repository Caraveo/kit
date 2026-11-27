use super::*;
use bind::BindVolume;
use glam::Quat;
use kit_gamesys::{GameSysModule, MeshKind, RigClip, RigShape};
use std::f32::consts::{FRAC_PI_2, PI};

/// A tube along Y from 0 to 2 (radius 0.2), `rings` rings of 16 corners,
/// as triangle soup.
fn tube(rings: usize) -> Vec<Vertex> {
    let ring = |i: usize, j: usize| {
        let y = 2.0 * i as f32 / (rings - 1) as f32;
        let a = j as f32 / 16.0 * std::f32::consts::TAU;
        let n = Vec3::new(a.cos(), 0.0, a.sin());
        Vertex { pos: (n * 0.2 + Vec3::Y * y).to_array(), normal: n.to_array(), uv: [0.0; 2] }
    };
    let mut out = Vec::new();
    for i in 0..rings - 1 {
        for j in 0..16 {
            let (a, b, c, d) = (ring(i, j), ring(i, j + 1), ring(i + 1, j), ring(i + 1, j + 1));
            out.extend([a, c, b, b, c, d]);
        }
    }
    out
}

/// Two capsules: lower (0..1) and upper (1..2) halves of the tube.
fn two_capsules() -> Vec<BindVolume> {
    let cap = |y: f32| BindVolume {
        volume: RigVolume {
            shape: RigShape::Capsule { radius: 0.25, length: 0.5 },
            falloff: 0.3,
            ..Default::default()
        },
        rest: Mat4::from_translation(Vec3::new(0.0, y, 0.0)),
    };
    vec![cap(0.5), cap(1.5)]
}

#[test]
fn weights_are_normalized_and_follow_the_nearest_volume() {
    let mesh = tube(21);
    let vols = two_capsules();
    let w = bind::weights(&mesh, &vols);
    for (v, inf) in mesh.iter().zip(&w) {
        let total: f32 = inf.iter().map(|(_, x)| x).sum();
        assert!((total - 1.0).abs() < 1e-4, "weights sum to 1: {total}");
        let upper: f32 = inf.iter().filter(|(i, _)| *i == 1).map(|(_, x)| x).sum();
        if v.pos[1] < 0.4 {
            assert!(upper < 0.05, "bottom follows the lower volume");
        }
        if v.pos[1] > 1.6 {
            assert!(upper > 0.95, "top follows the upper volume");
        }
    }
    // Around the joint, both share smoothly.
    let mid: Vec<f32> = mesh
        .iter()
        .zip(&w)
        .filter(|(v, _)| (v.pos[1] - 1.0).abs() < 0.01)
        .map(|(_, i)| i.iter().filter(|(k, _)| *k == 1).map(|(_, x)| x).sum())
        .collect();
    assert!(mid.iter().all(|u| *u > 0.2 && *u < 0.8), "{mid:?}");
}

#[test]
fn rest_pose_reproduces_the_mesh_exactly() {
    let mesh = tube(11);
    let vols = two_capsules();
    let w = bind::weights(&mesh, &vols);
    let motions = vec![Motion::identity(); 2];
    for preserve in [false, true] {
        let mut out = Vec::new();
        deform::deform(&mesh, &w, &motions, preserve, &mut out);
        for (a, b) in mesh.iter().zip(&out) {
            assert!(Vec3::from_array(a.pos).distance(Vec3::from_array(b.pos)) < 1e-5);
            assert!(Vec3::from_array(a.normal).distance(Vec3::from_array(b.normal)) < 1e-4);
        }
    }
}

/// Bend the upper capsule 90° around the joint at y = 1.
fn bend(preserve: bool) -> (Vec<Vertex>, Vec<Vertex>) {
    let mesh = tube(21);
    let vols = two_capsules();
    let w = bind::weights(&mesh, &vols);
    let pivot = Vec3::new(0.0, 1.0, 0.0);
    let rot = Mat4::from_translation(pivot) * Mat4::from_rotation_z(FRAC_PI_2) * Mat4::from_translation(-pivot);
    let motions = vec![Motion::identity(), Motion::new(rot)];
    let mut out = Vec::new();
    deform::deform(&mesh, &w, &motions, preserve, &mut out);
    (mesh, out)
}

#[test]
fn bending_moves_the_top_keeps_the_bottom_and_never_tears() {
    for preserve in [false, true] {
        let (rest, out) = bend(preserve);
        for (a, b) in rest.iter().zip(&out) {
            let (pa, pb) = (Vec3::from_array(a.pos), Vec3::from_array(b.pos));
            assert!(pb.is_finite() && Vec3::from_array(b.normal).is_finite());
            if pa.y < 0.3 {
                assert!(pa.distance(pb) < 1e-3, "bottom stays");
            }
            if pa.y > 1.9 {
                // The top end swings to the -X side.
                assert!(pb.x < -0.6 && pb.y < 1.3, "top bends: {pb}");
            }
        }
        // Same rest position → same deformed position (no cracks between triangles).
        let mut seen: HashMap<[i32; 3], Vec3> = HashMap::new();
        for (a, b) in rest.iter().zip(&out) {
            let k = [(a.pos[0] * 1e4) as i32, (a.pos[1] * 1e4) as i32, (a.pos[2] * 1e4) as i32];
            let p = Vec3::from_array(b.pos);
            if let Some(q) = seen.insert(k, p) {
                assert!(p.distance(q) < 1e-5, "shared corners stay together");
            }
        }
    }
}

#[test]
fn preserve_volume_keeps_a_twisted_limb_thick() {
    // Twist the upper half 180° around its own axis: linear blending collapses
    // the middle ("candy wrapper"), dual quaternions keep the radius.
    let mesh = tube(21);
    let vols = two_capsules();
    let w = bind::weights(&mesh, &vols);
    let twist = Mat4::from_rotation_y(PI * 0.999);
    let motions = vec![Motion::identity(), Motion::new(twist)];
    let radius_at_joint = |preserve: bool| {
        let mut out = Vec::new();
        deform::deform(&mesh, &w, &motions, preserve, &mut out);
        let rs: Vec<f32> = mesh
            .iter()
            .zip(&out)
            .filter(|(a, _)| (a.pos[1] - 1.0).abs() < 0.01)
            .map(|(_, b)| Vec3::new(b.pos[0], 0.0, b.pos[2]).length())
            .collect();
        rs.iter().sum::<f32>() / rs.len() as f32
    };
    let (linear, dq) = (radius_at_joint(false), radius_at_joint(true));
    assert!(dq > 0.17, "dual quaternions keep ~0.2: {dq}");
    assert!(linear < dq * 0.7, "linear thins out: {linear} vs {dq}");
}

#[test]
fn clips_sample_smoothly_and_key_by_time() {
    let mut c = RigClip { name: "Wave".into(), length: 2.0, looping: true, tracks: Vec::new() };
    let up = Transform::IDENTITY;
    let mut side = Transform::IDENTITY;
    side.rotation = Quat::from_rotation_z(1.0);
    side.translation = Vec3::X;
    clip::set_key(&mut c, "Arm", 1.0, &side);
    clip::set_key(&mut c, "Arm", 0.0, &up);
    clip::set_key(&mut c, "Arm", 1.0, &side); // replaces, no duplicate
    assert_eq!(c.tracks[0].keys.len(), 2);
    assert_eq!(c.tracks[0].keys[0].time, 0.0, "kept sorted");
    let mid = clip::sample(&c.tracks[0], 0.5).unwrap();
    assert!((mid.translation.x - 0.5).abs() < 1e-5);
    assert_eq!(clip::sample(&c.tracks[0], 5.0).unwrap().translation, Vec3::X, "clamped after the last key");
    assert!((clip::clip_time(&c, 2.5) - 0.5).abs() < 1e-6, "loops");
    assert_eq!(clip::key_times(&c), vec![0.0, 1.0]);
    assert_eq!(clip::remove_keys_at(&mut c, 1.0), 1);
}

// ------------------------------------------------------------ through the engine

fn app() -> App {
    let mut app = App::new();
    app.add_module(kit_resource::ResourceModule { root: std::env::temp_dir() })
        .add_module(kit_scene::SceneModule)
        .add_module(kit_input::InputModule)
        .add_module(GameSysModule)
        .add_module(RiggModule);
    // Like the editor: edit first, Play when asked.
    app.mode = RunMode::Edit;
    app
}

/// A sphere model with two volumes: left and right halves.
fn sphere_rig(app: &mut App) -> (Entity, Entity, Entity) {
    let w = &mut app.world;
    let body = kit_scene::spawn_node(w, "Body", Transform::IDENTITY);
    w.insert(body, Model { mesh: MeshKind::Sphere, ..Default::default() });
    w.insert(body, Rig::default());
    let left =
        preset::add_volume(w, body, "L_Part", RigShape::Sphere { radius: 0.3 }, Transform::from_xyz(-0.3, 0.0, 0.0));
    let right =
        preset::add_volume(w, body, "R_Part", RigShape::Sphere { radius: 0.3 }, Transform::from_xyz(0.3, 0.0, 0.0));
    (body, left, right)
}

fn deformed(app: &App, e: Entity) -> Vec<Vertex> {
    app.world.resource::<Deformed>().meshes.get(&e).map(|m| m.vertices.clone()).unwrap_or_default()
}

#[test]
fn moving_a_volume_deforms_the_model_and_save_load_keeps_the_rig() {
    let mut app = app();
    let (body, _left, right) = sphere_rig(&mut app);
    app.update(1.0 / 60.0);
    let rest = deformed(&app, body);
    assert!(!rest.is_empty(), "bound and drawn as a deformed mesh");
    let rev = app.world.resource::<Deformed>().meshes[&body].revision;
    app.update(1.0 / 60.0);
    assert_eq!(app.world.resource::<Deformed>().meshes[&body].revision, rev, "no work when nothing moved");

    app.world.get_mut::<Transform>(right).unwrap().translation.x += 0.5;
    app.update(1.0 / 60.0);
    let moved = deformed(&app, body);
    let max_x = |v: &[Vertex]| v.iter().map(|v| v.pos[0]).fold(f32::MIN, f32::max);
    let min_x = |v: &[Vertex]| v.iter().map(|v| v.pos[0]).fold(f32::MAX, f32::min);
    assert!(max_x(&moved) > max_x(&rest) + 0.4, "right side stretched");
    assert!((min_x(&moved) - min_x(&rest)).abs() < 0.02, "left side stayed");

    // Save and load: the rig, its volumes and their rest poses survive.
    let scene = kit_gamesys::save_scene(&app.world);
    let json = scene.to_json();
    assert!(json.contains("rig_volume") && json.contains("\"rest\""));
    let mut app2 = self::app();
    kit_gamesys::load_scene(&mut app2.world, &kit_gamesys::SceneFile::from_json(&json).unwrap()).unwrap();
    app2.update(1.0 / 60.0);
    let body2 = kit_scene::find_by_name(&app2.world, "Body").unwrap();
    let again = deformed(&app2, body2);
    assert_eq!(again.len(), moved.len());
    assert!((max_x(&again) - max_x(&moved)).abs() < 1e-4, "the same pose after reload");
}

#[test]
fn clips_play_in_the_game_and_presets_mirror() {
    let mut app = app();
    let (body, left, right) = sphere_rig(&mut app);
    app.update(1.0 / 60.0);
    // Key the right part moving up over one second.
    {
        let mut r = app.world.get_mut::<Rig>(body).unwrap();
        r.clips.push(RigClip { name: "Bounce".into(), length: 1.0, looping: false, tracks: Vec::new() });
        r.autoplay = "Bounce".into();
    }
    key_pose(&mut app.world, body, "Bounce", 0.0);
    app.world.get_mut::<Transform>(right).unwrap().translation.y = 1.0;
    key_pose(&mut app.world, body, "Bounce", 1.0);
    reset_pose(&mut app.world, body);
    assert!(app.world.get::<Transform>(right).unwrap().translation.y.abs() < 1e-4, "reset to rest");
    app.mode = RunMode::Play;
    for _ in 0..90 {
        app.update(1.0 / 60.0);
    }
    assert!((app.world.get::<Transform>(right).unwrap().translation.y - 1.0).abs() < 1e-3, "clip played to its end");
    reset(&mut app.world);

    // Mirror L → R.
    app.world.get_mut::<Transform>(left).unwrap().translation = Vec3::new(-0.6, 0.2, 0.1);
    assert_eq!(mirror(&mut app.world, body, left), Some(right));
    kit_scene::propagate_transforms(&mut app.world);
    let t = rig_space(&app.world, body, right).w_axis.truncate();
    assert!(t.distance(Vec3::new(0.6, 0.2, 0.1)) < 1e-4, "{t}");

    // Saved poses.
    save_pose(&mut app.world, body, "T");
    app.world.get_mut::<Transform>(left).unwrap().translation = Vec3::ZERO;
    assert!(apply_pose(&mut app.world, body, "T"));
    assert_eq!(app.world.get::<Transform>(left).unwrap().translation, Vec3::new(-0.6, 0.2, 0.1));
}

#[test]
fn humanoid_preset_and_weight_preview() {
    assert!(preset::humanoid_names().len() >= 25);
    let mut app = app();
    let w = &mut app.world;
    let body = kit_scene::spawn_node(w, "Hero", Transform::IDENTITY);
    w.insert(body, Model { mesh: MeshKind::Sphere, ..Default::default() });
    w.insert(body, Rig::default());
    preset::add_humanoid(w, body, (Vec3::new(-0.5, -0.5, -0.5), Vec3::new(0.5, 0.5, 0.5)));
    app.update(1.0 / 60.0);
    let n = volumes_of(&app.world, body).len();
    assert!(n >= 25, "{n} volumes");
    let head = kit_scene::find_by_name(&app.world, "Head").unwrap();
    *app.world.resource_mut::<RigPreview>() =
        RigPreview { rig: Some(body), weights: true, focus: Some(head), ..Default::default() };
    app.update(1.0 / 60.0);
    let mesh = &app.world.resource::<Deformed>().meshes[&body];
    let colors = mesh.colors.as_ref().expect("preview colors");
    assert_eq!(colors.len(), mesh.vertices.len());
    // The top of the sphere is the head's: hot; the bottom isn't.
    let top = mesh.vertices.iter().zip(colors).max_by(|a, b| a.0.pos[1].total_cmp(&b.0.pos[1])).unwrap().1;
    let bottom = mesh.vertices.iter().zip(colors).min_by(|a, b| a.0.pos[1].total_cmp(&b.0.pos[1])).unwrap().1;
    assert!(top[0] > 0.8 && bottom[0] < 0.4, "{top:?} {bottom:?}");
}

#[test]
fn connected_volume_follows_and_keeps_its_fitted_length() {
    let mut app = app();
    let (body, parent, _) = sphere_rig(&mut app);
    let child = preset::add_volume(
        &mut app.world,
        parent,
        "Linked control",
        RigShape::Sphere { radius: 0.1 },
        Transform::from_xyz(0.0, 2.0, 0.0),
    );
    rebind(&mut app.world, body);
    app.world.get_mut::<Transform>(parent).unwrap().translation += Vec3::X;
    app.world.get_mut::<Transform>(child).unwrap().translation = Vec3::new(3.0, 4.0, 0.0);
    app.update(1.0 / 60.0);

    let center = |e| app.world.get::<GlobalTransform>(e).unwrap().0.w_axis.truncate();
    let offset = center(child) - center(parent);
    assert!(offset.abs_diff_eq(Vec3::new(1.2, 1.6, 0.0), 1e-5));

    let parent_before = center(parent);
    let desired = center(child) + Vec3::Y * 4.0;
    move_control(&mut app.world, body, child, desired);
    let position = |e| app.world.get::<GlobalTransform>(e).unwrap().0.w_axis.truncate();
    assert!(position(child).abs_diff_eq(desired, 1e-5));
    assert!(position(parent).distance(parent_before) > 2.0, "the child pulls its parent");
    assert!((position(parent).distance(position(child)) - 2.0).abs() < 1e-5);

    app.world.get_mut::<Transform>(parent).unwrap().scale = Vec3::new(2.0, 3.0, 2.0);
    app.update(1.0 / 60.0);
    let position = |e| app.world.get::<GlobalTransform>(e).unwrap().0.w_axis.truncate();
    assert!((position(parent).distance(position(child)) - 2.0).abs() < 1e-5);

    *app.world.resource_mut::<RigPreview>() = RigPreview { rig: Some(body), fitting: true, ..Default::default() };
    app.world.get_mut::<Transform>(child).unwrap().translation = Vec3::Y * 3.0;
    app.update(1.0 / 60.0);
    assert_eq!(app.world.get::<Transform>(child).unwrap().translation, Vec3::Y * 3.0);
}

#[test]
fn a_spinning_rigged_model_is_not_re_deformed_every_frame() {
    let mut app = app();
    let (body, _, _) = sphere_rig(&mut app);
    app.update(1.0 / 60.0);
    let rev = app.world.resource::<Deformed>().meshes[&body].revision;
    // Spin the whole model: the volumes turn with it, so the pose doesn't change.
    for i in 0..30 {
        app.world.get_mut::<Transform>(body).unwrap().rotation = Quat::from_rotation_y(i as f32 * 0.37);
        app.update(1.0 / 60.0);
    }
    assert_eq!(app.world.resource::<Deformed>().meshes[&body].revision, rev, "no re-deform from rigid motion");
}
