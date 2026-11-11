//! Snapping geometry, in design-space pixels. No UI types, so it is easy to test.
//!
//! A moving box snaps its left/centre/right edges to vertical targets and its
//! top/middle/bottom edges to horizontal targets. Targets are the region's edges and
//! centre lines plus the edges and centres of the other elements. When nothing is
//! close enough and a grid is enabled, the box snaps to the grid instead.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Box4 {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct Config {
    /// Maximum distance, in design pixels, at which an edge is pulled to a target.
    pub threshold: f32,
    /// Grid spacing in design pixels; `None` disables grid snapping.
    pub grid: Option<f32>,
    /// Snap to other elements and the region's edges and centre.
    pub objects: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Targets {
    pub xs: Vec<f32>,
    pub ys: Vec<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Snapped {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    /// Vertical guide line (an x position) to draw, if an object target was hit.
    pub guide_x: Option<f32>,
    /// Horizontal guide line (a y position) to draw, if an object target was hit.
    pub guide_y: Option<f32>,
}

pub fn targets(region_w: f32, region_h: f32, others: &[Box4]) -> Targets {
    let mut t = Targets { xs: vec![0.0, region_w / 2.0, region_w], ys: vec![0.0, region_h / 2.0, region_h] };
    for o in others {
        t.xs.extend([o.x, o.x + o.w / 2.0, o.x + o.w]);
        t.ys.extend([o.y, o.y + o.h / 2.0, o.y + o.h]);
    }
    t
}

/// Smallest `(delta, target)` that moves any edge onto any target within `threshold`.
fn best(edges: &[f32], targets: &[f32], threshold: f32) -> Option<(f32, f32)> {
    let mut found: Option<(f32, f32)> = None;
    for &e in edges {
        for &t in targets {
            let d = t - e;
            if d.abs() <= threshold && found.is_none_or(|(bd, _)| d.abs() < bd.abs()) {
                found = Some((d, t));
            }
        }
    }
    found
}

fn grid_delta(edge: f32, grid: f32) -> f32 {
    (edge / grid).round() * grid - edge
}

fn axis(edges: &[f32], leading: f32, targets: &[f32], cfg: &Config) -> (f32, Option<f32>) {
    if cfg.objects {
        if let Some((d, t)) = best(edges, targets, cfg.threshold) {
            return (d, Some(t));
        }
    }
    match cfg.grid {
        Some(g) if g >= 1.0 => (grid_delta(leading, g), None),
        _ => (0.0, None),
    }
}

/// Snap a box that is being dragged.
pub fn snap_move(b: Box4, t: &Targets, cfg: &Config) -> Snapped {
    let (dx, gx) = axis(&[b.x, b.x + b.w / 2.0, b.x + b.w], b.x, &t.xs, cfg);
    let (dy, gy) = axis(&[b.y, b.y + b.h / 2.0, b.y + b.h], b.y, &t.ys, cfg);
    Snapped { x: b.x + dx, y: b.y + dy, w: b.w, h: b.h, guide_x: gx, guide_y: gy }
}

/// Snap a box whose bottom-right corner is being dragged. The top-left corner stays put.
pub fn snap_resize(b: Box4, t: &Targets, cfg: &Config) -> Snapped {
    let (dx, gx) = axis(&[b.x + b.w], b.x + b.w, &t.xs, cfg);
    let (dy, gy) = axis(&[b.y + b.h], b.y + b.h, &t.ys, cfg);
    Snapped { x: b.x, y: b.y, w: b.w + dx, h: b.h + dy, guide_x: gx, guide_y: gy }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CFG: Config = Config { threshold: 6.0, grid: None, objects: true };

    fn other() -> Box4 {
        Box4 { x: 100.0, y: 200.0, w: 200.0, h: 50.0 }
    }

    #[test]
    fn snaps_left_edge_to_neighbour() {
        let t = targets(1000.0, 700.0, &[other()]);
        let s = snap_move(Box4 { x: 103.0, y: 400.0, w: 80.0, h: 40.0 }, &t, &CFG);
        assert_eq!((s.x, s.guide_x), (100.0, Some(100.0)));
        assert_eq!(s.guide_y, None);
    }

    #[test]
    fn snaps_centre_to_region_centre() {
        let t = targets(1000.0, 700.0, &[]);
        // box centre is at 497 -> pulled to 500
        let s = snap_move(Box4 { x: 457.0, y: 10.0, w: 80.0, h: 40.0 }, &t, &CFG);
        assert_eq!((s.x, s.guide_x), (460.0, Some(500.0)));
    }

    #[test]
    fn snaps_right_and_bottom_edges() {
        let t = targets(1000.0, 700.0, &[other()]);
        // right edge 298 -> 300 (neighbour right edge); bottom 252 -> 250
        let s = snap_move(Box4 { x: 198.0, y: 212.0, w: 100.0, h: 40.0 }, &t, &CFG);
        assert_eq!((s.x, s.y), (200.0, 210.0));
    }

    #[test]
    fn nothing_in_range_leaves_position_alone() {
        let t = targets(1000.0, 700.0, &[other()]);
        let s = snap_move(Box4 { x: 640.0, y: 420.0, w: 80.0, h: 40.0 }, &t, &CFG);
        assert_eq!((s.x, s.y, s.guide_x, s.guide_y), (640.0, 420.0, None, None));
    }

    #[test]
    fn picks_the_closest_target() {
        let t = Targets { xs: vec![100.0, 104.0], ys: vec![] };
        let s = snap_move(Box4 { x: 103.0, y: 0.0, w: 10.0, h: 10.0 }, &t, &CFG);
        assert_eq!(s.x, 104.0);
    }

    #[test]
    fn grid_applies_when_no_object_is_near() {
        let cfg = Config { threshold: 6.0, grid: Some(20.0), objects: true };
        let t = targets(1000.0, 700.0, &[other()]);
        let s = snap_move(Box4 { x: 647.0, y: 423.0, w: 80.0, h: 40.0 }, &t, &cfg);
        assert_eq!((s.x, s.y, s.guide_x, s.guide_y), (640.0, 420.0, None, None));
        // objects win over the grid
        let s = snap_move(Box4 { x: 103.0, y: 423.0, w: 80.0, h: 40.0 }, &t, &cfg);
        assert_eq!((s.x, s.y), (100.0, 420.0));
    }

    #[test]
    fn grid_only_mode_ignores_objects() {
        let cfg = Config { threshold: 6.0, grid: Some(10.0), objects: false };
        let t = targets(1000.0, 700.0, &[other()]);
        let s = snap_move(Box4 { x: 103.0, y: 77.0, w: 80.0, h: 40.0 }, &t, &cfg);
        assert_eq!((s.x, s.y, s.guide_x), (100.0, 80.0, None));
    }

    #[test]
    fn resize_moves_only_the_far_corner() {
        let t = targets(1000.0, 700.0, &[other()]);
        let s = snap_resize(Box4 { x: 50.0, y: 300.0, w: 247.0, h: 100.0 }, &t, &CFG);
        assert_eq!((s.x, s.y, s.w, s.h), (50.0, 300.0, 250.0, 100.0));
        assert_eq!(s.guide_x, Some(300.0));
    }
}
