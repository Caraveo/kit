//! Proves the Vulkan backend can sample the scene's depth with MSAA on
//! (post-processing's Focus, Ambient and "Objects inside" volumes rely on it).
//! Skipped when no Vulkan device is available.

use glam::{Vec3, Vec4};
use kit_ecs::World;
use kit_gamesys::{MeshKind, Model};
use kit_graphics::{Gfx, Image, vk};
use kit_render::{Renderer, Target, View};
use kit_scene::Transform;

fn wall_world() -> World {
    let mut w = World::new();
    let e = kit_scene::spawn_node(
        &mut w,
        "Wall",
        Transform { translation: Vec3::new(0.0, 0.0, -5.0), scale: Vec3::new(40.0, 40.0, 0.1), ..Transform::IDENTITY },
    );
    w.insert(e, Model { mesh: MeshKind::Cube, color: Vec4::ONE, ..Default::default() });
    kit_scene::propagate_transforms(&mut w);
    w
}

fn view(orthographic: bool) -> View {
    let mut v = View::fallback(1.0);
    v.view = glam::camera::rh::view::look_at_mat4(Vec3::ZERO, Vec3::NEG_Z, Vec3::Y);
    v.camera_pos = Vec3::ZERO;
    v.proj = if orthographic {
        glam::camera::rh::proj::vulkan::orthographic(-5.0, 5.0, -5.0, 5.0, 0.1, 100.0)
    } else {
        glam::camera::rh::proj::vulkan::perspective(60f32.to_radians(), 1.0, 0.1, 100.0)
    };
    v
}

#[test]
fn msaa_depth_is_sampleable_in_post() {
    let Ok(gfx) = Gfx::new(None, "kit post depth test") else {
        eprintln!("no Vulkan device: skipped");
        return;
    };
    println!("GPU {} with {:?} MSAA", gfx.device_name, gfx.caps.msaa);
    let mut renderer = Renderer::new(&gfx).unwrap();
    let world = wall_world();
    let extent = vk::Extent2D { width: 64, height: 64 };
    let format = vk::Format::R8G8B8A8_UNORM;
    for orthographic in [false, true] {
        let mut image = Image::new(
            &gfx,
            extent,
            format,
            vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_SRC,
            vk::SampleCountFlags::TYPE_1,
            "depth test target",
        )
        .unwrap();
        let target = Target { image: image.image, view: image.view, extent, format };
        let v = view(orthographic);
        let mut result = Ok(());
        gfx.one_shot(|cmd| {
            gfx.image_barrier(
                cmd,
                image.image,
                vk::ImageAspectFlags::COLOR,
                vk::ImageLayout::UNDEFINED,
                vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                (vk::PipelineStageFlags::TOP_OF_PIPE, vk::AccessFlags::empty()),
                (vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT, vk::AccessFlags::COLOR_ATTACHMENT_WRITE),
            );
            result = renderer.render_debug_depth(&gfx, cmd, &target, &v, &world, 10.0);
        })
        .unwrap();
        result.unwrap();
        let pixels = image.read_back(&gfx, vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL).unwrap();
        for (x, y) in [(32, 32), (5, 5), (60, 10), (10, 58)] {
            let i = (y * 64 + x) * 4;
            // The wall's front face is 4.95 units away; written as 4.95/10.
            let depth = pixels[i] as f32 / 255.0 * 10.0;
            assert!(
                (depth - 4.95).abs() < 0.1,
                "{} camera, pixel ({x},{y}): depth {depth} (rgba {:?})",
                if orthographic { "orthographic" } else { "perspective" },
                &pixels[i..i + 4]
            );
        }
        image.destroy(&gfx);
    }
    renderer.destroy(&gfx);
}
