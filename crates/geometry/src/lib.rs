//! Geometry for Hephaestus.
//!
//! All design coordinates are integer nanometres ([`Coord`]) in a Y-up world (the convention of
//! Altium's coordinate readout and of Gerber). Floating point is used only for derived quantities
//! such as distances and rotations, and results are rounded back to nanometres.

mod index;
mod point;
mod shape;
mod units;

pub use index::GridIndex;
pub use point::{Point, Rect, Transform};
pub use shape::{Shape, point_segment_distance, polygon_contains, segment_segment_distance};
pub use units::{Unit, format_coord, parse_length};

/// One coordinate in nanometres.
pub type Coord = i64;

/// Nanometres per unit.
pub const NM_PER_MM: f64 = 1_000_000.0;
pub const NM_PER_MIL: f64 = 25_400.0;
pub const NM_PER_INCH: f64 = 25_400_000.0;

/// Millimetres to nanometres, rounded.
pub fn mm(value: f64) -> Coord {
    (value * NM_PER_MM).round() as Coord
}

/// Mils (thousandths of an inch) to nanometres, rounded.
pub fn mil(value: f64) -> Coord {
    (value * NM_PER_MIL).round() as Coord
}

/// Inches to nanometres, rounded.
pub fn inch(value: f64) -> Coord {
    (value * NM_PER_INCH).round() as Coord
}

/// Snap a coordinate to the nearest multiple of `grid` (a zero or negative grid leaves it as is).
pub fn snap(value: Coord, grid: Coord) -> Coord {
    if grid <= 0 {
        return value;
    }
    let half = grid / 2;
    if value >= 0 { (value + half) / grid * grid } else { -((-value + half) / grid * grid) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_conversions_round_to_nanometres() {
        assert_eq!(mm(1.0), 1_000_000);
        assert_eq!(mil(1.0), 25_400);
        assert_eq!(inch(1.0), mil(1000.0));
        assert_eq!(mm(0.0000004), 0);
    }

    #[test]
    fn snapping_is_symmetric_around_zero() {
        assert_eq!(snap(149, 100), 100);
        assert_eq!(snap(150, 100), 200);
        assert_eq!(snap(-149, 100), -100);
        assert_eq!(snap(-150, 100), -200);
        assert_eq!(snap(1234, 0), 1234);
    }
}
