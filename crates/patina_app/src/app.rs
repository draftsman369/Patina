//! Editor state and the per-frame update.

use std::path::PathBuf;
use std::time::Instant;

use eframe::egui_wgpu;
use egui::TextureId;
use patina_core::{Channel, Image, NormalFormat};
use patina_mesh::primitives::Primitive;
use patina_render::{
    OrbitCamera, Renderer, Scene, UvTarget, UvView, ViewMode, ViewportSettings, ViewportTarget,
};

use crate::document::{self, Document};
use crate::theme;

const RECENT_KEY: &str = "patina.recent_files";
const MAX_RECENT: usize = 10;

/// Viewport arrangement, switched with F1/F2/F3.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    /// 3D and 2D side by side.
    Split,
    Only3d,
    Only2d,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShelfTab {
    Materials,
    SmartMaterials,
    SmartMasks,
    Brushes,
    Alphas,
    Textures,
    Environments,
}

impl ShelfTab {
    pub const ALL: [ShelfTab; 7] = [
        ShelfTab::Materials,
        ShelfTab::SmartMaterials,
        ShelfTab::SmartMasks,
        ShelfTab::Brushes,
        ShelfTab::Alphas,
        ShelfTab::Textures,
        ShelfTab::Environments,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ShelfTab::Materials => "Materials",
            ShelfTab::SmartMaterials => "Smart materials",
            ShelfTab::SmartMasks => "Smart masks",
            ShelfTab::Brushes => "Brushes",
            ShelfTab::Alphas => "Alphas",
            ShelfTab::Textures => "Textures",
            ShelfTab::Environments => "Environments",
        }
    }
}

/// Which panels are visible (Window menu).
pub struct Panels {
    pub texture_sets: bool,
    pub layers: bool,
    pub properties: bool,
    pub display: bool,
    pub shelf: bool,
    pub tools: bool,
}

impl Default for Panels {
    fn default() -> Self {
        Self {
            texture_sets: true,
            layers: true,
            properties: true,
            display: true,
            shelf: true,
            tools: true,
        }
    }
}

/// State of the New Project dialog.
pub struct NewProjectDialog {
    pub mesh_path: Option<PathBuf>,
    pub primitive: Primitive,
    pub resolution: u32,
    pub normal_format: NormalFormat,
}

impl Default for NewProjectDialog {
    fn default() -> Self {
        Self {
            mesh_path: None,
            primitive: Primitive::Sphere,
            resolution: 2048,
            normal_format: NormalFormat::OpenGl,
        }
    }
}

/// Deferred editor operations, queued by the UI and applied after it is drawn.
pub enum Action {
    OpenMesh(PathBuf),
    OpenMeshDialog,
    NewPrimitive(Primitive),
    CreateProject,
    LoadChannelImage {
        set: usize,
        channel: Channel,
        path: PathBuf,
    },
    PickChannelImage {
        set: usize,
        channel: Channel,
    },
    ClearChannel {
        set: usize,
        channel: Channel,
    },
    /// Factors of a texture set changed (cheap uniform update).
    FactorsChanged(usize),
    /// Normal map convention changed for the whole project.
    NormalFormatChanged,
    FrameCamera,
}

/// GPU resources owned by the editor.
pub struct Gpu {
    pub render_state: egui_wgpu::RenderState,
    pub renderer: Renderer,
    pub scene: Option<Scene>,
    pub viewport: Option<(ViewportTarget, TextureId)>,
    pub uv: Option<(UvTarget, TextureId)>,
}

impl Gpu {
    /// Registers a texture view with egui, or points an existing id at it.
    pub fn register_texture(
        &self,
        view: &patina_render::wgpu::TextureView,
        existing: Option<TextureId>,
    ) -> TextureId {
        let mut egui_renderer = self.render_state.renderer.write();
        let filter = patina_render::wgpu::FilterMode::Linear;
        match existing {
            Some(id) => {
                egui_renderer.update_egui_texture_from_wgpu_texture(
                    &self.render_state.device,
                    view,
                    filter,
                    id,
                );
                id
            }
            None => egui_renderer.register_native_texture(&self.render_state.device, view, filter),
        }
    }
}

pub struct Status {
    pub text: String,
    pub is_error: bool,
    pub at: Instant,
}

pub struct PatinaApp {
    pub gpu: Option<Gpu>,
    pub doc: Document,
    pub camera: OrbitCamera,
    pub settings: ViewportSettings,
    pub uv_view: UvView,
    pub uv_fit_pending: bool,
    pub layout: Layout,
    pub selected_set: usize,
    pub panels: Panels,
    pub shelf_tab: ShelfTab,
    pub recent: Vec<PathBuf>,
    pub status: Option<Status>,
    pub new_project: Option<NewProjectDialog>,
    pub show_about: bool,
    pub actions: Vec<Action>,
}

