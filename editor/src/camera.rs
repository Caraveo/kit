//! The editor's viewport camera: orbit in 3D, pan/zoom in 2D.

use kit::math::{Mat4, Vec2, Vec3};
use kit::render::{GridPlane, View};

pub struct EditorCamera {
    pub target: Vec3,
    /// Radians around Y.
    pub yaw: f32,
    /// Radians above the horizon.
    pub pitch: f32,
    pub distance: f32,
    /// 2D mode: orthographic, looking down -Z at the XY plane.
    pub two_d: bool,
    /// Visible height in world units for 2D mode.
    pub ortho_height: f32,
    /// Auto-rotate (Dimension's R key).
    pub auto_rotate: bool,
    /// Where an animated "frame" move is heading: (target, distance, ortho height).
    goal: Option<(Vec3, f32, f32)>,
}

impl Default for EditorCamera {
    fn default() -> Self {
        Self {
            target: Vec3::new(0.0, 0.5, 0.0),
            yaw: 0.6,
            pitch: 0.42,
            distance: 9.0,
            two_d: false,
            ortho_height: 10.0,
            auto_rotate: false,
            goal: None,
        }
    }
}

impl EditorCamera {
    pub fn eye(&self) -> Vec3 {
        if self.two_d {
            return Vec3::new(self.target.x, self.target.y, 50.0);
        }
        let dir = Vec3::new(self.pitch.cos() * self.yaw.sin(), self.pitch.sin(), self.pitch.cos() * self.yaw.cos());
        self.target + dir * self.distance
    }

    pub fn view_matrix(&self) -> Mat4 {
        if self.two_d {
            let eye = self.eye();
            return kit::math::camera::rh::view::look_at_mat4(eye, Vec3::new(eye.x, eye.y, 0.0), Vec3::Y);
        }
        kit::math::camera::rh::view::look_at_mat4(self.eye(), self.target, Vec3::Y)
    }

    pub fn proj_matrix(&self, aspect: f32) -> Mat4 {
        let aspect = aspect.max(1e-3);
        if self.two_d {
            let h = self.ortho_height * 0.5;
            let w = h * aspect;
            kit::math::camera::rh::proj::vulkan::orthographic(-w, w, -h, h, 0.1, 200.0)
        } else {
            let near = (self.distance * 0.005).clamp(0.01, 0.5);
            kit::math::camera::rh::proj::vulkan::perspective(45f32.to_radians(), aspect, near, 2000.0)
        }
    }

    pub fn view(&self, aspect: f32) -> View {
        let mut v = View::fallback(aspect);
        v.view = self.view_matrix();
        v.proj = self.proj_matrix(aspect);
        v.camera_pos = self.eye();
        v.backdrop = true;
        v.grid = Some(if self.two_d { GridPlane::XY } else { GridPlane::XZ });
        v.grid_fade = if self.two_d { 1000.0 } else { (self.distance * 6.0).clamp(40.0, 600.0) };
        v
    }

    pub fn orbit(&mut self, delta: Vec2) {
        self.goal = None;
        if self.two_d {
            return;
        }
        self.yaw -= delta.x * 0.008;
        self.pitch = (self.pitch + delta.y * 0.008).clamp(-1.54, 1.54);
    }

    /// Pan by a screen-space delta (pixels) given the viewport height in pixels.
    pub fn pan(&mut self, delta: Vec2, viewport_height: f32) {
        self.goal = None;
        let h = viewport_height.max(1.0);
        if self.two_d {
            let scale = self.ortho_height / h;
            self.target.x -= delta.x * scale;
            self.target.y += delta.y * scale;
            return;
        }
        let view = self.view_matrix();
        let right = view.row(0).truncate();
        let up = view.row(1).truncate();
        let scale = 2.0 * self.distance * (22.5f32.to_radians()).tan() / h;
        self.target += (-right * delta.x + up * delta.y) * scale;
    }

    /// Keyboard camera: `dir` = (A/D, Q/E, W/S) in -1…1, held for `dt` seconds.
    /// W/S zoom in and out, A/D pan the view left and right, Q/E move it
    /// down and up. Speeds follow the zoom level; `fast` (Shift) triples them.
    pub fn fly(&mut self, dir: Vec3, dt: f32, fast: bool) {
        self.goal = None;
        let boost = if fast { 3.0 } else { 1.0 };
        let reach = if self.two_d { self.ortho_height } else { self.distance };
        let speed = reach.max(1.0) * 0.9 * boost;
        // Zoom: about ×2 per second held.
        let zoom = (-dir.z * 0.7 * boost * dt).exp();
        if self.two_d {
            self.ortho_height = (self.ortho_height * zoom).clamp(0.5, 2000.0);
            self.target.x += dir.x * speed * dt;
            self.target.y += dir.y * speed * dt;
            return;
        }
        self.distance = (self.distance * zoom).clamp(0.3, 1500.0);
        let forward = (self.target - self.eye()).normalize_or(Vec3::NEG_Z);
        let right = forward.cross(Vec3::Y).normalize_or(Vec3::X);
        self.target += (right * dir.x + Vec3::Y * dir.y) * speed * dt;
    }

