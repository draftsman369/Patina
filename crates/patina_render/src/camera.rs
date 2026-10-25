use glam::{Mat4, Vec3};
use patina_mesh::Aabb;

/// Turntable camera orbiting a target point.
#[derive(Clone, Debug, PartialEq)]
pub struct OrbitCamera {
    pub target: Vec3,
    /// Rotation around the world Y axis, radians.
    pub yaw: f32,
    /// Elevation, radians, kept within ±89°.
    pub pitch: f32,
    pub distance: f32,
    /// Vertical field of view, degrees.
    pub fov_y_deg: f32,
    pub orthographic: bool,
    /// Bounding radius of the scene; drives clip planes and zoom limits.
    pub scene_radius: f32,
}

const MAX_PITCH: f32 = 89.0 * std::f32::consts::PI / 180.0;

impl Default for OrbitCamera {
    fn default() -> Self {
        Self {
            target: Vec3::ZERO,
            yaw: 0.6,
            pitch: 0.35,
            distance: 4.0,
            fov_y_deg: 40.0,
            orthographic: false,
            scene_radius: 1.0,
        }
    }
}

impl OrbitCamera {
    /// Unit vector from the target toward the camera.
    pub fn direction(&self) -> Vec3 {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        Vec3::new(cp * sy, sp, cp * cy)
    }

    /// Camera position used for the view matrix.
    pub fn eye(&self) -> Vec3 {
        // In orthographic mode `distance` only sets the zoom; keep the eye
        // far enough back that nothing gets clipped by the near plane.
        let d = if self.orthographic {
            self.distance + self.scene_radius * 4.0
        } else {
            self.distance
        };
        self.target + self.direction() * d
    }

    pub fn view(&self) -> Mat4 {
        Mat4::look_at_rh(self.eye(), self.target, Vec3::Y)
    }

    fn clip_planes(&self) -> (f32, f32) {
        let r = self.scene_radius.max(1e-3);
        let d = (self.eye() - self.target).length();
        let near = (d - r * 2.0).max(d * 0.005).max(r * 1e-3);
        let far = d + r * 4.0;
        (near, far)
    }

    /// Half the visible height at the target distance.
    fn half_height(&self) -> f32 {
        self.distance * (self.fov_y_deg.to_radians() * 0.5).tan()
    }

    pub fn projection(&self, aspect: f32) -> Mat4 {
        let (near, far) = self.clip_planes();
        if self.orthographic {
            let h = self.half_height();
            let w = h * aspect;
            Mat4::orthographic_rh(-w, w, -h, h, near, far)
        } else {
            Mat4::perspective_rh(self.fov_y_deg.to_radians(), aspect, near, far)
        }
    }

    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        self.projection(aspect) * self.view()
    }

    /// Rotates around the target. Deltas are in pixels.
    pub fn orbit(&mut self, dx: f32, dy: f32) {
        self.yaw -= dx * 0.008;
        self.pitch = (self.pitch + dy * 0.008).clamp(-MAX_PITCH, MAX_PITCH);
    }

    /// Moves the target in the view plane so the surface follows the cursor.
    pub fn pan(&mut self, dx: f32, dy: f32, viewport_height: f32) {
        let units_per_px = 2.0 * self.half_height() / viewport_height.max(1.0);
        let forward = -self.direction();
        let right = forward.cross(Vec3::Y).normalize_or(Vec3::X);
        let up = right.cross(forward);
        self.target += (-right * dx + up * dy) * units_per_px;
    }

    /// Multiplies the distance by `factor` (< 1 zooms in).
    pub fn zoom(&mut self, factor: f32) {
        let r = self.scene_radius.max(1e-3);
        self.distance = (self.distance * factor).clamp(r * 0.02, r * 200.0);
    }

    /// Centers the camera on `bounds` so the whole box is visible.
    pub fn frame(&mut self, bounds: &Aabb) {
        let r = bounds.radius().max(1e-3);
        self.target = bounds.center();
        self.scene_radius = r;
        let half_fov = (self.fov_y_deg.to_radians() * 0.5).min(1.5);
        self.distance = r / half_fov.sin() * 1.05;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit_box() -> Aabb {
        Aabb {
            min: Vec3::splat(-1.0),
            max: Vec3::splat(1.0),
        }
    }

    #[test]
    fn framed_box_corners_are_on_screen() {
        for ortho in [false, true] {
            let mut cam = OrbitCamera {
                orthographic: ortho,
                ..Default::default()
            };
            cam.frame(&unit_box());
            let vp = cam.view_proj(1.0);
            for x in [-1.0, 1.0] {
                for y in [-1.0, 1.0] {
                    for z in [-1.0, 1.0] {
                        let p = vp.project_point3(Vec3::new(x, y, z));
                        assert!(p.x.abs() <= 1.0 && p.y.abs() <= 1.0, "{p:?} ortho={ortho}");
                        assert!((0.0..=1.0).contains(&p.z), "depth {p:?} ortho={ortho}");
                    }
                }
            }
        }
    }

    #[test]
    fn pitch_is_clamped() {
        let mut cam = OrbitCamera::default();
        cam.orbit(0.0, 1e6);
        assert!(cam.pitch <= MAX_PITCH);
        cam.orbit(0.0, -1e6);
        assert!(cam.pitch >= -MAX_PITCH);
    }

    #[test]
    fn zoom_respects_limits() {
        let mut cam = OrbitCamera::default();
        cam.zoom(1e-9);
        assert!(cam.distance > 0.0);
        cam.zoom(1e9);
        assert!(cam.distance <= cam.scene_radius * 200.0);
    }

    #[test]
    fn pan_keeps_distance_and_moves_target() {
        let mut cam = OrbitCamera::default();
        cam.frame(&unit_box());
        let before = cam.eye().distance(cam.target);
        cam.pan(50.0, 20.0, 600.0);
        assert!((cam.eye().distance(cam.target) - before).abs() < 1e-4);
        assert!(cam.target.length() > 0.0);
    }
}
