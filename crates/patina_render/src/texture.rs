//! Texture upload with a CPU-generated mip chain.

use patina_core::Image;

/// One mip level: width, height, RGBA8 data.
pub(crate) type Mip = (u32, u32, Vec<u8>);

fn srgb_to_linear(v: u8) -> f32 {
    let c = v as f32 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(c: f32) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let s = if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    (s * 255.0 + 0.5) as u8
}

/// Halves an image with a box filter. Color channels of sRGB images are
/// averaged in linear space; alpha and data images are averaged directly.
pub(crate) fn downsample(src: &[u8], w: u32, h: u32, srgb: bool, lut: &[f32; 256]) -> Mip {
    let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
    let mut out = Vec::with_capacity(nw as usize * nh as usize * 4);
    for y in 0..nh {
        for x in 0..nw {
            let mut acc = [0.0f32; 4];
            let mut n = 0.0;
            for (sx, sy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let px = (x * 2 + sx).min(w - 1);
                let py = (y * 2 + sy).min(h - 1);
                let i = (py as usize * w as usize + px as usize) * 4;
                for c in 0..4 {
                    let v = src[i + c];
                    acc[c] += if srgb && c < 3 {
                        lut[v as usize]
                    } else {
                        v as f32 / 255.0
                    };
                }
                n += 1.0;
            }
            for (c, a) in acc.iter().enumerate() {
                let v = a / n;
                out.push(if srgb && c < 3 {
                    linear_to_srgb(v)
                } else {
                    (v * 255.0 + 0.5) as u8
                });
            }
        }
    }
    (nw, nh, out)
}

/// Full mip chain for `image`, dropping levels larger than `max_dim`.
pub(crate) fn mip_chain(image: &Image, srgb: bool, max_dim: u32) -> Vec<Mip> {
    let lut: [f32; 256] = std::array::from_fn(|i| srgb_to_linear(i as u8));
    let mut mips = vec![(image.width, image.height, image.data.clone())];
    while let Some((w, h, data)) = mips.last() {
        if *w == 1 && *h == 1 {
            break;
        }
        let next = downsample(data, *w, *h, srgb, &lut);
        mips.push(next);
    }
    let skip = mips
        .iter()
        .take_while(|(w, h, _)| *w > max_dim || *h > max_dim)
        .count();
    if skip > 0 {
        log::warn!(
            "texture {}x{} exceeds the GPU limit of {max_dim}; uploading a smaller mip",
            image.width,
            image.height
        );
    }
    mips.split_off(skip)
}

pub(crate) fn upload(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    image: &Image,
    srgb: bool,
    label: &str,
) -> wgpu::Texture {
    let max_dim = device.limits().max_texture_dimension_2d;
    let mips = mip_chain(image, srgb, max_dim);
    let (w, h, _) = mips[0];
    let format = if srgb {
        wgpu::TextureFormat::Rgba8UnormSrgb
    } else {
        wgpu::TextureFormat::Rgba8Unorm
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: mips.len() as u32,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for (level, (mw, mh, data)) in mips.iter().enumerate() {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: level as u32,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(mw * 4),
                rows_per_image: Some(*mh),
            },
            wgpu::Extent3d {
                width: *mw,
                height: *mh,
                depth_or_array_layers: 1,
            },
        );
    }
    texture
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mip_chain_reaches_one_pixel() {
        let img = Image::solid(8, 4, [255, 0, 0, 255]);
        let mips = mip_chain(&img, true, 8192);
        let sizes: Vec<_> = mips.iter().map(|(w, h, _)| (*w, *h)).collect();
        assert_eq!(sizes, vec![(8, 4), (4, 2), (2, 1), (1, 1)]);
        // Solid colour survives filtering.
        assert_eq!(&mips[3].2, &[255, 0, 0, 255]);
    }

    #[test]
    fn oversized_levels_are_dropped() {
        let img = Image::solid(16, 16, [0; 4]);
        let mips = mip_chain(&img, false, 4);
        assert_eq!((mips[0].0, mips[0].1), (4, 4));
    }

    #[test]
    fn srgb_averaging_is_linear() {
        // Black and white average to linear 0.5, which is ~188 in sRGB (not 128).
        let lut: [f32; 256] = std::array::from_fn(|i| srgb_to_linear(i as u8));
        let src = [0, 0, 0, 255, 255, 255, 255, 255];
        let (_, _, out) = downsample(&src, 2, 1, true, &lut);
        assert!((186..=190).contains(&out[0]), "{}", out[0]);
        let (_, _, out) = downsample(&src, 2, 1, false, &lut);
        assert_eq!(out[0], 128);
    }
}
