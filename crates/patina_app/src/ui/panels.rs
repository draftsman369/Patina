use std::time::Duration;

use egui::{Color32, RichText};
use patina_core::MeshMap;
use patina_core::{Channel, NormalFormat};
use patina_render::{ENV_PRESETS, EnvSource, Tonemap, ViewMode};

use crate::app::{Action, PatinaApp, ShelfTab};
use crate::theme;
use crate::ui::icons::{self, Tool};

const RESOLUTIONS: [u32; 7] = [128, 256, 512, 1024, 2048, 4096, 8192];

impl PatinaApp {
    pub(crate) fn tools_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::left("tools")
            .exact_size(40.0)
            .resizable(false)
            .show(ui, |ui| {
                ui.add_space(6.0);
                ui.vertical_centered(|ui| {
                    for tool in Tool::ALL {
                        icons::tool_button(ui, tool);
                    }
                });
            });
    }

    pub(crate) fn status_bar(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame) {
        egui::Panel::bottom("status_bar")
            .exact_size(24.0)
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    let mesh = &self.doc.mesh;
                    ui.label(RichText::new(&self.doc.project.name).strong());
                    ui.separator();
                    ui.label(format!(
                        "{} verts · {} tris",
                        mesh.vertices.len(),
                        mesh.triangle_count()
                    ));
                    ui.separator();
                    ui.label(format!(
                        "{} texture set(s)",
                        self.doc.project.texture_sets.len()
                    ));
                    if let Some(status) = &self.status {
                        // Messages fade out after a while; errors stay longer.
                        let ttl = if status.is_error { 20 } else { 8 };
                        if status.at.elapsed() < Duration::from_secs(ttl) {
                            ui.separator();
                            let color = if status.is_error {
                                ui.visuals().error_fg_color
                            } else {
                                theme::ACCENT
                            };
                            ui.label(RichText::new(&status.text).color(color));
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if let Some(gpu) = &self.gpu {
                            let info = gpu.render_state.adapter.get_info();
                            ui.weak(format!("{} · {:?}", info.name, info.backend));
                        }
                        if let Some(cpu) = frame.info().cpu_usage {
                            ui.separator();
                            ui.weak(format!("UI {:.1} ms", cpu * 1000.0));
                        }
                    });
                });
            });
    }

    pub(crate) fn shelf(&mut self, ui: &mut egui::Ui) {
        egui::Panel::bottom("shelf")
            .resizable(true)
            .default_size(150.0)
            .size_range(80.0..=400.0)
            .show(ui, |ui| {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Shelf").strong());
                    ui.separator();
                    for tab in ShelfTab::ALL {
                        ui.selectable_value(&mut self.shelf_tab, tab, tab.label());
                    }
                });
                ui.separator();
                egui::ScrollArea::horizontal().show(ui, |ui| {
                    ui.horizontal(|ui| match self.shelf_tab {
                        ShelfTab::Environments => {
                            for env in self.environment_choices() {
                                let selected = self.settings.environment == env;
                                if environment_tile(ui, &env, selected).clicked() {
                                    self.settings.environment = env;
                                }
                            }
                            if ui
                                .add_sized([96.0, 62.0], egui::Button::new("Load HDRI…"))
                                .on_hover_text(
                                    "Radiance .hdr or OpenEXR .exr (or drop one on the window)",
                                )
                                .clicked()
                            {
                                self.actions.push(Action::PickHdri);
                            }
                        }
                        other => {
                            ui.weak(format!(
                                "{} will be listed here once the asset library lands (M6).",
                                other.label()
                            ));
                        }
                    });
                });
            });
    }

    pub(crate) fn side_panel(&mut self, ui: &mut egui::Ui) {
        let p = &self.panels;
        if !(p.texture_sets || p.layers || p.properties || p.display) {
            return;
        }
        egui::Panel::right("side")
            .resizable(true)
            .default_size(330.0)
            .size_range(260.0..=520.0)
            .show(ui, |ui| {
                egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                    if self.panels.texture_sets {
                        section(ui, "Texture Set List", |ui| self.texture_set_list(ui));
                    }
                    if self.panels.layers {
                        section(ui, "Layers", |ui| {
                            ui.weak("The layer stack (paint, fill and folder layers) arrives in M3.");
                        });
                    }
                    if self.panels.properties {
                        section(ui, "Properties", |ui| self.properties(ui));
                    }
                    if self.panels.display {
                        section(ui, "Display Settings", |ui| self.display_settings(ui));
                    }
                });
            });
    }

    fn texture_set_list(&mut self, ui: &mut egui::Ui) {
        for (i, set) in self.doc.project.texture_sets.iter().enumerate() {
            let selected = self.selected_set == i;
            let text = format!("{}   ({} px)", set.name, set.resolution);
            if ui
                .add_sized(
                    [ui.available_width(), 22.0],
                    egui::Button::selectable(selected, text),
                )
                .clicked()
            {
                self.selected_set = i;
            }
        }
    }

    fn properties(&mut self, ui: &mut egui::Ui) {
        let set_index = self.selected_set;
        let Some(set) = self.doc.project.texture_sets.get_mut(set_index) else {
            ui.weak("No texture set selected.");
            return;
        };
        let mut factors_changed = false;
        let mut actions = Vec::new();

        egui::Grid::new("texture_set_props")
            .num_columns(2)
            .spacing([10.0, 6.0])
            .show(ui, |ui| {
                ui.label("Name");
                ui.text_edit_singleline(&mut set.name);
                ui.end_row();

                ui.label("Resolution");
                egui::ComboBox::from_id_salt("resolution")
                    .selected_text(format!("{0} × {0}", set.resolution))
                    .show_ui(ui, |ui| {
                        for r in RESOLUTIONS {
                            ui.selectable_value(&mut set.resolution, r, format!("{r} × {r}"));
                        }
                    });
                ui.end_row();

                ui.label("Base color");
                let mut rgb = [
                    set.base_color_factor[0],
                    set.base_color_factor[1],
                    set.base_color_factor[2],
                ];
                if ui.color_edit_button_rgb(&mut rgb).changed() {
                    set.base_color_factor[..3].copy_from_slice(&rgb);
                    factors_changed = true;
                }
                ui.end_row();

                ui.label("Metallic");
                factors_changed |= ui
                    .add(egui::Slider::new(&mut set.metallic_factor, 0.0..=1.0))
                    .changed();
                ui.end_row();

                ui.label("Roughness");
                factors_changed |= ui
                    .add(egui::Slider::new(&mut set.roughness_factor, 0.0..=1.0))
                    .changed();
                ui.end_row();

                ui.label("Emissive");
                factors_changed |= ui.color_edit_button_rgb(&mut set.emissive_factor).changed();
                ui.end_row();
            });

        ui.add_space(6.0);
        ui.label(RichText::new("Channel bitmaps").strong());
        egui::Grid::new("channel_slots")
            .num_columns(3)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                for channel in Channel::ALL {
                    ui.label(channel.label());
                    match set.image(channel) {
                        Some(img) => ui.label(
                            RichText::new(format!("{}×{}", img.width, img.height))
                                .color(theme::ACCENT),
                        ),
                        None => ui.weak("factor only"),
                    };
                    ui.horizontal(|ui| {
                        if ui.small_button("Load…").clicked() {
                            actions.push(Action::PickChannelImage {
                                set: set_index,
                                channel,
                            });
                        }
                        if ui
                            .add_enabled(
                                set.image(channel).is_some(),
                                egui::Button::new("×").small(),
                            )
                            .on_hover_text("Clear bitmap")
                            .clicked()
                        {
                            actions.push(Action::ClearChannel {
                                set: set_index,
                                channel,
                            });
                        }
                    });
                    ui.end_row();
                }
            });
        ui.weak(
            "Tip: drop an image on the window to load it into the channel shown by the view mode.",
        );

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Mesh maps").strong());
            if ui.small_button("Bake…").clicked() {
                actions.push(Action::OpenBakeDialog);
            }
        });
        egui::Grid::new("mesh_map_slots")
            .num_columns(3)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                for map in MeshMap::ALL {
                    if ui
                        .selectable_label(
                            self.settings.view_mode == ViewMode::MeshMap
                                && self.settings.mesh_map == map,
                            map.label(),
                        )
                        .on_hover_text("Show this map in the viewport")
                        .clicked()
                    {
                        self.settings.view_mode = ViewMode::MeshMap;
                        self.settings.mesh_map = map;
                    }
                    match set.mesh_map(map) {
                        Some(img) => ui.label(
                            RichText::new(format!("{}×{}", img.width, img.height))
                                .color(theme::ACCENT),
                        ),
                        None => ui.weak("empty"),
                    };
                    ui.horizontal(|ui| {
                        if ui.small_button("Import…").clicked() {
                            actions.push(Action::PickMeshMap {
                                set: set_index,
                                map,
                            });
                        }
                        if ui
                            .add_enabled(
                                set.mesh_map(map).is_some(),
                                egui::Button::new("×").small(),
                            )
                            .on_hover_text("Clear map")
                            .clicked()
                        {
                            actions.push(Action::ClearMeshMap {
                                set: set_index,
                                map,
                            });
                        }
                    });
                    ui.end_row();
                }
            });

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label("Normal map format");
            let nf = &mut self.doc.project.normal_format;
            let before = *nf;
            ui.selectable_value(nf, NormalFormat::OpenGl, "OpenGL");
            ui.selectable_value(nf, NormalFormat::DirectX, "DirectX");
            if *nf != before {
                actions.push(Action::NormalFormatChanged);
            }
        });

        if factors_changed {
            actions.push(Action::FactorsChanged(set_index));
        }
        self.actions.extend(actions);
    }

    fn display_settings(&mut self, ui: &mut egui::Ui) {
        let choices = self.environment_choices();
        let s = &mut self.settings;
        egui::Grid::new("display_settings")
            .num_columns(2)
            .spacing([10.0, 6.0])
            .show(ui, |ui| {
                ui.label("View mode");
                egui::ComboBox::from_id_salt("view_mode")
                    .selected_text(s.view_mode.label())
                    .show_ui(ui, |ui| {
                        for m in ViewMode::ALL {
                            ui.selectable_value(&mut s.view_mode, m, m.label());
                        }
                    });
                ui.end_row();

                if s.view_mode == ViewMode::MeshMap {
                    ui.label("Mesh map");
                    egui::ComboBox::from_id_salt("mesh_map")
                        .selected_text(s.mesh_map.label())
                        .show_ui(ui, |ui| {
                            for m in MeshMap::ALL {
                                ui.selectable_value(&mut s.mesh_map, m, m.label());
                            }
                        });
                    ui.end_row();
                }

                ui.label("Environment");
                egui::ComboBox::from_id_salt("env")
                    .selected_text(s.environment.name().to_owned())
                    .show_ui(ui, |ui| {
                        for env in choices {
                            let name = env.name().to_owned();
                            ui.selectable_value(&mut s.environment, env, name);
                        }
                    });
                ui.end_row();

                ui.label("Rotation");
                let mut deg = s.env_rotation.to_degrees();
                if ui
                    .add(egui::Slider::new(&mut deg, -180.0..=180.0).suffix("°"))
                    .on_hover_text("Shift + right-drag in the viewport")
                    .changed()
                {
                    s.env_rotation = deg.to_radians();
                }
                ui.end_row();

                ui.label("Exposure");
                ui.add(egui::Slider::new(&mut s.exposure_ev, -5.0..=5.0).suffix(" EV"));
                ui.end_row();

                ui.label("Background blur");
                ui.add(egui::Slider::new(&mut s.background_blur, 0.0..=1.0));
                ui.end_row();

                ui.label("Background opacity");
                ui.add(egui::Slider::new(&mut s.background_opacity, 0.0..=1.0));
                ui.end_row();

                ui.label("Tonemapping");
                ui.horizontal(|ui| {
                    for t in Tonemap::ALL {
                        ui.selectable_value(&mut s.tonemap, t, t.label());
                    }
                });
                ui.end_row();

                ui.label("Wireframe");
                ui.checkbox(&mut s.wireframe, "");
                ui.end_row();

                ui.label("Field of view");
                ui.add(egui::Slider::new(&mut self.camera.fov_y_deg, 10.0..=100.0).suffix("°"));
                ui.end_row();

                ui.label("Orthographic");
                ui.checkbox(&mut self.camera.orthographic, "");
                ui.end_row();
            });
    }
}

