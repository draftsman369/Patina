use patina_core::MeshMap;

use crate::environment::EnvSource;

/// What the viewport displays. Values match the `VIEW_*` constants in common.wgsl.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ViewMode {
    /// Fully lit PBR material.
    #[default]
    Material,
    BaseColor,
    Metallic,
    Roughness,
    /// Tangent-space normal map as stored.
    Normal,
    /// Final shading normal in world space.
    WorldNormal,
    UvChecker,
    Emissive,
    /// The mesh map chosen in [`ViewportSettings::mesh_map`] (B shortcut).
    MeshMap,
}

impl ViewMode {
    pub const ALL: [ViewMode; 9] = [
        ViewMode::Material,
        ViewMode::BaseColor,
        ViewMode::Metallic,
        ViewMode::Roughness,
        ViewMode::Normal,
        ViewMode::WorldNormal,
        ViewMode::UvChecker,
        ViewMode::Emissive,
        ViewMode::MeshMap,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ViewMode::Material => "Material",
            ViewMode::BaseColor => "Base Color",
            ViewMode::Metallic => "Metallic",
            ViewMode::Roughness => "Roughness",
            ViewMode::Normal => "Normal",
            ViewMode::WorldNormal => "World Normal",
            ViewMode::UvChecker => "UV Checker",
            ViewMode::Emissive => "Emissive",
            ViewMode::MeshMap => "Mesh Map",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        let norm = |s: &str| s.to_ascii_lowercase().replace([' ', '-', '_'], "");
        Self::ALL
            .into_iter()
            .find(|m| norm(m.label()) == norm(name))
    }

    /// The next channel view, used by the C shortcut (skips Material and Mesh Map).
    pub fn next_channel(self) -> Self {
        let cycle = &Self::ALL[1..Self::ALL.len() - 1];
        let i = cycle.iter().position(|&m| m == self);
        match i {
            Some(i) => cycle[(i + 1) % cycle.len()],
            None => cycle[0],
        }
    }