    pub fn zoom(&mut self, scroll: f32) {
        self.goal = None;
        let f = (-scroll * 0.12).exp();
        if self.two_d {
            self.ortho_height = (self.ortho_height * f).clamp(0.5, 2000.0);
        } else {
            self.distance = (self.distance * f).clamp(0.3, 1500.0);
        }
    }

    /// Frame a sphere (center, radius) instantly.
    pub fn frame(&mut self, center: Vec3, radius: f32) {
        let (t, d, h) = self.framing(center, radius);
        self.target = t;
        self.distance = d;
        self.ortho_height = h;
        self.goal = None;
    }

    /// Glide to frame a sphere over a fraction of a second.
    pub fn frame_animated(&mut self, center: Vec3, radius: f32) {
        self.goal = Some(self.framing(center, radius));
    }

    fn framing(&self, center: Vec3, radius: f32) -> (Vec3, f32, f32) {
        let r = radius.max(0.25);
        let distance = if self.two_d { self.distance } else { r / (22.5f32.to_radians()).sin() * 1.15 };
        let ortho = if self.two_d { r * 3.0 } else { self.ortho_height };
        (center, distance.clamp(0.3, 1500.0), ortho.clamp(0.5, 2000.0))
    }

    /// Is a framing animation running?
    pub fn animating(&self) -> bool {
        self.goal.is_some()
    }

    pub fn set_two_d(&mut self, two_d: bool) {
        self.two_d = two_d;
        if two_d {
            self.ortho_height = (self.distance * 0.8).clamp(2.0, 200.0);
        }
    }

    pub fn tick(&mut self, dt: f32) {
        if let Some((t, d, h)) = self.goal {
            // Exponential ease: ~90% of the way in a quarter second.
            let k = 1.0 - (-dt * 9.0).exp();
            self.target = self.target.lerp(t, k);
            self.distance += (d - self.distance) * k;
            self.ortho_height += (h - self.ortho_height) * k;
            if self.target.distance(t) < 1e-3
                && (self.distance - d).abs() < 1e-3
                && (self.ortho_height - h).abs() < 1e-3
            {
                self.target = t;
                self.distance = d;
                self.ortho_height = h;
                self.goal = None;
            }
        }
        if self.auto_rotate && !self.two_d {
            self.yaw += dt * 0.35;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framing_glides_and_fits_size() {
        let mut cam = EditorCamera::default();
        cam.frame_animated(Vec3::new(10.0, 0.0, 0.0), 1.0);
        let small = cam.goal.unwrap().1;
        for _ in 0..120 {
            cam.tick(1.0 / 60.0);
        }
        assert!(!cam.animating());
        assert!(cam.target.distance(Vec3::new(10.0, 0.0, 0.0)) < 1e-3);
        cam.frame_animated(Vec3::ZERO, 20.0);
        assert!(cam.goal.unwrap().1 > small * 10.0, "bigger objects zoom out");
        cam.tick(1.0 / 60.0);
        assert!(cam.animating());
        cam.orbit(Vec2::new(5.0, 0.0));
        assert!(!cam.animating(), "user input cancels the glide");
    }

    #[test]
    fn w_s_zoom_and_a_d_pan() {
        let mut cam = EditorCamera { yaw: 0.0, pitch: 0.0, ..Default::default() };
        let (start, d) = (cam.target, cam.distance);
        cam.fly(Vec3::new(0.0, 0.0, 1.0), 0.5, false);
        assert!(cam.distance < d * 0.9, "W zooms in");
        assert_eq!(cam.target, start, "zooming doesn't move the view");
        cam.fly(Vec3::new(0.0, 0.0, -1.0), 1.0, false);
        assert!(cam.distance > d, "S zooms out");
        // Yaw 0 looks toward -Z, so D pans the view to +X.
        cam.fly(Vec3::new(1.0, 0.0, 0.0), 0.5, false);
        assert!(cam.target.x > start.x + 1.0 && (cam.target.z - start.z).abs() < 1e-4, "{:?}", cam.target);
        // 2D: W/S zoom the framing.
        let mut flat = EditorCamera::default();
        flat.set_two_d(true);
        let h = flat.ortho_height;
        flat.fly(Vec3::new(0.0, 0.0, 1.0), 0.5, false);
        assert!(flat.ortho_height < h);
    }
}
