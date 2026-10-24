/// A material channel stored in a texture set.
///
/// M1 covers the core metal/rough channels; the full list from the roadmap
/// (height, opacity, user channels…) arrives with the layer stack in M3.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Channel {
    BaseColor,
    Metallic,
    Roughness,
    Normal,
    Emissive,
}

impl Channel {
    pub const ALL: [Channel; 5] = [
        Channel::BaseColor,
        Channel::Metallic,
        Channel::Roughness,
        Channel::Normal,
        Channel::Emissive,
    ];

    pub const COUNT: usize = Self::ALL.len();

    pub fn label(self) -> &'static str {
        match self {
            Channel::BaseColor => "Base Color",
            Channel::Metallic => "Metallic",
            Channel::Roughness => "Roughness",
            Channel::Normal => "Normal",
            Channel::Emissive => "Emissive",
        }
    }

    /// Color channels are stored sRGB-encoded; data channels are linear.
    pub fn is_srgb(self) -> bool {
        matches!(self, Channel::BaseColor | Channel::Emissive)
    }

    /// Index into fixed-size per-channel arrays (`[T; Channel::COUNT]`).
    pub fn index(self) -> usize {
        self as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indices_match_all_order() {
        for (i, c) in Channel::ALL.iter().enumerate() {
            assert_eq!(c.index(), i);
        }
    }

    #[test]
    fn srgb_only_for_color_channels() {
        assert!(Channel::BaseColor.is_srgb());
        assert!(Channel::Emissive.is_srgb());
        assert!(!Channel::Normal.is_srgb());
        assert!(!Channel::Roughness.is_srgb());
    }
}
