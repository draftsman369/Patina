use crate::{Channel, Image, MeshMap};

/// How the green channel of normal maps is interpreted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NormalFormat {
    /// Y+ (glTF, Blender, Godot).
    #[default]
    OpenGl,
    /// Y- (Unreal, DirectX engines).
    DirectX,
}

/// One texture set: the textures painted for one material of the mesh.
#[derive(Clone, Debug)]
pub struct TextureSet {
    pub name: String,
    /// Index of the mesh material this set maps to.
    pub material_index: usize,
    /// Document resolution (square), used for painting and export.
    pub resolution: u32,
    /// Linear RGBA multiplier for the base color.
    pub base_color_factor: [f32; 4],
    pub metallic_factor: f32,
    pub roughness_factor: f32,
    /// Linear RGB multiplier for emission.
    pub emissive_factor: [f32; 3],
    /// Optional bitmap per channel, indexed by [`Channel::index`].
    /// `None` means the channel uses its factor only.
    pub images: [Option<Image>; Channel::COUNT],
    /// Baked or imported mesh maps, indexed by [`MeshMap::index`].
    pub mesh_maps: [Option<Image>; MeshMap::COUNT],
}

impl TextureSet {
    pub fn new(name: impl Into<String>, material_index: usize) -> Self {
        Self {
            name: name.into(),
            material_index,
            resolution: 2048,
            base_color_factor: [0.8, 0.8, 0.8, 1.0],
            metallic_factor: 0.0,
            roughness_factor: 0.5,
            emissive_factor: [0.0; 3],
            images: Default::default(),
            mesh_maps: Default::default(),
        }
    }

    pub fn image(&self, channel: Channel) -> Option<&Image> {
        self.images[channel.index()].as_ref()
    }

    pub fn set_image(&mut self, channel: Channel, image: Option<Image>) {
        self.images[channel.index()] = image;
    }

    pub fn mesh_map(&self, map: MeshMap) -> Option<&Image> {
        self.mesh_maps[map.index()].as_ref()
    }

    pub fn set_mesh_map(&mut self, map: MeshMap, image: Option<Image>) {
        self.mesh_maps[map.index()] = image;
    }
}

/// A Patina project: one mesh and its texture sets.
#[derive(Clone, Debug, Default)]
pub struct Project {
    pub name: String,
    /// Path of the mesh file, if it came from disk (primitives have none).
    pub mesh_path: Option<std::path::PathBuf>,
    pub normal_format: NormalFormat,
    pub texture_sets: Vec<TextureSet>,
}

impl Project {
    pub fn texture_set_for_material(&self, material_index: usize) -> Option<&TextureSet> {
        self.texture_sets
            .iter()
            .find(|t| t.material_index == material_index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_and_clear_image() {
        let mut ts = TextureSet::new("body", 0);
        assert!(ts.image(Channel::Roughness).is_none());
        ts.set_image(Channel::Roughness, Some(Image::solid(1, 1, [128; 4])));
        assert!(ts.image(Channel::Roughness).is_some());
        assert!(ts.image(Channel::Metallic).is_none());
        ts.set_image(Channel::Roughness, None);
        assert!(ts.image(Channel::Roughness).is_none());
    }

    #[test]
    fn finds_texture_set_by_material() {
        let project = Project {
            texture_sets: vec![TextureSet::new("a", 3), TextureSet::new("b", 1)],
            ..Default::default()
        };
        assert_eq!(project.texture_set_for_material(1).unwrap().name, "b");
        assert!(project.texture_set_for_material(0).is_none());
    }
}