impl PatinaApp {
    pub fn new(cc: &eframe::CreationContext<'_>, mesh: Option<PathBuf>) -> Self {
        theme::apply(&cc.egui_ctx);
        let gpu = cc.wgpu_render_state.as_ref().map(|rs| {
            let renderer = Renderer::new(&rs.device, &rs.queue);
            Gpu {
                render_state: rs.clone(),
                renderer,
                scene: None,
                viewport: None,
                uv: None,
            }
        });
        let recent = cc
            .storage
            .and_then(|s| eframe::get_value::<Vec<PathBuf>>(s, RECENT_KEY))
            .unwrap_or_default();

        let mut app = Self {
            gpu,
            doc: Document::primitive(Primitive::Sphere),
            camera: OrbitCamera::default(),
            settings: ViewportSettings::default(),
            uv_view: UvView::default(),
            uv_fit_pending: true,
            layout: Layout::Split,
            selected_set: 0,
            panels: Panels::default(),
            shelf_tab: ShelfTab::Environments,
            recent,
            status: None,
            new_project: None,
            show_about: false,
            actions: Vec::new(),
        };
        if app.gpu.is_none() {
            app.set_error("No wgpu render state: the viewport is disabled");
        }
        match mesh {
            Some(path) => app.open_mesh(path),
            None => app.set_document(Document::primitive(Primitive::Sphere)),
        }
        app
    }

    pub fn set_status(&mut self, text: impl Into<String>) {
        self.status = Some(Status {
            text: text.into(),
            is_error: false,
            at: Instant::now(),
        });
    }

    pub fn set_error(&mut self, text: impl Into<String>) {
        let text = text.into();
        log::error!("{text}");
        self.status = Some(Status {
            text,
            is_error: true,
            at: Instant::now(),
        });
    }

    /// Replaces the document and rebuilds GPU resources.
    pub fn set_document(&mut self, doc: Document) {
        self.doc = doc;
        self.selected_set = 0;
        self.camera.frame(&self.doc.mesh.bounds());
        self.uv_fit_pending = true;
        self.rebuild_scene();
        if !self.doc.mesh.has_uvs {
            self.set_error(
                "This mesh has no UVs: texturing needs UVs (auto-unwrap arrives in M10)",
            );
        }
    }

    pub fn rebuild_scene(&mut self) {
        if let Some(gpu) = &mut self.gpu {
            gpu.scene = Some(gpu.renderer.create_scene(&self.doc.mesh, &self.doc.project));
        }
    }

    /// Re-uploads one texture set's textures.
    fn rebuild_material(&mut self, set_index: usize) {
        let (Some(gpu), Some(set)) = (&mut self.gpu, self.doc.project.texture_sets.get(set_index))
        else {
            return;
        };
        let Some(scene) = &mut gpu.scene else { return };
        if let Some(slot) = scene.materials.get_mut(set.material_index) {
            *slot = gpu.renderer.create_material(
                set,
                self.doc.mesh.has_uvs,
                self.doc.project.normal_format,
            );
        }
    }

    fn update_factors(&mut self, set_index: usize) {
        let (Some(gpu), Some(set)) = (&self.gpu, self.doc.project.texture_sets.get(set_index))
        else {
            return;
        };
        let Some(scene) = &gpu.scene else { return };
        if let Some(material) = scene.materials.get(set.material_index) {
            gpu.renderer.update_material_factors(
                material,
                set,
                self.doc.mesh.has_uvs,
                self.doc.project.normal_format,
            );
        }
    }

    pub fn open_mesh(&mut self, path: PathBuf) {
        match Document::open(&path) {
            Ok(doc) => {
                let summary = format!(
                    "Opened {} ({} triangles, {} texture sets)",
                    path.display(),
                    doc.mesh.triangle_count(),
                    doc.project.texture_sets.len()
                );
                self.set_document(doc);
                if self.doc.mesh.has_uvs {
                    self.set_status(summary);
                }
                self.add_recent(path);
            }
            Err(e) => self.set_error(format!("{e:#}")),
        }
    }

    fn add_recent(&mut self, path: PathBuf) {
        self.recent.retain(|p| p != &path);
        self.recent.insert(0, path);
        self.recent.truncate(MAX_RECENT);
    }

    /// The UV view shows the channel matching the current 3D view mode.
    pub fn uv_channel(&self) -> Channel {
        match self.settings.view_mode {
            ViewMode::Metallic => Channel::Metallic,
            ViewMode::Roughness => Channel::Roughness,
            ViewMode::Normal => Channel::Normal,
            ViewMode::Emissive => Channel::Emissive,
            _ => Channel::BaseColor,
        }
    }

