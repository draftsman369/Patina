use egui::{Color32, RichText};
use patina_core::NormalFormat;
use patina_mesh::primitives::Primitive;

use crate::app::{Action, PatinaApp};
use crate::theme;

impl PatinaApp {
    pub(crate) fn dialogs(&mut self, ctx: &egui::Context) {
        self.new_project_dialog(ctx);
        if self.show_about {
            egui::Window::new("About Patina")
                .collapsible(false)
                .resizable(false)
                .open(&mut self.show_about)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.heading(RichText::new("Patina").color(theme::ACCENT));
                    ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                    ui.label("Open-source, local-first 3D texture painting.");
                    ui.label("Free software under the GNU GPL v3 or later.");
                });
        }
    }

    fn new_project_dialog(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &mut self.new_project else {
            return;
        };
        let mut create = false;
        let mut cancel = false;
        let modal = egui::Modal::new(egui::Id::unique("new_project")).show(ctx, |ui| {
            ui.set_width(420.0);
            ui.heading("New project");
            ui.add_space(8.0);
            egui::Grid::new("new_project_grid")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    ui.label("Mesh");
                    ui.horizontal(|ui| {
                        let text = match &dialog.mesh_path {
                            Some(p) => p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                            None => "(primitive)".to_owned(),
                        };
                        ui.label(text);
                        if ui.button("Select…").clicked()
                            && let Some(path) = rfd::FileDialog::new()
                                .add_filter("Meshes", patina_mesh::SUPPORTED_EXTENSIONS)
                                .pick_file()
                        {
                            dialog.mesh_path = Some(path);
                        }
                        if dialog.mesh_path.is_some() && ui.small_button("×").clicked() {
                            dialog.mesh_path = None;
                        }
                    });
                    ui.end_row();

                    ui.label("Primitive");
                    ui.add_enabled_ui(dialog.mesh_path.is_none(), |ui| {
                        egui::ComboBox::from_id_salt("np_primitive")
                            .selected_text(dialog.primitive.label())
                            .show_ui(ui, |ui| {
                                for p in Primitive::ALL {
                                    ui.selectable_value(&mut dialog.primitive, p, p.label());
                                }
                            });
                    });
                    ui.end_row();

                    ui.label("Document resolution");
                    egui::ComboBox::from_id_salt("np_resolution")
                        .selected_text(format!("{0} × {0}", dialog.resolution))
                        .show_ui(ui, |ui| {
                            for r in [512, 1024, 2048, 4096, 8192] {
                                ui.selectable_value(&mut dialog.resolution, r, format!("{r} × {r}"));
                            }
                        });
                    ui.end_row();

                    ui.label("Normal map format");
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut dialog.normal_format, NormalFormat::OpenGl, "OpenGL");
                        ui.selectable_value(&mut dialog.normal_format, NormalFormat::DirectX, "DirectX");
                    });
                    ui.end_row();
                });
            ui.add_space(4.0);
            ui.label(
                RichText::new("Each mesh material becomes a texture set. Templates and UDIM options come later.")
                    .small()
                    .color(Color32::GRAY),
            );
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button(RichText::new("Create").strong()).clicked() {
                    create = true;
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
        });
        if create {
            self.actions.push(Action::CreateProject);
        } else if cancel || modal.should_close() {
            self.new_project = None;
        }
    }

    /// Highlights the window while files are dragged over it.
    pub(crate) fn drop_overlay(&self, ctx: &egui::Context) {
        let hovering = ctx.input(|i| !i.raw.hovered_files.is_empty());
        if !hovering {
            return;
        }
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::unique("drop_overlay"),
        ));
        let rect = ctx.content_rect();
        painter.rect_filled(rect, 0.0, Color32::from_black_alpha(150));
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Drop a mesh to open it, or an image to load it into the current channel",
            egui::FontId::proportional(20.0),
            Color32::WHITE,
        );
    }
}
