use egui::{Color32, PointerButton, Rect, Sense, Vec2, pos2};
use glam::Vec2 as GVec2;
use patina_render::{UvTarget, ViewMode, ViewportTarget};

use crate::app::{Layout, PatinaApp};

const FULL_UV: Rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));

impl PatinaApp {
    pub(crate) fn central(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::no_frame().show(ui, |ui| {
            let rect = ui.max_rect();
            match self.layout {
                Layout::Only3d => self.viewport_3d(ui, rect),
                Layout::Only2d => self.viewport_2d(ui, rect),
                Layout::Split => {
                    let mid = rect.center().x;
                    let left = Rect::from_min_max(rect.min, pos2(mid - 1.0, rect.max.y));
                    let right = Rect::from_min_max(pos2(mid + 1.0, rect.min.y), rect.max);
                    self.viewport_3d(ui, left);
                    self.viewport_2d(ui, right);
                    ui.painter().rect_filled(
                        Rect::from_min_max(
                            pos2(mid - 1.0, rect.min.y),
                            pos2(mid + 1.0, rect.max.y),
                        ),
                        0.0,
                        Color32::from_gray(20),
                    );
                }
            }
        });
    }

    fn viewport_3d(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let response = ui.interact(
            rect,
            ui.scope_id().with("viewport_3d"),
            Sense::click_and_drag(),
        );
        let (mods, scroll) = ui.input(|i| (i.modifiers, i.smooth_scroll_delta.y));

        if response.hovered() && scroll != 0.0 {
            self.camera.zoom((-scroll * 0.0015).exp());
        }
        let d = response.drag_delta();
        if d != Vec2::ZERO {
            if response.dragged_by(PointerButton::Primary) && mods.alt {
                self.camera.orbit(d.x, d.y);
            } else if response.dragged_by(PointerButton::Middle) {
                self.camera.pan(d.x, d.y, rect.height());
            } else if response.dragged_by(PointerButton::Secondary) {
                if mods.shift {
                    self.settings.env_rotation =
                        wrap_angle(self.settings.env_rotation + d.x * 0.01);
                } else if mods.alt {
                    self.camera.zoom(((d.y - d.x) * 0.005).exp());
                } else {
                    // Fallback for mice and trackpads without comfortable Alt+LMB.
                    self.camera.orbit(d.x, d.y);
                }
            }
        }

        let painter = ui.painter_at(rect);
        let Some(gpu) = &mut self.gpu else {
            painter.rect_filled(rect, 0.0, Color32::from_gray(25));
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "No GPU available",
                egui::FontId::proportional(16.0),
                Color32::GRAY,
            );
            return;
        };

        let ppp = ui.ctx().pixels_per_point();
        let (w, h) = (
            (rect.width() * ppp).round() as u32,
            (rect.height() * ppp).round() as u32,
        );
        if w == 0 || h == 0 {
            return;
        }
        let needs_registration = match &mut gpu.viewport {
            Some((target, _)) => target.resize(&gpu.renderer, w, h),
            None => {
                gpu.viewport = Some((
                    ViewportTarget::new(&gpu.renderer, w, h),
                    egui::TextureId::default(),
                ));
                true
            }
        };
        if needs_registration {
            let (target, id) = gpu.viewport.as_ref().unwrap();
            let existing = (*id != egui::TextureId::default()).then_some(*id);
            let new_id = gpu.register_texture(target.output_view(), existing);
            gpu.viewport.as_mut().unwrap().1 = new_id;
        }
        let (target, id) = gpu.viewport.as_ref().unwrap();
        gpu.renderer
            .render_viewport(target, gpu.scene.as_ref(), &self.camera, &self.settings);
        painter.image(*id, rect, FULL_UV, Color32::WHITE);