fn section(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui)) {
    egui::CollapsingHeader::new(RichText::new(title).strong())
        .default_open(true)
        .show(ui, add);
    ui.add_space(2.0);
}

/// Average colors of the top, middle and bottom thirds of an equirect HDRI,
/// from a sparse grid of samples (cheap enough to run every frame).
fn hdri_bands(img: &patina_core::HdrImage) -> [[f32; 3]; 3] {
    let mut bands = [[0.0f32; 3]; 3];
    for (b, band) in bands.iter_mut().enumerate() {
        let mut n = 0.0;
        for sy in 0..4 {
            let y = ((b * 4 + sy) as u32 * img.height / 12).min(img.height - 1);
            for sx in 0..16 {
                let x = (sx * img.width / 16).min(img.width - 1);
                let p = img.data[(y * img.width + x) as usize];
                for c in 0..3 {
                    band[c] += p[c].min(16.0);
                }
                n += 1.0;
            }
        }
        for c in band.iter_mut() {
            *c /= n;
        }
    }
    bands
}

/// A swatch previewing an environment's gradient (and key light for presets).
fn environment_tile(ui: &mut egui::Ui, env: &EnvSource, selected: bool) -> egui::Response {
    match env {
        EnvSource::Preset(i) => {
            preset_tile(ui, &ENV_PRESETS[(*i).min(ENV_PRESETS.len() - 1)], selected)
        }
        EnvSource::Hdri { image, name } => {
            let bands = hdri_bands(image);
            swatch_tile(ui, name, &bands, None, selected)
        }
    }
}

