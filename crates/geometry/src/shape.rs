use crate::{Coord, Point, Rect};

/// A copper or region shape for hit testing and clearance checks: a core (a point, a segment or a
/// simple polygon) inflated by `radius`.
///
/// This one representation covers circles (point + r), tracks and obround pads (segment + r),
/// rectangles (polygon), rounded rectangles (shrunken polygon + r) and regions (polygon), and
/// gives exact edge-to-edge distances between any two of them.
#[derive(Clone, Debug, PartialEq)]
pub struct Shape {
    pub core: Vec<Point>,
    pub radius: f64,
}

impl Shape {
    pub fn circle(center: Point, diameter: Coord) -> Self {
        Self { core: vec![center], radius: diameter as f64 / 2.0 }
    }

    /// A track: a segment with round ends.
    pub fn capsule(a: Point, b: Point, width: Coord) -> Self {
        Self { core: vec![a, b], radius: width as f64 / 2.0 }
    }

    pub fn rect(center: Point, width: Coord, height: Coord, rotation: f64) -> Self {
        let (hw, hh) = (width / 2, height / 2);
        let core =
            [(-hw, -hh), (hw, -hh), (hw, hh), (-hw, hh)].into_iter().map(|(x, y)| Point::new(x, y).rotated(rotation) + center).collect();
        Self { core, radius: 0.0 }
    }

    /// A rectangle whose corners are rounded with `corner_radius`.
    pub fn rounded_rect(center: Point, width: Coord, height: Coord, corner_radius: Coord, rotation: f64) -> Self {
        let r = corner_radius.clamp(0, width.min(height) / 2);
        if r == 0 {
            return Self::rect(center, width, height, rotation);
        }
        let mut shape = Self::rect(center, width - 2 * r, height - 2 * r, rotation);
        shape.radius = r as f64;
        shape
    }

    /// A stadium: the shorter side is fully rounded.
    pub fn obround(center: Point, width: Coord, height: Coord, rotation: f64) -> Self {
        let (half_len, d) = if width >= height { ((width - height) / 2, height) } else { ((height - width) / 2, width) };
        let axis = if width >= height { Point::new(half_len, 0) } else { Point::new(0, half_len) }.rotated(rotation);
        Self { core: vec![center - axis, center + axis], radius: d as f64 / 2.0 }
    }

    pub fn polygon(points: Vec<Point>) -> Self {
        Self { core: points, radius: 0.0 }
    }

    pub fn bounds(&self) -> Option<Rect> {
        let r = self.radius.ceil() as Coord;
        Rect::bounding(self.core.iter().copied()).map(|b| b.inflated(r))
    }

    /// Edges of the core; a single point yields one degenerate edge.
    fn edges(&self) -> impl Iterator<Item = (Point, Point)> + '_ {
        let n = self.core.len();
        let closed = n >= 3;
        let count = match n {
            0 => 0,
            1 => 1,
            _ if closed => n,
            _ => n - 1,
        };
        (0..count).filter_map(move |i| {
            let a = *self.core.get(i)?;
            let b = if n == 1 { a } else { *self.core.get((i + 1) % n)? };
            Some((a, b))
        })
    }

    fn core_contains(&self, p: Point) -> bool {
        self.core.len() >= 3 && polygon_contains(&self.core, p)
    }

    /// Distance between the cores, zero when they overlap.
    fn core_distance(&self, other: &Shape) -> f64 {
        if other.core.first().is_some_and(|&p| self.core_contains(p)) || self.core.first().is_some_and(|&p| other.core_contains(p)) {
            return 0.0;
        }
        let mut best = f64::INFINITY;
        for (a, b) in self.edges() {
            for (c, d) in other.edges() {
                best = best.min(segment_segment_distance(a, b, c, d));
                if best == 0.0 {
                    return 0.0;
                }
            }
        }
        best
    }

    /// Edge-to-edge gap: positive when apart, zero or negative when touching or overlapping.
    pub fn gap(&self, other: &Shape) -> f64 {
        self.core_distance(other) - self.radius - other.radius
    }

    pub fn contains(&self, p: Point) -> bool {
        self.gap(&Shape { core: vec![p], radius: 0.0 }) <= 0.0
    }

    /// Distance from a point to the shape's outline (zero inside).
    pub fn distance_to(&self, p: Point) -> f64 {
        self.gap(&Shape { core: vec![p], radius: 0.0 }).max(0.0)
    }
}

