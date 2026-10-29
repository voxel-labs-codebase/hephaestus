//! Unit handling at the agent boundary: agents speak millimetres (or explicit unit strings), the
//! core speaks nanometres.

use hephaestus_core::geometry::{Coord, Point, Unit, parse_length};
use serde_json::{Value, json};

use crate::ApiError;

/// A length: a number in millimetres, or a string with units (`"10mil"`, `"0.25mm"`, `"0.1in"`).
pub fn length(v: &Value, what: &str) -> Result<Coord, ApiError> {
    match v {
        Value::Number(n) => {
            let mm = n
                .as_f64()
                .filter(|f| f.is_finite() && f.abs() <= 1.0e7)
                .ok_or_else(|| ApiError::invalid(format!("{what} is not a valid length")))?;
            Ok(Unit::Mm.to_nm(mm))
        }
        Value::String(s) => parse_length(s, Unit::Mm).ok_or_else(|| ApiError::invalid(format!("{what}: cannot parse length {s:?}"))),
        _ => Err(ApiError::invalid(format!("{what} must be a number (mm) or a string with units"))),
    }
}

pub fn opt_length(args: &Value, key: &str) -> Result<Option<Coord>, ApiError> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => length(v, key).map(Some),
    }
}

pub fn req_length(args: &Value, key: &str) -> Result<Coord, ApiError> {
    args.get(key).ok_or_else(|| ApiError::invalid(format!("missing {key}"))).and_then(|v| length(v, key))
}

/// A point: `{"x": 10, "y": 5}` or `[10, 5]`, each coordinate a length.
pub fn point(v: &Value, what: &str) -> Result<Point, ApiError> {
    match v {
        Value::Object(o) => {
            let x = o.get("x").ok_or_else(|| ApiError::invalid(format!("{what}.x missing")))?;
            let y = o.get("y").ok_or_else(|| ApiError::invalid(format!("{what}.y missing")))?;
            Ok(Point::new(length(x, what)?, length(y, what)?))
        }
        Value::Array(a) if a.len() == 2 => Ok(Point::new(length(&a[0], what)?, length(&a[1], what)?)),
        _ => Err(ApiError::invalid(format!("{what} must be {{\"x\":…, \"y\":…}} or [x, y]"))),
    }
}

pub fn points(v: Option<&Value>, what: &str) -> Result<Vec<Point>, ApiError> {
    let Some(Value::Array(items)) = v else { return Err(ApiError::invalid(format!("{what} must be an array of points"))) };
    if items.len() > 100_000 {
        return Err(ApiError::invalid(format!("{what} has too many points")));
    }
    items.iter().map(|p| point(p, what)).collect()
}

/// Nanometres to millimetres for output, rounded to the nanometre.
pub fn mm(nm: Coord) -> f64 {
    (nm as f64 / 1_000.0).round() / 1_000.0
}

pub fn pt(p: Point) -> Value {
    json!({"x": mm(p.x), "y": mm(p.y)})
}

#[cfg(test)]
mod tests {
    use super::*;
    use hephaestus_core::geometry::mil;

    #[test]
    fn numbers_are_millimetres_and_strings_carry_units() {
        assert_eq!(length(&json!(2.54), "x").unwrap(), mil(100.0));
        assert_eq!(length(&json!("100mil"), "x").unwrap(), mil(100.0));
        assert!(length(&json!("abc"), "x").is_err());
        assert!(length(&json!(true), "x").is_err());
        assert_eq!(point(&json!([1, 2]), "p").unwrap(), Point::new(1_000_000, 2_000_000));
        assert_eq!(pt(Point::new(1_500_000, -254_000)), json!({"x": 1.5, "y": -0.254}));
    }
}
