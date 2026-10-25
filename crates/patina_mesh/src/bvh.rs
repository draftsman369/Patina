//! Bounding volume hierarchy over triangles.
//!
//! Built on the CPU with a binned surface-area heuristic and stored as a flat
//! array of 32-byte nodes so the GPU bakers can traverse it directly.

use bytemuck::{Pod, Zeroable};
use glam::Vec3;

use crate::Aabb;

/// A BVH node, laid out for GPU storage buffers.
///
/// Leaves have `count > 0` and cover `order[first..first + count]`.
/// Interior nodes have `count == 0`; their children are `first` and `first + 1`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct BvhNode {
    pub min: [f32; 3],
    pub first: u32,
    pub max: [f32; 3],
    pub count: u32,
}

impl BvhNode {
    pub fn is_leaf(&self) -> bool {
        self.count > 0
    }
}

#[derive(Clone, Debug, Default)]
pub struct Bvh {
    pub nodes: Vec<BvhNode>,
    /// Triangle indices in leaf order.
    pub order: Vec<u32>,
}

/// A ray hit: distance along the ray, triangle and barycentrics (of vertices 1 and 2).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub t: f32,
    pub triangle: u32,
    pub u: f32,
    pub v: f32,
}

const MAX_LEAF: usize = 4;
const BINS: usize = 12;

struct Builder<'a> {
    triangles: &'a [[Vec3; 3]],
    bounds: Vec<Aabb>,
    centroids: Vec<Vec3>,
    order: Vec<u32>,
    nodes: Vec<BvhNode>,
}

impl Bvh {
    /// Builds a BVH over `triangles`.
    pub fn build(triangles: &[[Vec3; 3]]) -> Bvh {
        if triangles.is_empty() {
            return Bvh {
                nodes: vec![BvhNode::default()],
                order: Vec::new(),
            };
        }
        let bounds: Vec<Aabb> = triangles
            .iter()
            .map(|t| {
                let mut b = Aabb::EMPTY;
                t.iter().for_each(|&p| b.grow(p));
                b
            })
            .collect();
        let centroids = bounds.iter().map(|b| b.center()).collect();
        let mut builder = Builder {
            triangles,
            bounds,
            centroids,
            order: (0..triangles.len() as u32).collect(),
            nodes: Vec::with_capacity(triangles.len() * 2 / MAX_LEAF + 1),
        };
        builder.nodes.push(BvhNode::default());
        builder.subdivide(0, 0, triangles.len());
        let _ = builder.triangles;
        Bvh {
            nodes: builder.nodes,
            order: builder.order,
        }
    }

    /// Closest hit along the ray within `[t_min, t_max]`, ignoring back faces when `cull_back`.
    pub fn intersect(
        &self,
        triangles: &[[Vec3; 3]],
        origin: Vec3,
        dir: Vec3,
        t_min: f32,
        t_max: f32,
    ) -> Option<Hit> {
        let inv = dir.recip();
        let mut best: Option<Hit> = None;
        let mut limit = t_max;
        let mut stack = [0u32; 64];
        let mut sp = 1;
        while sp > 0 {
            sp -= 1;
            let node = self.nodes[stack[sp] as usize];
            if slab(&node, origin, inv) > limit {
                continue;
            }
            if node.is_leaf() {
                for &tri in &self.order[node.first as usize..(node.first + node.count) as usize] {
                    if let Some((t, u, v)) =
                        intersect_triangle(&triangles[tri as usize], origin, dir)
                        && t >= t_min
                        && t < limit
                    {
                        limit = t;
                        best = Some(Hit {
                            t,
                            triangle: tri,
                            u,
                            v,
                        });
                    }
                }
            } else if node.count == 0 && sp + 2 <= stack.len() {
                stack[sp] = node.first;
                stack[sp + 1] = node.first + 1;
                sp += 2;
            }
        }
        best
    }
}

