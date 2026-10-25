//! The Bake Mesh Maps dialog.

use std::path::PathBuf;
use std::sync::Arc;

use egui::RichText;
use patina_bake::{BakeSettings, IdSource, MatchMode};
use patina_core::MeshMap;
use patina_render::ViewMode;

use crate::app::PatinaApp;
use crate::bake_job::{BakeJob, BakeMessage, BakeRequest};
use crate::theme;

const SIZES: [u32; 7] = [256, 512, 1024, 2048, 4096, 8192, 128];

pub struct BakeDialog {
    pub open: bool,
    pub high_path: Option<PathBuf>,
    pub cage_path: Option<PathBuf>,
    pub settings: BakeSettings,
    /// Which texture sets to bake, by index.
    pub sets: Vec<bool>,
    pub job: Option<BakeJob>,
    pub log: Vec<String>,
}

impl BakeDialog {
    fn new(set_count: usize, resolution: u32) -> Self {
        Self {
            open: true,
            high_path: None,
            cage_path: None,
            settings: BakeSettings {
                resolution,
                ..Default::default()
            },
            sets: vec![true; set_count],
            job: None,
            log: Vec::new(),
        }
    }
}

fn file_name(path: &Option<PathBuf>, empty: &str) -> String {
    path.as_ref()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| empty.to_owned())
}

fn pick_mesh(title: &str) -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title(title)
        .add_filter("Meshes", patina_mesh::SUPPORTED_EXTENSIONS)
        .pick_file()
}

impl PatinaApp {
    pub(crate) fn open_bake_dialog(&mut self) {
        let sets = self.doc.project.texture_sets.len();
        let resolution = self
            .doc
            .project
            .texture_sets
            .get(self.selected_set)
            .map(|s| s.resolution)
            .unwrap_or(2048);
        match &mut self.bake_dialog {
            Some(d) => {
                d.open = true;
                d.sets.resize(sets, true);
            }
            None => self.bake_dialog = Some(BakeDialog::new(sets, resolution)),
        }
    }

    /// Applies finished bakes. Called every frame.
    pub(crate) fn poll_bake(&mut self, ctx: &egui::Context) {
        let Some(job) = self.bake_dialog.as_ref().and_then(|d| d.job.as_ref()) else {
            return;
        };
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
        let mut finished = None;
        let mut baked = Vec::new();
        let mut log = Vec::new();
        while let Ok(msg) = job.rx.try_recv() {
            match msg {
                BakeMessage::Baked { set, maps } => baked.push((set, maps)),
                BakeMessage::Failed(e) => log.push(format!("Error: {e}")),
                BakeMessage::Finished { cancelled } => finished = Some(cancelled),
            }
        }
        for (set, maps) in baked {
            let Some(ts) = self.doc.project.texture_sets.get_mut(set) else {
                continue;
            };
            let names: Vec<&str> = maps.iter().map(|(m, _)| m.label()).collect();
            log.push(format!("{}: baked {}", ts.name, names.join(", ")));
            let first = maps.first().map(|(m, _)| *m);
            for (map, image) in maps {
                ts.set_mesh_map(map, Some(image));
            }
            self.rebuild_material(set);
            if let Some(m) = first {
                self.settings.view_mode = ViewMode::MeshMap;
                if !maps_contains_current(
                    &self.doc.project.texture_sets[set],
                    self.settings.mesh_map,
                ) {
                    self.settings.mesh_map = m;
                }
            }
        }
        let dialog = self.bake_dialog.as_mut().expect("dialog exists");
        dialog.log.extend(log);
        if let Some(cancelled) = finished {
            dialog.job = None;
            let msg = if cancelled {
                "Bake cancelled"
            } else {
                "Bake finished"
            };
            dialog.log.push(msg.to_owned());
            self.set_status(msg);
        }
    }