    fn apply_actions(&mut self) {
        for action in std::mem::take(&mut self.actions) {
            match action {
                Action::OpenMesh(path) => self.open_mesh(path),
                Action::OpenMeshDialog => {
                    if let Some(path) = rfd::FileDialog::new()
                        .set_title("Open mesh")
                        .add_filter("Meshes", patina_mesh::SUPPORTED_EXTENSIONS)
                        .pick_file()
                    {
                        self.open_mesh(path);
                    }
                }
                Action::NewPrimitive(p) => {
                    self.set_document(Document::primitive(p));
                    self.set_status(format!("New project from primitive: {}", p.label()));
                }
                Action::CreateProject => {
                    let Some(dialog) = self.new_project.take() else {
                        continue;
                    };
                    let doc = match &dialog.mesh_path {
                        Some(path) => match Document::open(path) {
                            Ok(doc) => {
                                self.add_recent(path.clone());
                                doc
                            }
                            Err(e) => {
                                self.set_error(format!("{e:#}"));
                                self.new_project = Some(dialog);
                                continue;
                            }
                        },
                        None => Document::primitive(dialog.primitive),
                    };
                    let mut doc = doc;
                    doc.apply_settings(dialog.resolution, dialog.normal_format);
                    let name = doc.project.name.clone();
                    self.set_document(doc);
                    if self.doc.mesh.has_uvs {
                        self.set_status(format!("New project: {name}"));
                    }
                }
                Action::PickChannelImage { set, channel } => {
                    if let Some(path) = rfd::FileDialog::new()
                        .set_title(format!("Load {} image", channel.label()))
                        .add_filter("Images", document::IMAGE_EXTENSIONS)
                        .pick_file()
                    {
                        self.load_channel_image(set, channel, path);
                    }
                }
                Action::LoadChannelImage { set, channel, path } => {
                    self.load_channel_image(set, channel, path)
                }
                Action::ClearChannel { set, channel } => {
                    if let Some(ts) = self.doc.project.texture_sets.get_mut(set) {
                        ts.set_image(channel, None);
                        self.rebuild_material(set);
                    }
                }
                Action::FactorsChanged(set) => self.update_factors(set),
                Action::NormalFormatChanged => {
                    for i in 0..self.doc.project.texture_sets.len() {
                        self.update_factors(i);
                    }
                }
                Action::FrameCamera => {
                    self.camera.frame(&self.doc.mesh.bounds());
                    self.uv_fit_pending = true;
                }
            }
        }
    }

    fn load_channel_image(&mut self, set: usize, channel: Channel, path: PathBuf) {
        match Image::load(&path) {
            Ok(image) => {
                let Some(ts) = self.doc.project.texture_sets.get_mut(set) else {
                    return;
                };
                let (w, h) = (image.width, image.height);
                ts.set_image(channel, Some(image));
                // A bitmap replaces the factor, as in other painters.
                match channel {
                    Channel::BaseColor => ts.base_color_factor = [1.0; 4],
                    Channel::Metallic => ts.metallic_factor = 1.0,
                    Channel::Roughness => ts.roughness_factor = 1.0,
                    Channel::Emissive if ts.emissive_factor == [0.0; 3] => {
                        ts.emissive_factor = [1.0; 3]
                    }
                    _ => {}
                }
                let set_name = ts.name.clone();
                self.rebuild_material(set);
                self.set_status(format!(
                    "Loaded {w}x{h} image into {set_name} / {}",
                    channel.label()
                ));
            }
            Err(e) => self.set_error(format!("Could not load {}: {e}", path.display())),
        }
    }

    /// Files dropped on the window: meshes open, images go to the current
    /// texture set's channel matching the view mode.
    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_owned())
                .collect()
        });
        for path in dropped {
            if document::is_mesh_path(&path) {
                self.actions.push(Action::OpenMesh(path));
            } else if document::is_image_path(&path) {
                let channel = self.uv_channel();
                self.actions.push(Action::LoadChannelImage {
                    set: self.selected_set,
                    channel,
                    path,
                });
            } else {
                self.set_error(format!("Unsupported file: {}", path.display()));
            }
        }
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        use egui::{Key, KeyboardShortcut, Modifiers};
        if ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::O)))
        {
            self.actions.push(Action::OpenMeshDialog);
        }
        if ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::N)))
        {
            self.new_project = Some(NewProjectDialog::default());
        }
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        ctx.input(|i| {
            if i.key_pressed(Key::F1) {
                self.layout = Layout::Split;
            }
            if i.key_pressed(Key::F2) {
                self.layout = Layout::Only3d;
            }
            if i.key_pressed(Key::F3) {
                self.layout = Layout::Only2d;
            }
            if i.modifiers.is_none() {
                if i.key_pressed(Key::F) {
                    self.actions.push(Action::FrameCamera);
                }
                if i.key_pressed(Key::C) {
                    self.settings.view_mode = self.settings.view_mode.next_channel();
                }
                if i.key_pressed(Key::M) {
                    self.settings.view_mode = ViewMode::Material;
                }
            }
        });
    }
}

impl eframe::App for PatinaApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.handle_shortcuts(&ctx);
        self.handle_dropped_files(&ctx);

        self.menu_bar(ui);
        self.status_bar(ui, frame);
        if self.panels.shelf {
            self.shelf(ui);
        }
        if self.panels.tools {
            self.tools_bar(ui);
        }
        self.side_panel(ui);
        self.central(ui);
        self.dialogs(&ctx);
        self.drop_overlay(&ctx);

        self.apply_actions();
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, RECENT_KEY, &self.recent);
    }
}
