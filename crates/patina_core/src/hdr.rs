use crate::ImageError;

/// A floating-point RGBA image, used for HDRI environments.
#[derive(Clone, PartialEq)]
pub struct HdrImage {
    pub width: u32,
    pub height: u32,
    /// Linear RGBA, row-major, top row first.
    pub data: Vec<[f32; 4]>,
}

impl HdrImage {
    pub fn new(width: u32, height: u32, data: Vec<[f32; 4]>) -> Result<Self, ImageError> {
        if width == 0 || height == 0 {
            return Err(ImageError::Empty);
        }
        let expected = width as usize * height as usize;
        if data.len() != expected {
            return Err(ImageError::SizeMismatch {
                width,
                height,
                expected: expected * 16,
                actual: data.len() * 16,
            });
        }
        Ok(Self {
            width,
            height,
            data,
        })
    }

    /// Loads a Radiance `.hdr` or OpenEXR `.exr` file (or any LDR image, linearized).
    pub fn load(path: impl AsRef<std::path::Path>) -> Result<Self, ImageError> {
        let img = image::open(path)?.into_rgba32f();
        let (width, height) = img.dimensions();
        let data = img.into_raw().as_chunks::<4>().0.to_vec();
        Self::new(width, height, data)
    }

    /// Halves the resolution with a box filter (used to fit GPU limits).
    pub fn downsample(&self) -> Self {
        let (w, h) = ((self.width / 2).max(1), (self.height / 2).max(1));
        let mut data = Vec::with_capacity(w as usize * h as usize);
        for y in 0..h {
            for x in 0..w {
                let mut acc = [0.0f32; 4];
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let sx = (x * 2 + dx).min(self.width - 1);
                    let sy = (y * 2 + dy).min(self.height - 1);
                    let p = self.data[(sy * self.width + sx) as usize];
                    for c in 0..4 {
                        acc[c] += p[c] * 0.25;
                    }
                }
                data.push(acc);
            }
        }
        Self {
            width: w,
            height: h,
            data,
        }
    }

    /// Average color, a cheap sanity check for loaded environments.
    pub fn average(&self) -> [f32; 3] {
        let n = self.data.len() as f32;
        let mut s = [0.0f32; 3];
        for p in &self.data {
            for c in 0..3 {
                s[c] += p[c] / n;
            }
        }
        s
    }
}

impl std::fmt::Debug for HdrImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "HdrImage({}x{})", self.width, self.height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downsample_averages() {
        let img = HdrImage::new(2, 2, vec![[0.0; 4], [2.0; 4], [4.0; 4], [6.0; 4]]).unwrap();
        let half = img.downsample();
        assert_eq!((half.width, half.height), (1, 1));
        assert_eq!(half.data[0], [3.0; 4]);
    }

    #[test]
    fn hdr_round_trip() {
        // Write a tiny Radiance file with the image crate and read it back.
        let dir = std::env::temp_dir().join(format!("patina_hdr_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.hdr");
        let pixels = vec![image::Rgb([4.0f32, 0.5, 0.25]); 4];
        let file = std::fs::File::create(&path).unwrap();
        image::codecs::hdr::HdrEncoder::new(file)
            .encode(&pixels, 2, 2)
            .unwrap();
        let img = HdrImage::load(&path).unwrap();
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!((img.width, img.height), (2, 2));
        assert!((img.data[0][0] - 4.0).abs() < 0.1, "{:?}", img.data[0]);
        assert!(img.average()[1] > 0.4);
    }
}
