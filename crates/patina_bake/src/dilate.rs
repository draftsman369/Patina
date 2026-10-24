//! Padding around UV islands so texture filtering and mips don't bleed
//! empty texels into the surface.

/// Grows covered texels (`w > 0`) outward by `radius` pixels. Each new texel
/// takes the average of its already-filled 8-neighbors, ring by ring.
pub fn dilate(pixels: &mut [[f32; 4]], width: usize, height: usize, radius: u32) {
    let mut filled: Vec<bool> = pixels.iter().map(|p| p[3] > 0.0).collect();
    let neighbors = |i: usize| {
        let (x, y) = ((i % width) as i64, (i / width) as i64);
        (-1..=1i64)
            .flat_map(move |dy| (-1..=1i64).map(move |dx| (dx, dy)))
            .filter(|&(dx, dy)| dx != 0 || dy != 0)
            .filter_map(move |(dx, dy)| {
                let (nx, ny) = (x + dx, y + dy);
                (nx >= 0 && ny >= 0 && nx < width as i64 && ny < height as i64)
                    .then(|| ny as usize * width + nx as usize)
            })
    };

    // Empty texels touching the covered area.
    let mut frontier: Vec<usize> = (0..pixels.len())
        .filter(|&i| !filled[i] && neighbors(i).any(|n| filled[n]))
        .collect();
    let mut queued = vec![false; pixels.len()];
    for _ in 0..radius {
        if frontier.is_empty() {
            break;
        }
        let mut updates = Vec::with_capacity(frontier.len());
        for &i in &frontier {
            let mut sum = [0.0f32; 3];
            let mut n = 0.0;
            for j in neighbors(i) {
                if filled[j] {
                    for c in 0..3 {
                        sum[c] += pixels[j][c];
                    }
                    n += 1.0;
                }
            }
            if n > 0.0 {
                updates.push((i, [sum[0] / n, sum[1] / n, sum[2] / n]));
            }
        }
        let mut next = Vec::new();
        for (i, rgb) in updates {
            // Dilated texels are marked with a tiny coverage so they count as filled.
            pixels[i] = [rgb[0], rgb[1], rgb[2], 1e-6];
            filled[i] = true;
        }
        for &i in &frontier {
            for j in neighbors(i) {
                if !filled[j] && !queued[j] {
                    queued[j] = true;
                    next.push(j);
                }
            }
        }
        for &j in &next {
            queued[j] = false;
        }
        frontier = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grows_by_radius() {
        let (w, h) = (9, 9);
        let mut px = vec![[0.0f32; 4]; w * h];
        px[4 * w + 4] = [1.0, 0.5, 0.25, 1.0];
        dilate(&mut px, w, h, 2);
        // Within 2 rings: filled with the source color.
        assert_eq!(&px[2 * w + 2][..3], &[1.0, 0.5, 0.25]);
        assert!(px[2 * w + 2][3] > 0.0);
        // 3 rings away: still empty.
        assert_eq!(px[w + 1][3], 0.0);
        // The original texel keeps full coverage.
        assert_eq!(px[4 * w + 4][3], 1.0);
    }

    #[test]
    fn zero_radius_is_noop() {
        let mut px = vec![[0.0f32; 4]; 4];
        px[0] = [1.0, 1.0, 1.0, 1.0];
        let before = px.clone();
        dilate(&mut px, 2, 2, 0);
        assert_eq!(px, before);
    }
}
