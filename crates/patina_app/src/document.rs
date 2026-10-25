//! The open document: a mesh and the project built around it.

use std::path::{Path, PathBuf};

use anyhow::Context as _;
use patina_core::{NormalFormat, Project};
use patina_mesh::{Mesh, primitives::Primitive};

pub struct Document {
    pub project: Project,
    pub mesh: Mesh,
}

impl Document {
    pub fn from_mesh(mesh: Mesh, path: Option<PathBuf>) -> Self {
        let project = Project {
            name: mesh.name.clone(),
            mesh_path: path,
            normal_format: NormalFormat::OpenGl,
            texture_sets: mesh.texture_sets(),
        };
        Self { project, mesh }
    }

    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let mesh = patina_mesh::load(path)
            .with_context(|| format!("could not open {}", path.display()))?;
        Ok(Self::from_mesh(mesh, Some(path.to_owned())))
    }

    pub fn primitive(p: Primitive) -> Self {
        Self::from_mesh(p.build(), None)
    }

    /// Applies new-project settings to every texture set.
    pub fn apply_settings(&mut self, resolution: u32, normal_format: NormalFormat) {
        self.project.normal_format = normal_format;
        for set in &mut self.project.texture_sets {
            set.resolution = resolution;
        }
    }
}

/// True if the path looks like a mesh Patina can open.
pub fn is_mesh_path(path: &Path) -> bool {
    has_extension(path, patina_mesh::SUPPORTED_EXTENSIONS)
}

/// Formats accepted as HDRI environments.
pub const HDRI_EXTENSIONS: &[&str] = &["hdr", "exr"];

pub fn is_hdri_path(path: &Path) -> bool {
    has_extension(path, HDRI_EXTENSIONS)
}

/// Image formats accepted for channel slots.
pub const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "tga", "bmp", "tif", "tiff", "hdr"];

pub fn is_image_path(path: &Path) -> bool {
    has_extension(path, IMAGE_EXTENSIONS)
}

fn has_extension(path: &Path, list: &[&str]) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| list.iter().any(|x| x.eq_ignore_ascii_case(e)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primitive_document_has_one_texture_set() {
        let doc = Document::primitive(Primitive::Cube);
        assert_eq!(doc.project.texture_sets.len(), 1);
        assert_eq!(doc.project.texture_sets[0].material_index, 0);
        assert!(doc.project.mesh_path.is_none());
    }

    #[test]
    fn settings_apply_to_all_sets() {
        let mut doc = Document::primitive(Primitive::Sphere);
        doc.apply_settings(4096, NormalFormat::DirectX);
        assert_eq!(doc.project.texture_sets[0].resolution, 4096);
        assert_eq!(doc.project.normal_format, NormalFormat::DirectX);
    }

    #[test]
    fn recognizes_file_types() {
        assert!(is_mesh_path(Path::new("a/b/Model.GLB")));
        assert!(is_mesh_path(Path::new("x.obj")));
        assert!(is_mesh_path(Path::new("x.FBX")));
        assert!(!is_mesh_path(Path::new("x.blend")));
        assert!(is_image_path(Path::new("albedo.PNG")));
        assert!(is_hdri_path(Path::new("studio.EXR")));
        assert!(!is_hdri_path(Path::new("albedo.png")));
        assert!(!is_image_path(Path::new("notes.txt")));
    }
}
