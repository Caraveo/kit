//! Translate winit events into [`kit_input::Input`].

use kit_ecs::World;
use kit_input::Input;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::keyboard::PhysicalKey;

/// Kit key name for a winit key code (`KeyA` → "a", `ArrowLeft` → "left").
pub fn key_name(key: PhysicalKey) -> Option<String> {
    match key {
        PhysicalKey::Code(code) => Some(kit_input::normalize_key(&format!("{code:?}"))),
        PhysicalKey::Unidentified(_) => None,
    }
}

pub fn mouse_name(b: MouseButton) -> String {
    match b {
        MouseButton::Left => "MouseLeft".into(),
        MouseButton::Right => "MouseRight".into(),
        MouseButton::Middle => "MouseMiddle".into(),
        MouseButton::Back => "MouseBack".into(),
        MouseButton::Forward => "MouseForward".into(),
        MouseButton::Other(n) => format!("Mouse{n}"),
    }
}

/// Feed one window event into the world's `Input` resource (if present).
pub fn handle_event(world: &World, event: &WindowEvent) {
    let Some(mut input) = world.try_resource_mut::<Input>() else { return };
    match event {
        WindowEvent::KeyboardInput { event, .. } => {
            if let Some(name) = key_name(event.physical_key) {
                match event.state {
                    ElementState::Pressed => input.press(&name),
                    ElementState::Released => input.release(&name),
                }
            }
        }
        WindowEvent::MouseInput { state, button, .. } => {
            let name = mouse_name(*button);
            match state {
                ElementState::Pressed => input.press(&name),
                ElementState::Released => input.release(&name),
            }
        }
        WindowEvent::CursorMoved { position, .. } => {
            input.set_mouse(glam::Vec2::new(position.x as f32, position.y as f32));
        }
        WindowEvent::MouseWheel { delta, .. } => {
            input.scroll += match delta {
                MouseScrollDelta::LineDelta(_, y) => *y,
                MouseScrollDelta::PixelDelta(p) => p.y as f32 / 40.0,
            };
        }
        WindowEvent::Focused(false) => input.clear(),
        _ => {}
    }
}