fn preset_tile(
    ui: &mut egui::Ui,
    preset: &patina_render::EnvPreset,
    selected: bool,
) -> egui::Response {
    let bands = [preset.sky_top, preset.sky_horizon, preset.sky_ground];
    let key = preset.lights.first().map(|l| {
        let c = l.color;
        let peak = c[0].max(c[1]).max(c[2]).max(1e-3);
        [c[0] / peak * 4.0, c[1] / peak * 4.0, c[2] / peak * 4.0]
    });
    swatch_tile(ui, preset.name, &bands, key, selected)
}

fn swatch_tile(
    ui: &mut egui::Ui,
    name: &str,
    bands: &[[f32; 3]; 3],
    key: Option<[f32; 3]>,
    selected: bool,
) -> egui::Response {
    let size = egui::vec2(96.0, 84.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let painter = ui.painter_at(rect);
    let swatch = egui::Rect::from_min_size(rect.min, egui::vec2(size.x, 62.0));
    let to_color = |c: [f32; 3]| {
        // Rough display mapping (Reinhard) just for the swatch.
        let m = |v: f32| v / (1.0 + v);
        egui::Color32::from(egui::Rgba::from_rgb(m(c[0]), m(c[1]), m(c[2])))
    };
    let h = swatch.height() / bands.len() as f32;
    for (i, c) in bands.iter().enumerate() {
        let band = egui::Rect::from_min_size(
            swatch.min + egui::vec2(0.0, h * i as f32),
            egui::vec2(size.x, h),
        );
        painter.rect_filled(band, 0.0, to_color(*c));
    }
    if let Some(key) = key {
        painter.circle_filled(swatch.center(), 7.0, to_color(key));
    }
    let stroke = if selected {
        egui::Stroke::new(2.0, theme::ACCENT)
    } else if response.hovered() {
        egui::Stroke::new(1.0, Color32::from_gray(160))
    } else {
        egui::Stroke::new(1.0, Color32::from_gray(70))
    };
    painter.rect_stroke(swatch, 3.0, stroke, egui::StrokeKind::Inside);
    painter.text(
        egui::pos2(rect.center().x, rect.max.y - 10.0),
        egui::Align2::CENTER_CENTER,
        name,
        egui::FontId::proportional(12.0),
        ui.visuals().text_color(),
    );
    response.on_hover_text(format!("Environment: {name}"))
}