    pub(crate) fn bake_window(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &mut self.bake_dialog else {
            return;
        };
        if !dialog.open {
            return;
        }
        let set_names: Vec<String> = self
            .doc
            .project
            .texture_sets
            .iter()
            .map(|s| s.name.clone())
            .collect();
        let mut open = dialog.open;
        let mut start = false;
        egui::Window::new("Bake Mesh Maps")
            .open(&mut open)
            .default_width(460.0)
            .resizable(true)
            .show(ctx, |ui| {
                let busy = dialog.job.is_some();
                ui.add_enabled_ui(!busy, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(520.0)
                        .show(ui, |ui| {
                            settings_ui(ui, dialog, &set_names);
                        });
                });
                ui.separator();
                match &dialog.job {
                    Some(job) => {
                        let (p, text) = job.progress();
                        ui.add(
                            egui::ProgressBar::new(p).text(format!("{text} — {:.0}%", p * 100.0)),
                        );
                        if ui.button("Cancel").clicked() {
                            job.cancel();
                        }
                    }
                    None => {
                        let any_set = dialog.sets.iter().any(|&b| b);
                        let any_map = !dialog.settings.maps.is_empty();
                        if ui
                            .add_enabled(
                                any_set && any_map,
                                egui::Button::new(
                                    RichText::new("Bake selected texture sets").strong(),
                                ),
                            )
                            .clicked()
                        {
                            start = true;
                        }
                    }
                }
                if !dialog.log.is_empty() {
                    ui.add_space(4.0);
                    egui::ScrollArea::vertical()
                        .id_salt("bake_log")
                        .max_height(90.0)
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            for line in &dialog.log {
                                let color = if line.starts_with("Error") {
                                    ui.visuals().error_fg_color
                                } else {
                                    ui.visuals().weak_text_color()
                                };
                                ui.label(RichText::new(line).small().color(color));
                            }
                        });
                }
            });
        dialog.open = open;
        if start {
            self.start_bake();
        }
    }

    fn start_bake(&mut self) {
        let Some(gpu) = &self.gpu else {
            self.set_error("Baking needs a GPU");
            return;
        };
        let Some(dialog) = &mut self.bake_dialog else {
            return;
        };
        if !self.doc.mesh.has_uvs {
            dialog.log.push("Error: the mesh has no UVs".into());
            return;
        }
        let mut settings = dialog.settings.clone();
        settings.normal_format = self.doc.project.normal_format;
        let sets = self
            .doc
            .project
            .texture_sets
            .iter()
            .enumerate()
            .filter(|(i, _)| dialog.sets.get(*i).copied().unwrap_or(false))
            .map(|(i, s)| (i, s.material_index, s.name.clone()))
            .collect();
        let request = BakeRequest {
            low: Arc::new(self.doc.mesh.clone()),
            high_path: dialog.high_path.clone(),
            cage_path: dialog.cage_path.clone(),
            sets,
            settings,
        };
        dialog.log.push("Bake started".into());
        dialog.job = Some(BakeJob::start(
            gpu.render_state.device.clone(),
            gpu.render_state.queue.clone(),
            request,
        ));
    }
}

fn maps_contains_current(set: &patina_core::TextureSet, map: MeshMap) -> bool {
    set.mesh_map(map).is_some()
}