    pub(crate) fn shader_id(self) -> u32 {
        self as u32
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tonemap {
    #[default]
    Aces,
    Filmic,
    Linear,
}

impl Tonemap {
    pub const ALL: [Tonemap; 3] = [Tonemap::Aces, Tonemap::Filmic, Tonemap::Linear];

    pub fn label(self) -> &'static str {
        match self {
            Tonemap::Aces => "ACES",
            Tonemap::Filmic => "Filmic",
            Tonemap::Linear => "Linear",
        }
    }
}

/// A light lobe of an analytic environment (a spherical Gaussian).
#[derive(Clone, Copy, Debug)]
pub struct EnvLight {
    /// Direction the light comes from (normalized at upload).
    pub direction: [f32; 3],
    /// Lobe sharpness: higher is a smaller, harder light.
    pub sharpness: f32,
    /// Peak radiance (linear RGB).
    pub color: [f32; 3],
}

/// An analytic studio environment: a sky gradient plus up to four light lobes.
#[derive(Clone, Copy, Debug)]
pub struct EnvPreset {
    pub name: &'static str,
    pub sky_top: [f32; 3],
    pub sky_horizon: [f32; 3],
    pub sky_ground: [f32; 3],
    pub lights: &'static [EnvLight],
}

const fn light(direction: [f32; 3], sharpness: f32, color: [f32; 3]) -> EnvLight {
    EnvLight {
        direction,
        sharpness,
        color,
    }
}

pub const ENV_PRESETS: [EnvPreset; 5] = [
    EnvPreset {
        name: "Studio Soft",
        sky_top: [0.50, 0.52, 0.55],
        sky_horizon: [0.62, 0.62, 0.62],
        sky_ground: [0.16, 0.15, 0.14],
        lights: &[
            light([0.6, 0.7, 0.5], 24.0, [7.0, 6.8, 6.5]),
            light([-0.7, 0.3, 0.4], 12.0, [1.8, 1.9, 2.1]),
            light([-0.2, 0.5, -0.9], 40.0, [5.0, 5.0, 5.0]),
        ],
    },
    EnvPreset {
        name: "Studio Contrast",
        sky_top: [0.05, 0.05, 0.055],
        sky_horizon: [0.08, 0.08, 0.08],
        sky_ground: [0.02, 0.02, 0.02],
        lights: &[
            light([0.5, 0.6, 0.6], 60.0, [28.0, 27.0, 25.0]),
            light([-0.6, 0.4, -0.7], 80.0, [20.0, 20.0, 22.0]),
            light([0.0, 1.0, 0.0], 10.0, [1.4, 1.4, 1.4]),
        ],
    },
    EnvPreset {
        name: "Overcast",
        sky_top: [0.80, 0.85, 0.95],
        sky_horizon: [0.85, 0.85, 0.85],
        sky_ground: [0.22, 0.20, 0.18],
        lights: &[light([0.0, 1.0, 0.0], 3.0, [0.6, 0.6, 0.65])],
    },
    EnvPreset {
        name: "Sunset",
        sky_top: [0.20, 0.30, 0.55],
        sky_horizon: [0.95, 0.50, 0.28],
        sky_ground: [0.12, 0.09, 0.07],
        lights: &[light([0.8, 0.12, 0.2], 600.0, [420.0, 260.0, 150.0])],
    },
    EnvPreset {
        name: "Night Rim",
        sky_top: [0.02, 0.03, 0.06],
        sky_horizon: [0.04, 0.05, 0.08],
        sky_ground: [0.01, 0.01, 0.01],
        lights: &[
            light([-0.8, 0.3, -0.5], 50.0, [8.0, 12.0, 24.0]),
            light([0.8, 0.3, -0.5], 50.0, [24.0, 14.0, 8.0]),
            light([0.3, 0.8, 0.6], 20.0, [1.5, 1.5, 1.5]),
        ],
    },
];

/// Display settings for one viewport.
#[derive(Clone, Debug, PartialEq)]
pub struct ViewportSettings {
    /// Lighting: an analytic preset or an HDRI.
    pub environment: EnvSource,
    /// Environment rotation around Y, radians.
    pub env_rotation: f32,
    /// Exposure in stops.
    pub exposure_ev: f32,
    /// 0 = sharp environment background, 1 = fully blurred.
    pub background_blur: f32,
    /// 0 = flat gray background, 1 = environment.
    pub background_opacity: f32,
    pub tonemap: Tonemap,
    pub view_mode: ViewMode,
    /// Mesh map shown by [`ViewMode::MeshMap`].
    pub mesh_map: MeshMap,
    pub wireframe: bool,
}

impl Default for ViewportSettings {
    fn default() -> Self {
        Self {
            environment: EnvSource::Preset(0),
            env_rotation: 0.0,
            exposure_ev: 0.0,
            background_blur: 0.6,
            background_opacity: 1.0,
            tonemap: Tonemap::Aces,
            view_mode: ViewMode::Material,
            mesh_map: MeshMap::Normal,
            wireframe: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_mode_ids_match_shader_constants() {
        // See VIEW_* in common.wgsl.
        assert_eq!(ViewMode::Material.shader_id(), 0);
        assert_eq!(ViewMode::UvChecker.shader_id(), 6);
        assert_eq!(ViewMode::Emissive.shader_id(), 7);
        assert_eq!(ViewMode::MeshMap.shader_id(), 8);
    }

    #[test]
    fn view_mode_parsing() {
        assert_eq!(
            ViewMode::from_name("world-normal"),
            Some(ViewMode::WorldNormal)
        );
        assert_eq!(ViewMode::from_name("basecolor"), Some(ViewMode::BaseColor));
        assert_eq!(ViewMode::from_name("nope"), None);
    }

    #[test]
    fn next_channel_skips_material() {
        let mut m = ViewMode::Material;
        for _ in 0..20 {
            m = m.next_channel();
            assert_ne!(m, ViewMode::Material);
            assert_ne!(m, ViewMode::MeshMap);
        }
        assert_eq!(ViewMode::Emissive.next_channel(), ViewMode::BaseColor);
        assert_eq!(ViewMode::MeshMap.next_channel(), ViewMode::BaseColor);
    }

    #[test]
    fn presets_fit_shader_limits() {
        for p in ENV_PRESETS {
            assert!(p.lights.len() <= 4, "{} has too many lights", p.name);
        }
    }
}
