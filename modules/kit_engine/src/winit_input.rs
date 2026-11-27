//! Translate winit events into [`kit_input::Input`].

use kit_ecs::World;
use kit_input::Input;
use winit::event::{DeviceEvent, ElementState, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent};
use winit::keyboard::PhysicalKey;
use winit::window::{CursorGrabMode, Window};

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
        WindowEvent::Touch(t) => {
            let down = matches!(t.phase, TouchPhase::Started | TouchPhase::Moved);
            input.touch(t.id, glam::Vec2::new(t.location.x as f32, t.location.y as f32), down);
        }
        WindowEvent::Focused(false) => input.clear(),
        _ => {}
    }
}

/// Raw device events: mouse movement while a script has locked the mouse.
pub fn handle_device_event(world: &World, event: &DeviceEvent) {
    if let DeviceEvent::MouseMotion { delta } = event {
        if let Some(mut input) = world.try_resource_mut::<Input>() {
            input.add_mouse_motion(glam::Vec2::new(delta.0 as f32, delta.1 as f32));
        }
    }
}

/// What the window's pointer was last set to, so it's only changed when needed.
#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub struct Pointer {
    pub locked: bool,
    pub hidden: bool,
}

/// Lock / hide the pointer the way scripts asked (`Input.LockMouse`,
/// `Input.ShowMouse`). `allowed` is false when the game shouldn't own the
/// pointer (the editor isn't playing, or the viewport lost focus).
pub fn apply_pointer(window: &Window, world: &World, allowed: bool, last: &mut Pointer) {
    let want = world
        .try_resource::<Input>()
        .filter(|_| allowed)
        .map(|i| Pointer { locked: i.mouse_locked, hidden: i.mouse_hidden || i.mouse_locked })
        .unwrap_or_default();
    if want == *last {
        return;
    }
    if want.locked {
        // Not every platform can lock; keeping it inside the window is the next best.
        let _ = window
            .set_cursor_grab(CursorGrabMode::Locked)
            .or_else(|_| window.set_cursor_grab(CursorGrabMode::Confined));
    } else {
        let _ = window.set_cursor_grab(CursorGrabMode::None);
    }
    window.set_cursor_visible(!want.hidden);
    *last = want;
}

/// Tell scripts the size of the game's view and where it sits in the window.
pub fn set_view(world: &World, origin: glam::Vec2, size: glam::Vec2) {
    if let Some(mut input) = world.try_resource_mut::<Input>() {
        input.view_origin = origin;
        input.screen = size;
    }
}
