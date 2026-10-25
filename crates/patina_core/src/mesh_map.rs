/// A baked mesh map stored in a texture set, used by display modes now and
/// by generators and smart materials later.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MeshMap {
    /// Tangent-space normal from the high-poly mesh.
    Normal,
    WorldNormal,
    /// Color ID (from vertex colors, materials or objects).
    Id,
    AmbientOcclusion,
    Curvature,
    /// Position normalized to the mesh bounds.
    Position,
    Thickness,
    Height,
    Opacity,
    BentNormal,
}

impl MeshMap {
    pub const ALL: [MeshMap; 10] = [
        MeshMap::Normal,
        MeshMap::WorldNormal,
        MeshMap::Id,
        MeshMap::AmbientOcclusion,
        MeshMap::Curvature,
        MeshMap::Position,
        MeshMap::Thickness,
        MeshMap::Height,
        MeshMap::Opacity,
        MeshMap::BentNormal,
    ];

    pub const COUNT: usize = Self::ALL.len();

    pub fn label(self) -> &'static str {
        match self {
            MeshMap::Normal => "Normal",
            MeshMap::WorldNormal => "World Space Normal",
            MeshMap::Id => "ID",
            MeshMap::AmbientOcclusion => "Ambient Occlusion",
            MeshMap::Curvature => "Curvature",
            MeshMap::Position => "Position",
            MeshMap::Thickness => "Thickness",
            MeshMap::Height => "Height",
            MeshMap::Opacity => "Opacity",
            MeshMap::BentNormal => "Bent Normal",
        }
    }

    /// Short machine name, used in file names and on the command line.
    pub fn key(self) -> &'static str {
        match self {
            MeshMap::Normal => "normal",
            MeshMap::WorldNormal => "world_normal",
            MeshMap::Id => "id",
            MeshMap::AmbientOcclusion => "ao",
            MeshMap::Curvature => "curvature",
            MeshMap::Position => "position",
            MeshMap::Thickness => "thickness",
            MeshMap::Height => "height",
            MeshMap::Opacity => "opacity",
            MeshMap::BentNormal => "bent_normal",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        let k = key.trim().to_ascii_lowercase().replace('-', "_");
        Self::ALL.into_iter().find(|m| m.key() == k)
    }

    pub fn index(self) -> usize {
        self as usize
    }

    /// The next map, wrapping around (B shortcut).
    pub fn next(self) -> Self {
        Self::ALL[(self.index() + 1) % Self::COUNT]
    }

    /// Value used for an empty slot so a missing map reads as neutral.
    pub fn neutral(self) -> [u8; 4] {
        match self {
            MeshMap::Normal | MeshMap::BentNormal => [128, 128, 255, 255],
            MeshMap::AmbientOcclusion | MeshMap::Opacity => [255, 255, 255, 255],
            MeshMap::Curvature | MeshMap::Height => [128, 128, 128, 255],
            _ => [0, 0, 0, 255],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_round_trip() {
        for m in MeshMap::ALL {
            assert_eq!(MeshMap::from_key(m.key()), Some(m));
        }
        assert_eq!(
            MeshMap::from_key("World-Normal"),
            Some(MeshMap::WorldNormal)
        );
        assert_eq!(MeshMap::from_key("nope"), None);
    }

    #[test]
    fn next_cycles_through_all() {
        let mut m = MeshMap::Normal;
        for _ in 0..MeshMap::COUNT {
            m = m.next();
        }
        assert_eq!(m, MeshMap::Normal);
    }
}