/// Entry distance into the node's box, or infinity on a miss.
fn slab(node: &BvhNode, origin: Vec3, inv: Vec3) -> f32 {
    let t0 = (Vec3::from(node.min) - origin) * inv;
    let t1 = (Vec3::from(node.max) - origin) * inv;
    let tmin = t0.min(t1).max_element().max(0.0);
    let tmax = t0.max(t1).min_element();
    if tmin <= tmax { tmin } else { f32::INFINITY }
}

/// Möller–Trumbore, double-sided. Returns (t, u, v).
pub fn intersect_triangle(tri: &[Vec3; 3], origin: Vec3, dir: Vec3) -> Option<(f32, f32, f32)> {
    let e1 = tri[1] - tri[0];
    let e2 = tri[2] - tri[0];
    let p = dir.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-12 {
        return None;
    }
    let inv = 1.0 / det;
    let s = origin - tri[0];
    let u = s.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = dir.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    Some((e2.dot(q) * inv, u, v))
}

impl Builder<'_> {
    fn range_bounds(&self, first: usize, count: usize) -> (Aabb, Aabb) {
        let mut b = Aabb::EMPTY;
        let mut c = Aabb::EMPTY;
        for &t in &self.order[first..first + count] {
            b = b.union(&self.bounds[t as usize]);
            c.grow(self.centroids[t as usize]);
        }
        (b, c)
    }

    fn subdivide(&mut self, node: usize, first: usize, count: usize) {
        let (bounds, centroid_bounds) = self.range_bounds(first, count);
        self.nodes[node].min = bounds.min.to_array();
        self.nodes[node].max = bounds.max.to_array();

        let make_leaf = |nodes: &mut Vec<BvhNode>| {
            nodes[node].first = first as u32;
            nodes[node].count = count as u32;
        };
        if count <= MAX_LEAF {
            make_leaf(&mut self.nodes);
            return;
        }

        // Binned SAH over the largest centroid axis.
        let extent = centroid_bounds.max - centroid_bounds.min;
        let axis = if extent.x >= extent.y && extent.x >= extent.z {
            0
        } else if extent.y >= extent.z {
            1
        } else {
            2
        };
        let lo = centroid_bounds.min[axis];
        let span = extent[axis];
        if span <= 1e-12 {
            // All centroids coincide: split in the middle.
            self.split(node, first, count, count / 2);
            return;
        }
        let bin_of = |c: Vec3| (((c[axis] - lo) / span * BINS as f32) as usize).min(BINS - 1);
        let mut bin_bounds = [Aabb::EMPTY; BINS];
        let mut bin_count = [0usize; BINS];
        for &t in &self.order[first..first + count] {
            let b = bin_of(self.centroids[t as usize]);
            bin_count[b] += 1;
            bin_bounds[b] = bin_bounds[b].union(&self.bounds[t as usize]);
        }
        let mut best = (f32::INFINITY, 0);
        for split in 1..BINS {
            let (mut lb, mut rb) = (Aabb::EMPTY, Aabb::EMPTY);
            let (mut lc, mut rc) = (0, 0);
            for i in 0..split {
                lb = lb.union(&bin_bounds[i]);
                lc += bin_count[i];
            }
            for i in split..BINS {
                rb = rb.union(&bin_bounds[i]);
                rc += bin_count[i];
            }
            if lc == 0 || rc == 0 {
                continue;
            }
            let cost = lb.surface_area() * lc as f32 + rb.surface_area() * rc as f32;
            if cost < best.0 {
                best = (cost, split);
            }
        }
        let leaf_cost = bounds.surface_area() * count as f32;
        if best.1 == 0 || (best.0 >= leaf_cost && count <= MAX_LEAF * 4) {
            if best.1 == 0 {
                self.split(node, first, count, count / 2);
            } else {
                make_leaf(&mut self.nodes);
            }
            return;
        }

        // Partition the range around the chosen bin.
        let order = &mut self.order[first..first + count];
        let mut i = 0;
        let mut j = order.len();
        while i < j {
            if bin_of(self.centroids[order[i] as usize]) < best.1 {
                i += 1;
            } else {
                j -= 1;
                order.swap(i, j);
            }
        }
        self.split(node, first, count, i);
    }

    /// Creates two children for `node`, the left one taking `left_count` triangles.
    fn split(&mut self, node: usize, first: usize, count: usize, left_count: usize) {
        let left_count = left_count.clamp(1, count - 1);
        if left_count == count / 2 {
            // Median split: order along the longest axis for a sensible tree.
            let (_, cb) = self.range_bounds(first, count);
            let e = cb.max - cb.min;
            let axis = if e.x >= e.y && e.x >= e.z {
                0
            } else if e.y >= e.z {
                1
            } else {
                2
            };
            let centroids = &self.centroids;
            self.order[first..first + count].sort_unstable_by(|a, b| {
                centroids[*a as usize][axis].total_cmp(&centroids[*b as usize][axis])
            });
        }
        let left = self.nodes.len();
        self.nodes.push(BvhNode::default());
        self.nodes.push(BvhNode::default());
        self.nodes[node].first = left as u32;
        self.nodes[node].count = 0;
        self.subdivide(left, first, left_count);
        self.subdivide(left + 1, first + left_count, count - left_count);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::Primitive;

    fn triangles(p: Primitive) -> Vec<[Vec3; 3]> {
        let m = p.build();
        (0..m.triangle_count()).map(|t| m.triangle(t)).collect()
    }

    fn brute_force(tris: &[[Vec3; 3]], o: Vec3, d: Vec3) -> Option<(f32, u32)> {
        tris.iter()
            .enumerate()
            .filter_map(|(i, t)| intersect_triangle(t, o, d).map(|(t, _, _)| (t, i as u32)))
            .filter(|(t, _)| *t >= 0.0)
            .min_by(|a, b| a.0.total_cmp(&b.0))
    }

    #[test]
    fn every_triangle_is_in_exactly_one_leaf() {
        let tris = triangles(Primitive::Torus);
        let bvh = Bvh::build(&tris);
        let mut seen = vec![0; tris.len()];
        for n in bvh.nodes.iter().filter(|n| n.is_leaf()) {
            for &t in &bvh.order[n.first as usize..(n.first + n.count) as usize] {
                seen[t as usize] += 1;
            }
        }
        assert!(seen.iter().all(|&c| c == 1));
    }

    #[test]
    fn matches_brute_force() {
        let tris = triangles(Primitive::Torus);
        let bvh = Bvh::build(&tris);
        // Deterministic pseudo-random rays from outside toward the origin.
        let mut seed = 1u32;
        let mut rnd = || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as f32 / (1 << 24) as f32 * 2.0 - 1.0
        };
        let mut hits = 0;
        for _ in 0..500 {
            let o = Vec3::new(rnd(), rnd(), rnd()).normalize() * 3.0;
            let target = Vec3::new(rnd(), rnd() * 0.3, rnd()) * 1.2;
            let d = (target - o).normalize();
            let expected = brute_force(&tris, o, d);
            let got = bvh.intersect(&tris, o, d, 0.0, f32::INFINITY);
            match (expected, got) {
                (None, None) => {}
                (Some((t, _)), Some(h)) => {
                    assert!((t - h.t).abs() < 1e-4, "t {t} vs {}", h.t);
                    hits += 1;
                }
                other => panic!("mismatch {other:?}"),
            }
        }
        assert!(hits > 100, "too few hits to be meaningful: {hits}");
    }

    #[test]
    fn handles_degenerate_input() {
        let same = [[Vec3::ZERO, Vec3::X, Vec3::Y]; 50];
        let bvh = Bvh::build(&same);
        assert!(
            bvh.intersect(&same, Vec3::new(0.2, 0.2, 1.0), Vec3::NEG_Z, 0.0, 10.0)
                .is_some()
        );
        let empty = Bvh::build(&[]);
        assert_eq!(empty.nodes.len(), 1);
    }
}
