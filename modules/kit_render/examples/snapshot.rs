//! Render a test scene offscreen and save it as a PNG (validates the Vulkan path
//! without a window): `cargo run -p kit_render --example snapshot -- out.png`

use glam::{Quat, Vec2, Vec3, Vec4};
use kit_ecs::World;
use kit_gamesys::{Camera, MeshKind, Model, Sprite};
use kit_graphics::{Gfx, Image, vk};
use kit_render::{GridPlane, Renderer, Target, View};
use kit_scene::Transform;

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let out = std::env::args().nth(1).unwrap_or_else(|| "snapshot.png".into());
    let two_d = std::env::args().any(|a| a == "--2d");

    let gfx = Gfx::new(None, "kit snapshot")?;
    let mut renderer = Renderer::new(&gfx)?;

    let mut world = World::new();
    let cam = kit_scene::spawn_node(&mut world, "Camera", Transform::from_xyz(0.0, 2.2, 6.5));
    world.get_mut::<Transform>(cam).unwrap().look_at(Vec3::new(0.0, 0.6, 0.0), Vec3::Y);
    world.insert(cam, if two_d { Camera::orthographic(8.0) } else { Camera::perspective() });
    if two_d {
        *world.get_mut::<Transform>(cam).unwrap() = Transform::from_xyz(0.0, 0.0, 10.0);
    }
    let shapes = [
        (MeshKind::Cube, -2.2, Vec4::new(0.76, 0.66, 1.0, 1.0)),
        (MeshKind::Sphere, 0.0, Vec4::new(0.95, 0.93, 1.0, 1.0)),
        (MeshKind::Torus, 2.2, Vec4::new(1.0, 0.6, 0.8, 1.0)),
    ];
    for (mesh, x, color) in shapes {
        let mut t = Transform::from_xyz(x, 0.7, 0.0).with_scale(Vec3::splat(1.3));
        t.rotation = Quat::from_rotation_y(0.5) * Quat::from_rotation_x(0.35);
        let e = kit_scene::spawn_node(&mut world, mesh.label(), t);
        world.insert(e, Model { mesh, color, ..Default::default() });
    }
    let s = kit_scene::spawn_node(&mut world, "Sprite", Transform::from_xyz(0.0, 2.4, 0.5));
    world.insert(s, Sprite { size: Vec2::new(1.5, 0.5), color: Vec4::new(0.4, 0.9, 0.7, 0.8), ..Default::default() });
    kit_scene::propagate_transforms(&mut world);

    let extent = vk::Extent2D { width: 960, height: 540 };
    let format = vk::Format::R8G8B8A8_SRGB;
    let mut color = Image::new(
        &gfx,
        extent,
        format,
        vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_SRC,
        vk::SampleCountFlags::TYPE_1,
        "snapshot",
    )?;
    let mut view = View::from_camera(&world, extent.width as f32 / extent.height as f32).unwrap();
    view.backdrop = true;
    view.grid = Some(if two_d { GridPlane::XY } else { GridPlane::XZ });
    view.selected = vec![kit_scene::find_by_name(&world, "Sphere").unwrap()];

    renderer.prepare(&gfx, &world);
    let target = Target { image: color.image, view: color.view, extent, format };
    let mut result = Ok(());
    gfx.one_shot(|cmd| {
        gfx.image_barrier(
            cmd,
            color.image,
            vk::ImageAspectFlags::COLOR,
            vk::ImageLayout::UNDEFINED,
            vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
            (vk::PipelineStageFlags::TOP_OF_PIPE, vk::AccessFlags::empty()),
            (vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT, vk::AccessFlags::COLOR_ATTACHMENT_WRITE),
        );
        result = renderer.render(&gfx, cmd, 0, &target, &view, &world);
    })?;
    result?;
    let pixels = color.read_back(&gfx, vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)?;
    image::save_buffer(&out, &pixels, extent.width, extent.height, image::ColorType::Rgba8)?;
    println!("wrote {out} on {}", gfx.device_name);

    color.destroy(&gfx);
    renderer.destroy(&gfx);
    Ok(())
}
