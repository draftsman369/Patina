use patina_mesh::primitives::Primitive;
use patina_render::ViewMode;

use crate::app::{Action, Layout, NewProjectDialog, PatinaApp};

impl PatinaApp {
    pub(crate) fn menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("menu_bar").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| self.file_menu(ui));
                ui.menu_button("Edit", |ui| {
                    // Undo/redo arrive with the command system in M3.
                    ui.add_enabled(false, egui::Button::new("Undo").shortcut_text("Ctrl+Z"));
                    ui.add_enabled(false, egui::Button::new("Redo").shortcut_text("Ctrl+Y"));
                });
                ui.menu_button("Mode", |ui| {
                    for (layout, label, key) in [
                        (Layout::Split, "3D / 2D", "F1"),
                        (Layout::Only3d, "3D only", "F2"),
                        (Layout::Only2d, "2D only", "F3"),
                    ] {
                        if ui
                            .add(
                                egui::Button::selectable(self.layout == layout, label)
                                    .shortcut_text(key),
                            )
                            .clicked()
                        {
                            self.layout = layout;
                            ui.close();
                        }
                    }
                });
                ui.menu_button("Window", |ui| {
                    let p = &mut self.panels;
                    ui.checkbox(&mut p.texture_sets, "Texture Set List");
                    ui.checkbox(&mut p.layers, "Layers");
                    ui.checkbox(&mut p.properties, "Properties");
                    ui.checkbox(&mut p.display, "Display Settings");
                    ui.checkbox(&mut p.shelf, "Shelf");
                    ui.checkbox(&mut p.tools, "Tools");
                });
                ui.menu_button("Viewport", |ui| self.viewport_menu(ui));
                ui.menu_button("Help", |ui| {
                    if ui.button("About Patina").clicked() {
                        self.show_about = true;
                        ui.close();
                    }
                    ui.hyperlink_to(
                        "Roadmap",
                        "https://github.com/draftsman369/Patina/blob/main/ROADMAP.md",
                    );
                });
            });
        });
    }

    fn file_menu(&mut self, ui: &mut egui::Ui) {
        if ui
            .add(egui::Button::new("New project…").shortcut_text("Ctrl+N"))
            .clicked()
        {
            self.new_project = Some(NewProjectDialog::default());
            ui.close();
        }
        if ui
            .add(egui::Button::new("Open mesh…").shortcut_text("Ctrl+O"))
            .clicked()
        {
            self.actions.push(Action::OpenMeshDialog);
            ui.close();
        }
        ui.menu_button("New from primitive", |ui| {
            for p in Primitive::ALL {
                if ui.button(p.label()).clicked() {
                    self.actions.push(Action::NewPrimitive(p));
                    ui.close();
                }
            }
        });
        ui.add_enabled_ui(!self.recent.is_empty(), |ui| {
            ui.menu_button("Open recent", |ui| {
                for path in self.recent.clone() {
                    let label = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| path.display().to_string());
                    if ui
                        .button(label)
                        .on_hover_text(path.display().to_string())
                        .clicked()
                    {
                        self.actions.push(Action::OpenMesh(path));
                        ui.close();
                    }
                }
                ui.separator();
                if ui.button("Clear list").clicked() {
                    self.recent.clear();
                    ui.close();
                }
            });
        });
        if ui.button("Bake mesh maps…").clicked() {
            self.actions.push(Action::OpenBakeDialog);
            ui.close();
        }
        if ui.button("Load HDRI…").clicked() {
            self.actions.push(Action::PickHdri);
            ui.close();
        }
        ui.separator();
        // Project files (.patina) arrive in M3.
        ui.add_enabled(
            false,
            egui::Button::new("Save project").shortcut_text("Ctrl+S"),
        );
        ui.add_enabled(
            false,
            egui::Button::new("Export textures…").shortcut_text("Ctrl+Shift+E"),
        );
        ui.separator();
        if ui.button("Quit").clicked() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn viewport_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("View mode", |ui| {
            for mode in ViewMode::ALL {
                let shortcut = match mode {
                    ViewMode::Material => "M",
                    _ => "",
                };
                if ui
                    .add(
                        egui::Button::selectable(self.settings.view_mode == mode, mode.label())
                            .shortcut_text(shortcut),
                    )
                    .clicked()
                {
                    self.settings.view_mode = mode;
                    ui.close();
                }
            }
            ui.separator();
            ui.weak("C cycles channels");
        });
        ui.menu_button("Environment", |ui| {
            for env in self.environment_choices() {
                if ui
                    .selectable_label(self.settings.environment == env, env.name())
                    .clicked()
                {
                    self.settings.environment = env;
                    ui.close();
                }
            }
            ui.separator();
            if ui.button("Load HDRI…").clicked() {
                self.actions.push(Action::PickHdri);
                ui.close();
            }
        });
        ui.menu_button("Mesh map", |ui| {
            for map in patina_core::MeshMap::ALL {
                let selected =
                    self.settings.view_mode == ViewMode::MeshMap && self.settings.mesh_map == map;
                if ui.selectable_label(selected, map.label()).clicked() {
                    self.settings.view_mode = ViewMode::MeshMap;
                    self.settings.mesh_map = map;
                    ui.close();
                }
            }
            ui.separator();
            ui.weak("B cycles mesh maps");
        });
        ui.checkbox(&mut self.settings.wireframe, "Wireframe");
        ui.checkbox(&mut self.camera.orthographic, "Orthographic");
        ui.separator();
        if ui
            .add(egui::Button::new("Frame mesh").shortcut_text("F"))
            .clicked()
        {
            self.actions.push(Action::FrameCamera);
            ui.close();
        }
        ui.menu_button("Camera presets", |ui| {
            use std::f32::consts::FRAC_PI_2;
            for (label, yaw, pitch) in [
                ("Front", 0.0, 0.0),
                ("Back", std::f32::consts::PI, 0.0),
                ("Left", -FRAC_PI_2, 0.0),
                ("Right", FRAC_PI_2, 0.0),
                ("Top", 0.0, 89.0f32.to_radians()),
                ("Bottom", 0.0, -89.0f32.to_radians()),
                ("Three-quarter", 0.6, 0.35),
            ] {
                if ui.button(label).clicked() {
                    self.camera.yaw = yaw;
                    self.camera.pitch = pitch;
                    ui.close();
                }
            }
        });
    }
}
