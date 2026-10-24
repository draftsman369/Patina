use patina_core::{MeshMap, NormalFormat};

/// How baking rays find their high-poly target.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MatchMode {
    /// Rays hit any high-poly triangle.
    #[default]
    Always,
    /// Low and high objects are paired by name, ignoring `_low` / `_high`
    /// suffixes; rays only hit their partner.
    ByName,
}

/// Where ID map colors come from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum IdSource {
    #[default]
    VertexColor,
    Material,
    Object,
}

impl IdSource {
    pub const ALL: [IdSource; 3] = [IdSource::VertexColor, IdSource::Material, IdSource::Object];

    pub fn label(self) -> &'static str {
        match self {
            IdSource::VertexColor => "Vertex color",
            IdSource::Material => "Material color",
            IdSource::Object => "Mesh ID",
        }
    }
}

/// Settings shared by all maps of one bake.
#[derive(Clone, Debug, PartialEq)]
pub struct BakeSettings {
    /// Output size in pixels (square).
    pub resolution: u32,
    /// Samples per pixel along each axis (1, 2, 4 or 8).
    pub supersampling: u32,
    /// Padding added around UV islands, in output pixels.
    pub dilation: u32,
    /// Ray start distance in front of the low-poly surface, relative to the bounding radius.
    pub max_frontal: f32,
    /// Ray distance behind the low-poly surface, relative to the bounding radius.
    pub max_rear: f32,
    /// Cast rays along smoothed normals so hard edges don't leave gaps.
    pub average_normals: bool,
    pub match_mode: MatchMode,
    pub maps: Vec<MeshMap>,
    pub normal_format: NormalFormat,
    pub ao_samples: u32,
    /// AO ray length, relative to the bounding radius.
    pub ao_distance: f32,
    pub ao_ignore_backfaces: bool,
    /// With [`MatchMode::ByName`], only objects with the same name occlude each other.
    pub ao_only_same_name: bool,
    pub thickness_samples: u32,
    /// Thickness ray length, relative to the bounding radius.
    pub thickness_distance: f32,
    /// Contrast of the curvature map.
    pub curvature_strength: f32,
    pub id_source: IdSource,
}

impl Default for BakeSettings {
    fn default() -> Self {
        Self {
            resolution: 2048,
            supersampling: 2,
            dilation: 16,
            max_frontal: 0.01,
            max_rear: 0.01,
            average_normals: true,
            match_mode: MatchMode::Always,
            maps: vec![
                MeshMap::Normal,
                MeshMap::WorldNormal,
                MeshMap::Id,
                MeshMap::AmbientOcclusion,
                MeshMap::Curvature,
                MeshMap::Position,
                MeshMap::Thickness,
            ],
            normal_format: NormalFormat::OpenGl,
            ao_samples: 64,
            ao_distance: 0.5,
            ao_ignore_backfaces: true,
            ao_only_same_name: false,
            thickness_samples: 32,
            thickness_distance: 0.5,
            curvature_strength: 1.0,
            id_source: IdSource::VertexColor,
        }
    }
}

impl BakeSettings {
    /// Settings clamped to supported values.
    pub fn sanitized(&self) -> Self {
        let mut s = self.clone();
        s.resolution = s.resolution.clamp(16, 8192);
        s.supersampling = match s.supersampling {
            0 | 1 => 1,
            2 | 3 => 2,
            4..=7 => 4,
            _ => 8,
        };
        // Keep the supersampled image within texture limits.
        while s.resolution * s.supersampling > 16384 && s.supersampling > 1 {
            s.supersampling /= 2;
        }
        s.max_frontal = s.max_frontal.max(1e-5);
        s.max_rear = s.max_rear.max(0.0);
        s.ao_samples = s.ao_samples.clamp(1, 1024);
        s.thickness_samples = s.thickness_samples.clamp(1, 1024);
        s.maps.sort();
        s.maps.dedup();
        s
    }
}

/// Removes a `_low` / `_high` / `_cage` style suffix and normalizes case,
/// so `Bolt_low` and `bolt_HIGH` pair up.
pub fn match_key(name: &str) -> String {
    let lower = name.trim().to_ascii_lowercase();
    for suffix in [
        "_low", "_high", "_lp", "_hp", "_cage", ".low", ".high", " low", " high",
    ] {
        if let Some(stripped) = lower.strip_suffix(suffix) {
            return stripped.to_owned();
        }
    }
    lower
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn match_keys_strip_suffixes() {
        assert_eq!(match_key("Bolt_low"), "bolt");
        assert_eq!(match_key("bolt_HIGH"), "bolt");
        assert_eq!(match_key("Body_hp"), "body");
        assert_eq!(match_key("Plain"), "plain");
    }

    #[test]
    fn sanitize_clamps() {
        let s = BakeSettings {
            resolution: 99999,
            supersampling: 3,
            maps: vec![MeshMap::Id, MeshMap::Normal, MeshMap::Id],
            ..Default::default()
        }
        .sanitized();
        assert_eq!(s.resolution, 8192);
        assert_eq!(s.supersampling, 2);
        assert_eq!(s.maps, vec![MeshMap::Normal, MeshMap::Id]);
    }
}