/// Even-odd point-in-polygon test (any simple polygon, convex or not).
pub fn polygon_contains(polygon: &[Point], p: Point) -> bool {
    let n = polygon.len();
    if n < 3 {
        return false;
    }
    let (px, py) = p.to_f64();
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let (Some(a), Some(b)) = (polygon.get(i), polygon.get(j)) else { break };
        let (ax, ay) = a.to_f64();
        let (bx, by) = b.to_f64();
        if (ay > py) != (by > py) && px < (bx - ax) * (py - ay) / (by - ay) + ax {
            inside = !inside;
        }
        j = i;
    }
    inside
}

pub fn point_segment_distance(p: Point, a: Point, b: Point) -> f64 {
    let (px, py) = p.to_f64();
    let (ax, ay) = a.to_f64();
    let (bx, by) = b.to_f64();
    let (dx, dy) = (bx - ax, by - ay);
    let len2 = dx * dx + dy * dy;
    let t = if len2 == 0.0 { 0.0 } else { (((px - ax) * dx + (py - ay) * dy) / len2).clamp(0.0, 1.0) };
    (px - (ax + t * dx)).hypot(py - (ay + t * dy))
}

fn orientation(a: Point, b: Point, c: Point) -> i128 {
    let v = (b.x - a.x) as i128 * (c.y - a.y) as i128 - (b.y - a.y) as i128 * (c.x - a.x) as i128;
    v.signum()
}

fn segments_intersect(a: Point, b: Point, c: Point, d: Point) -> bool {
    let (o1, o2, o3, o4) = (orientation(a, b, c), orientation(a, b, d), orientation(c, d, a), orientation(c, d, b));
    if o1 != o2 && o3 != o4 && o1 != 0 && o2 != 0 && o3 != 0 && o4 != 0 {
        return true;
    }
    let on = |p: Point, q: Point, r: Point| {
        orientation(p, q, r) == 0 && r.x >= p.x.min(q.x) && r.x <= p.x.max(q.x) && r.y >= p.y.min(q.y) && r.y <= p.y.max(q.y)
    };
    on(a, b, c) || on(a, b, d) || on(c, d, a) || on(c, d, b)
}

pub fn segment_segment_distance(a: Point, b: Point, c: Point, d: Point) -> f64 {
    if segments_intersect(a, b, c, d) {
        return 0.0;
    }
    point_segment_distance(a, c, d)
        .min(point_segment_distance(b, c, d))
        .min(point_segment_distance(c, a, b))
        .min(point_segment_distance(d, a, b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn circle_gaps() {
        let a = Shape::circle(Point::new(0, 0), 100);
        let b = Shape::circle(Point::new(300, 0), 100);
        assert_eq!(a.gap(&b), 200.0);
        let c = Shape::circle(Point::new(50, 0), 100);
        assert!(a.gap(&c) < 0.0);
    }

    #[test]
    fn capsule_to_rect_gap() {
        let track = Shape::capsule(Point::new(0, 0), Point::new(1000, 0), 100);
        let pad = Shape::rect(Point::new(500, 300), 200, 200, 0.0);
        assert!((track.gap(&pad) - 150.0).abs() < 1e-9);
    }

    #[test]
    fn containment_counts_as_overlap() {
        let big = Shape::rect(Point::ZERO, 1000, 1000, 0.0);
        let small = Shape::circle(Point::new(10, 10), 10);
        assert!(big.gap(&small) <= 0.0);
        assert!(small.gap(&big) <= 0.0);
        assert!(big.contains(Point::new(400, -400)));
        assert!(!big.contains(Point::new(600, 0)));
    }

    #[test]
    fn crossing_segments_touch() {
        assert_eq!(segment_segment_distance(Point::new(-10, 0), Point::new(10, 0), Point::new(0, -10), Point::new(0, 10)), 0.0);
        assert_eq!(segment_segment_distance(Point::new(0, 0), Point::new(10, 0), Point::new(0, 5), Point::new(10, 5)), 5.0);
    }

    #[test]
    fn obround_and_rounded_rect_bounds() {
        let o = Shape::obround(Point::ZERO, 300, 100, 0.0);
        assert_eq!(o.bounds(), Some(Rect::from_corners(Point::new(-150, -50), Point::new(150, 50))));
        let rr = Shape::rounded_rect(Point::ZERO, 200, 100, 20, 0.0);
        assert_eq!(rr.bounds(), Some(Rect::from_corners(Point::new(-100, -50), Point::new(100, 50))));
        assert!(rr.contains(Point::new(99, 0)));
        assert!(!rr.contains(Point::new(99, 49)));
    }

    #[test]
    fn concave_polygon_containment() {
        let l = [Point::new(0, 0), Point::new(10, 0), Point::new(10, 5), Point::new(5, 5), Point::new(5, 10), Point::new(0, 10)];
        assert!(polygon_contains(&l, Point::new(2, 8)));
        assert!(!polygon_contains(&l, Point::new(8, 8)));
    }
}