        // Overlays.
        let font = egui::FontId::proportional(12.0);
        if self.settings.view_mode != ViewMode::Material {
            painter.text(
                rect.left_top() + egui::vec2(10.0, 8.0),
                egui::Align2::LEFT_TOP,
                format!("{} (M: material)", self.settings.view_mode.label()),
                egui::FontId::proportional(14.0),
                Color32::from_gray(220),
            );
        }
        if response.hovered() {
            painter.text(
                rect.left_bottom() + egui::vec2(10.0, -8.0),
                egui::Align2::LEFT_BOTTOM,
                "Alt+LMB / RMB orbit · MMB pan · wheel / Alt+RMB zoom · Shift+RMB rotate environment · F frame",
                font,
                Color32::from_white_alpha(110),
            );
        }
    }

    fn viewport_2d(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let response = ui.interact(
            rect,
            ui.scope_id().with("viewport_2d"),
            Sense::click_and_drag(),
        );
        let ppp = ui.ctx().pixels_per_point();
        let size_px = GVec2::new(rect.width(), rect.height()) * ppp;

        if self.uv_fit_pending && size_px.min_element() > 1.0 {
            self.uv_view.fit(size_px);
            self.uv_fit_pending = false;
        }
        let (mods, scroll) = ui.input(|i| (i.modifiers, i.smooth_scroll_delta.y));
        if let Some(hover) = response.hover_pos()
            && scroll != 0.0
        {
            let cursor = GVec2::new(hover.x - rect.min.x, hover.y - rect.min.y) * ppp;
            self.uv_view
                .zoom_at((scroll * 0.0015).exp(), cursor, size_px);
        }
        let d = response.drag_delta();
        let panning = response.dragged_by(PointerButton::Middle)
            || response.dragged_by(PointerButton::Secondary)
            || (response.dragged_by(PointerButton::Primary) && mods.alt);
        if d != Vec2::ZERO && panning {
            self.uv_view.pan(GVec2::new(d.x, d.y) * ppp);
        }

        let channel = self.uv_channel();
        let painter = ui.painter_at(rect);
        let set = self.doc.project.texture_sets.get(self.selected_set);
        let Some(gpu) = &mut self.gpu else {
            painter.rect_filled(rect, 0.0, Color32::from_gray(25));
            return;
        };
        let (w, h) = (size_px.x.round() as u32, size_px.y.round() as u32);
        if w == 0 || h == 0 {
            return;
        }
        let needs_registration = match &mut gpu.uv {
            Some((target, _)) => target.resize(&gpu.renderer, w, h),
            None => {
                gpu.uv = Some((
                    UvTarget::new(&gpu.renderer, w, h),
                    egui::TextureId::default(),
                ));
                true
            }
        };
        if needs_registration {
            let (target, id) = gpu.uv.as_ref().unwrap();
            let existing = (*id != egui::TextureId::default()).then_some(*id);
            let new_id = gpu.register_texture(target.output_view(), existing);
            gpu.uv.as_mut().unwrap().1 = new_id;
        }
        let (target, id) = gpu.uv.as_ref().unwrap();
        if let (Some(scene), Some(set)) = (&gpu.scene, set) {
            gpu.renderer
                .render_uv(target, scene, set.material_index, &self.uv_view, channel);
        }
        painter.image(*id, rect, FULL_UV, Color32::WHITE);

        let label = match set {
            Some(set) => format!("UV · {} · {}", set.name, channel.label()),
            None => "UV".to_owned(),
        };
        painter.text(
            rect.left_top() + egui::vec2(10.0, 8.0),
            egui::Align2::LEFT_TOP,
            label,
            egui::FontId::proportional(14.0),
            Color32::from_gray(220),
        );
        if !self.doc.mesh.has_uvs {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "This mesh has no UVs",
                egui::FontId::proportional(16.0),
                ui.visuals().error_fg_color,
            );
        }
    }
}

fn wrap_angle(a: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    (a + PI).rem_euclid(TAU) - PI
}

#[cfg(test)]
mod tests {
    use super::wrap_angle;

    #[test]
    fn angles_wrap_to_pi() {
        use std::f32::consts::PI;
        assert!(
            (wrap_angle(3.0 * PI) - PI).abs() < 1e-4 || (wrap_angle(3.0 * PI) + PI).abs() < 1e-4
        );
        assert!((wrap_angle(0.5) - 0.5).abs() < 1e-6);
        assert!((wrap_angle(-0.5 - 2.0 * PI) + 0.5).abs() < 1e-4);
    }
}