fn settings_ui(ui: &mut egui::Ui, d: &mut BakeDialog, set_names: &[String]) {
    let s = &mut d.settings;
    ui.label(RichText::new("Common").strong().color(theme::ACCENT));
    egui::Grid::new("bake_common")
        .num_columns(2)
        .spacing([10.0, 6.0])
        .show(ui, |ui| {
            ui.label("Output size");
            egui::ComboBox::from_id_salt("bake_size")
                .selected_text(format!("{0} × {0}", s.resolution))
                .show_ui(ui, |ui| {
                    let mut sizes = SIZES;
                    sizes.sort();
                    for r in sizes {
                        ui.selectable_value(&mut s.resolution, r, format!("{r} × {r}"));
                    }
                });
            ui.end_row();

            ui.label("Antialiasing");
            egui::ComboBox::from_id_salt("bake_ss")
                .selected_text(match s.supersampling {
                    1 => "None".to_owned(),
                    n => format!("Supersampling {n}×{n}"),
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut s.supersampling, 1, "None");
                    for n in [2, 4, 8] {
                        ui.selectable_value(
                            &mut s.supersampling,
                            n,
                            format!("Supersampling {n}×{n}"),
                        );
                    }
                });
            ui.end_row();

            ui.label("Dilation width");
            ui.add(egui::Slider::new(&mut s.dilation, 0..=64).suffix(" px"));
            ui.end_row();

            ui.label("High poly mesh");
            ui.horizontal(|ui| {
                ui.label(file_name(&d.high_path, "(low poly itself)"));
                if ui.small_button("Select…").clicked()
                    && let Some(p) = pick_mesh("High poly mesh")
                {
                    d.high_path = Some(p);
                }
                if d.high_path.is_some()
                    && ui
                        .small_button("×")
                        .on_hover_text("Bake from the low poly")
                        .clicked()
                {
                    d.high_path = None;
                }
            });
            ui.end_row();

            ui.label("Cage");
            ui.horizontal(|ui| {
                ui.label(file_name(&d.cage_path, "(none)"));
                if ui.small_button("Select…").clicked()
                    && let Some(p) = pick_mesh("Cage mesh (same triangles as the low poly)")
                {
                    d.cage_path = Some(p);
                }
                if d.cage_path.is_some() && ui.small_button("×").clicked() {
                    d.cage_path = None;
                }
            });
            ui.end_row();

            ui.label("Max frontal distance");
            ui.add(egui::Slider::new(&mut s.max_frontal, 0.0..=1.0).logarithmic(true))
                .on_hover_text("Relative to the mesh's bounding radius");
            ui.end_row();

            ui.label("Max rear distance");
            ui.add(egui::Slider::new(&mut s.max_rear, 0.0..=1.0).logarithmic(true));
            ui.end_row();

            ui.label("Average normals");
            ui.checkbox(&mut s.average_normals, "")
                .on_hover_text("Cast rays along smoothed normals so hard edges don't leave gaps");
            ui.end_row();

            ui.label("Match");
            ui.horizontal(|ui| {
                ui.selectable_value(&mut s.match_mode, MatchMode::Always, "Always");
                ui.selectable_value(&mut s.match_mode, MatchMode::ByName, "By mesh name")
                    .on_hover_text("Pairs objects named like Bolt_low / Bolt_high");
            });
            ui.end_row();
        });

    ui.add_space(6.0);
    ui.label(RichText::new("Texture sets").strong().color(theme::ACCENT));
    for (i, name) in set_names.iter().enumerate() {
        if let Some(b) = d.sets.get_mut(i) {
            ui.checkbox(b, name);
        }
    }

    ui.add_space(6.0);
    ui.label(RichText::new("Maps").strong().color(theme::ACCENT));
    for map in MeshMap::ALL {
        let mut on = s.maps.contains(&map);
        if ui.checkbox(&mut on, map.label()).changed() {
            if on {
                s.maps.push(map);
            } else {
                s.maps.retain(|m| *m != map);
            }
        }
        if !on {
            continue;
        }
        ui.indent(map.key(), |ui| match map {
            MeshMap::AmbientOcclusion | MeshMap::BentNormal => {
                ui.horizontal(|ui| {
                    ui.label("Samples");
                    ui.add(egui::Slider::new(&mut s.ao_samples, 8..=512).logarithmic(true));
                });
                ui.horizontal(|ui| {
                    ui.label("Distance");
                    ui.add(egui::Slider::new(&mut s.ao_distance, 0.01..=2.0).logarithmic(true));
                });
                ui.checkbox(&mut s.ao_ignore_backfaces, "Ignore backfaces");
                ui.add_enabled(
                    s.match_mode == MatchMode::ByName,
                    egui::Checkbox::new(&mut s.ao_only_same_name, "Only same mesh name"),
                )
                .on_disabled_hover_text("Needs Match: By mesh name");
            }
            MeshMap::Thickness => {
                ui.horizontal(|ui| {
                    ui.label("Samples");
                    ui.add(egui::Slider::new(&mut s.thickness_samples, 8..=256).logarithmic(true));
                });
                ui.horizontal(|ui| {
                    ui.label("Distance");
                    ui.add(
                        egui::Slider::new(&mut s.thickness_distance, 0.01..=2.0).logarithmic(true),
                    );
                });
            }
            MeshMap::Curvature => {
                ui.horizontal(|ui| {
                    ui.label("Strength");
                    ui.add(
                        egui::Slider::new(&mut s.curvature_strength, 0.1..=10.0).logarithmic(true),
                    );
                });
            }
            MeshMap::Id => {
                ui.horizontal(|ui| {
                    ui.label("Color source");
                    for src in IdSource::ALL {
                        ui.selectable_value(&mut s.id_source, src, src.label());
                    }
                });
            }
            _ => {}
        });
    }
}
