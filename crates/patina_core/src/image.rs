use thiserror::Error;

/// An 8-bit RGBA image held on the CPU.
///
/// Higher bit depths arrive with the per-channel formats in M3.
#[derive(Clone, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    /// Tightly packed RGBA8, row-major, top row first.
    pub data: Vec<u8>,
}

#[derive(Debug, Error)]
pub enum ImageError {
    #[error("image data is {actual} bytes, expected {expected} for {width}x{height} RGBA8")]
    SizeMismatch {
        width: u32,
        height: u32,
        expected: usize,
        actual: usize,
    },
    #[error("image has zero width or height")]
    Empty,
    #[error("could not read image: {0}")]
    Decode(#[from] image::ImageError),
}

impl Image {
    pub fn new(width: u32, height: u32, data: Vec<u8>) -> Result<Self, ImageError> {
        if width == 0 || height == 0 {
            return Err(ImageError::Empty);
        }
        let expected = width as usize * height as usize * 4;
        if data.len() != expected {
            return Err(ImageError::SizeMismatch {
                width,
                height,
                expected,
                actual: data.len(),
            });
        }
        Ok(Self {
            width,
            height,
            data,
        })
    }

    /// Loads an image file (PNG, JPEG, TGA, BMP, TIFF, HDR) and converts it to RGBA8.
    pub fn load(path: impl AsRef<std::path::Path>) -> Result<Self, ImageError> {
        let img = image::open(path)?.into_rgba8();
        let (width, height) = img.dimensions();
        Self::new(width, height, img.into_raw())
    }

    /// Decodes an in-memory encoded image (format guessed from its header).
    pub fn decode(bytes: &[u8]) -> Result<Self, ImageError> {
        let img = image::load_from_memory(bytes)?.into_rgba8();
        let (width, height) = img.dimensions();
        Self::new(width, height, img.into_raw())
    }

    /// Writes the image as PNG.
    pub fn save_png(&self, path: impl AsRef<std::path::Path>) -> Result<(), ImageError> {
        image::save_buffer_with_format(
            path,
            &self.data,
            self.width,
            self.height,
            image::ExtendedColorType::Rgba8,
            image::ImageFormat::Png,
        )?;
        Ok(())
    }

    /// A single-color image.
    pub fn solid(width: u32, height: u32, rgba: [u8; 4]) -> Self {
        let data = rgba.repeat(width as usize * height as usize);
        Self {
            width,
            height,
            data,
        }
    }

    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = (y as usize * self.width as usize + x as usize) * 4;
        [
            self.data[i],
            self.data[i + 1],
            self.data[i + 2],
            self.data[i + 3],
        ]
    }

    /// Builds a grayscale image from channel `c` (0..4) of `self`.
    ///
    /// Used to split packed textures (e.g. glTF metallic-roughness) into
    /// separate Patina channels.
    pub fn extract_channel(&self, c: usize) -> Self {
        assert!(c < 4, "channel index out of range");
        let data = self
            .data
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|px| [px[c], px[c], px[c], 255])
            .collect();
        Self {
            width: self.width,
            height: self.height,
            data,
        }
    }
}

impl std::fmt::Debug for Image {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Image({}x{})", self.width, self.height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_wrong_size() {
        assert!(matches!(
            Image::new(2, 2, vec![0; 15]),
            Err(ImageError::SizeMismatch { .. })
        ));
        assert!(matches!(Image::new(0, 2, vec![]), Err(ImageError::Empty)));
    }

    #[test]
    fn extract_channel_copies_value() {
        let img = Image::new(1, 1, vec![10, 20, 30, 40]).unwrap();
        assert_eq!(img.extract_channel(2).pixel(0, 0), [30, 30, 30, 255]);
    }

    #[test]
    fn solid_fills_every_pixel() {
        let img = Image::solid(3, 2, [1, 2, 3, 4]);
        assert_eq!(img.data.len(), 24);
        assert_eq!(img.pixel(2, 1), [1, 2, 3, 4]);
    }
}
