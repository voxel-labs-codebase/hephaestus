use std::ops::{Add, Neg, Sub};

use serde::{Deserialize, Serialize};

use crate::Coord;

/// A point (or offset) in nanometres.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Point {
    pub x: Coord,
    pub y: Coord,
}

impl Point {
    pub const ZERO: Point = Point { x: 0, y: 0 };

    pub const fn new(x: Coord, y: Coord) -> Self {
        Self { x, y }
    }

    pub fn distance(self, other: Point) -> f64 {
        let dx = (other.x - self.x) as f64;
        let dy = (other.y - self.y) as f64;
        dx.hypot(dy)
    }

    /// Rotate about the origin by `degrees` counter-clockwise (Y up). Multiples of 90° are exact.
    pub fn rotated(self, degrees: f64) -> Point {
        let d = degrees.rem_euclid(360.0);
        if d == 0.0 {
            return self;
        }
        if d == 90.0 {
            return Point::new(-self.y, self.x);
        }
        if d == 180.0 {
            return Point::new(-self.x, -self.y);
        }
        if d == 270.0 {
            return Point::new(self.y, -self.x);
        }
        let (s, c) = d.to_radians().sin_cos();
        let (x, y) = (self.x as f64, self.y as f64);
        Point::new((x * c - y * s).round() as Coord, (x * s + y * c).round() as Coord)
    }

    pub fn snapped(self, grid: Coord) -> Point {
        Point::new(crate::snap(self.x, grid), crate::snap(self.y, grid))
    }

    pub fn to_f64(self) -> (f64, f64) {
        (self.x as f64, self.y as f64)
    }
}

impl Add for Point {
    type Output = Point;
    fn add(self, rhs: Point) -> Point {
        Point::new(self.x.saturating_add(rhs.x), self.y.saturating_add(rhs.y))
    }
}

impl Sub for Point {
    type Output = Point;
    fn sub(self, rhs: Point) -> Point {
        Point::new(self.x.saturating_sub(rhs.x), self.y.saturating_sub(rhs.y))
    }
}

impl Neg for Point {
    type Output = Point;
    fn neg(self) -> Point {
        Point::new(-self.x, -self.y)
    }
}

/// An axis-aligned rectangle, `min` inclusive and `max` inclusive.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rect {
    pub min: Point,
    pub max: Point,
}

impl Rect {
    /// The rectangle spanning two corners in any order.
    pub fn from_corners(a: Point, b: Point) -> Self {
        Self { min: Point::new(a.x.min(b.x), a.y.min(b.y)), max: Point::new(a.x.max(b.x), a.y.max(b.y)) }
    }

    pub fn from_center(center: Point, half_width: Coord, half_height: Coord) -> Self {
        Self::from_corners(
            Point::new(center.x - half_width, center.y - half_height),
            Point::new(center.x + half_width, center.y + half_height),
        )
    }

    /// The bounds of a set of points, or `None` when empty.
    pub fn bounding(points: impl IntoIterator<Item = Point>) -> Option<Self> {
        let mut iter = points.into_iter();
        let first = iter.next()?;
        Some(iter.fold(Rect { min: first, max: first }, |r, p| r.including(p)))
    }

    pub fn including(self, p: Point) -> Self {
        Rect { min: Point::new(self.min.x.min(p.x), self.min.y.min(p.y)), max: Point::new(self.max.x.max(p.x), self.max.y.max(p.y)) }
    }

    pub fn union(self, other: Rect) -> Self {
        self.including(other.min).including(other.max)
    }

    pub fn inflated(self, by: Coord) -> Self {
        Rect { min: Point::new(self.min.x - by, self.min.y - by), max: Point::new(self.max.x + by, self.max.y + by) }
    }

    pub fn width(&self) -> Coord {
        self.max.x - self.min.x
    }

    pub fn height(&self) -> Coord {
        self.max.y - self.min.y
    }

    pub fn center(&self) -> Point {
        Point::new(self.min.x + self.width() / 2, self.min.y + self.height() / 2)
    }

    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.min.x && p.x <= self.max.x && p.y >= self.min.y && p.y <= self.max.y
    }

    pub fn contains_rect(&self, other: &Rect) -> bool {
        self.contains(other.min) && self.contains(other.max)
    }

    pub fn intersects(&self, other: &Rect) -> bool {
        self.min.x <= other.max.x && other.min.x <= self.max.x && self.min.y <= other.max.y && other.min.y <= self.max.y
    }
}

/// Placement of a library object: mirror about the local Y axis, then rotate, then translate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Transform {
    pub offset: Point,
    /// Degrees counter-clockwise.
    pub rotation: f64,
    /// Mirror X (negate local x) before rotating — used for schematic mirroring and bottom-side
    /// footprints.
    pub mirror: bool,
}

impl Transform {
    pub fn new(offset: Point, rotation: f64, mirror: bool) -> Self {
        Self { offset, rotation, mirror }
    }

    pub fn apply(&self, local: Point) -> Point {
        let p = if self.mirror { Point::new(-local.x, local.y) } else { local };
        p.rotated(self.rotation) + self.offset
    }

    /// The angle a local direction (in degrees) points to after this transform.
    pub fn apply_angle(&self, local_degrees: f64) -> f64 {
        let a = if self.mirror { 180.0 - local_degrees } else { local_degrees };
        (a + self.rotation).rem_euclid(360.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn right_angle_rotations_are_exact() {
        let p = Point::new(3, 1);
        assert_eq!(p.rotated(90.0), Point::new(-1, 3));
        assert_eq!(p.rotated(180.0), Point::new(-3, -1));
        assert_eq!(p.rotated(-90.0), Point::new(1, -3));
        assert_eq!(p.rotated(450.0), Point::new(-1, 3));
    }

    #[test]
    fn transform_mirrors_before_rotating() {
        let t = Transform::new(Point::new(100, 0), 90.0, true);
        assert_eq!(t.apply(Point::new(10, 0)), Point::new(100, -10));
        assert_eq!(t.apply_angle(0.0), 270.0);
    }

    #[test]
    fn rect_operations() {
        let r = Rect::from_corners(Point::new(10, 5), Point::new(-10, -5));
        assert_eq!(r.width(), 20);
        assert!(r.contains(Point::ZERO));
        assert!(r.intersects(&Rect::from_center(Point::new(12, 0), 3, 3)));
        assert!(!r.intersects(&Rect::from_center(Point::new(20, 0), 3, 3)));
        assert_eq!(Rect::bounding([]), None);
    }
}
