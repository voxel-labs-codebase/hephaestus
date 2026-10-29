use serde::{Deserialize, Serialize};

use crate::{Coord, NM_PER_INCH, NM_PER_MIL, NM_PER_MM};

/// Display units. Storage is always nanometres.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Unit {
    #[default]
    Mil,
    Mm,
    Inch,
}

impl Unit {
    pub fn suffix(self) -> &'static str {
        match self {
            Unit::Mil => "mil",
            Unit::Mm => "mm",
            Unit::Inch => "in",
        }
    }

    pub fn nm_per_unit(self) -> f64 {
        match self {
            Unit::Mil => NM_PER_MIL,
            Unit::Mm => NM_PER_MM,
            Unit::Inch => NM_PER_INCH,
        }
    }

    /// Decimal places worth showing for this unit.
    pub fn precision(self) -> usize {
        match self {
            Unit::Mil => 2,
            Unit::Mm => 4,
            Unit::Inch => 5,
        }
    }

    pub fn from_nm(self, nm: Coord) -> f64 {
        nm as f64 / self.nm_per_unit()
    }

    pub fn to_nm(self, value: f64) -> Coord {
        (value * self.nm_per_unit()).round() as Coord
    }

    /// Altium's `Q` shortcut toggles between imperial and metric.
    pub fn toggled(self) -> Unit {
        match self {
            Unit::Mil | Unit::Inch => Unit::Mm,
            Unit::Mm => Unit::Mil,
        }
    }
}

/// Format a length with trailing zeros trimmed, e.g. `100mil`, `0.254mm`.
pub fn format_coord(nm: Coord, unit: Unit) -> String {
    let text = format!("{:.*}", unit.precision(), unit.from_nm(nm));
    let text = if text.contains('.') { text.trim_end_matches('0').trim_end_matches('.').to_string() } else { text };
    let text = if text == "-0" { "0".to_string() } else { text };
    format!("{text}{}", unit.suffix())
}

/// Parse a length such as `10mil`, `0.25mm`, `0.1in` or a bare number in `default` units.
pub fn parse_length(text: &str, default: Unit) -> Option<Coord> {
    let text = text.trim().to_ascii_lowercase();
    let (number, unit) = [("mil", Unit::Mil), ("mm", Unit::Mm), ("inch", Unit::Inch), ("in", Unit::Inch), ("\"", Unit::Inch)]
        .into_iter()
        .find_map(|(suffix, unit)| text.strip_suffix(suffix).map(|n| (n.trim().to_string(), unit)))
        .unwrap_or((text.clone(), default));
    let value: f64 = number.parse().ok()?;
    if !value.is_finite() || value.abs() > 1.0e6 {
        return None;
    }
    Some(unit.to_nm(value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{mil, mm};

    #[test]
    fn formats_trim_trailing_zeros() {
        assert_eq!(format_coord(mil(100.0), Unit::Mil), "100mil");
        assert_eq!(format_coord(mm(0.25), Unit::Mm), "0.25mm");
        assert_eq!(format_coord(mil(10.0), Unit::Mm), "0.254mm");
        assert_eq!(format_coord(0, Unit::Mm), "0mm");
    }

    #[test]
    fn parses_suffixes_and_defaults() {
        assert_eq!(parse_length("10mil", Unit::Mm), Some(mil(10.0)));
        assert_eq!(parse_length(" 0.5 mm", Unit::Mil), Some(mm(0.5)));
        assert_eq!(parse_length("8", Unit::Mil), Some(mil(8.0)));
        assert_eq!(parse_length("0.1in", Unit::Mm), Some(mil(100.0)));
        assert_eq!(parse_length("abc", Unit::Mm), None);
        assert_eq!(parse_length("inf", Unit::Mm), None);
    }
}
