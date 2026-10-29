//! Rubber-banding: wires and tracks that stay attached to objects as they move, rotate or mirror.
//!
//! The command planner applies these when it commits an edit, and the editors call the same
//! functions every frame to preview the result while the user drags.

use std::collections::{BTreeMap, BTreeSet};

use hephaestus_geometry::Point;

use crate::schematic::{BusEntry, Junction, NetLabel, NoConnect, Port, PowerPort, Wire};
use crate::{Design, ObjectId, PcbKind, PcbObject, SchKind, SchObject};

/// Electrical connection points of a schematic object, keyed so the same point can be found
/// again after the object is transformed (pins by number, wires by end).
pub fn connection_points(design: &Design, o: &SchObject) -> Vec<(String, Point)> {
    match &o.kind {
        SchKind::Symbol(_) => design.pins(o).into_iter().map(|p| (p.number, p.position)).collect(),
        SchKind::Wire(Wire { points }) => match (points.first(), points.last()) {
            (Some(a), Some(b)) => vec![("first".into(), *a), ("last".into(), *b)],
            _ => Vec::new(),
        },
        SchKind::BusEntry(BusEntry { start, end }) => vec![("start".into(), *start), ("end".into(), *end)],
        SchKind::Junction(Junction { position })
        | SchKind::NetLabel(NetLabel { position, .. })
        | SchKind::PowerPort(PowerPort { position, .. })
        | SchKind::Port(Port { position, .. })
        | SchKind::NoConnect(NoConnect { position }) => vec![("anchor".into(), *position)],
        _ => Vec::new(),
    }
}

/// Where each connection point of `before` went in `after` (matched by object id). Points that
/// did not move are left out.
pub fn point_moves(design: &Design, before: &[SchObject], after: &[SchObject]) -> BTreeMap<Point, Point> {
    let mut moves = BTreeMap::new();
    for b in before {
        let Some(a) = after.iter().find(|a| a.id == b.id) else { continue };
        let new: BTreeMap<String, Point> = connection_points(design, a).into_iter().collect();
        for (key, old) in connection_points(design, b) {
            if let Some(&to) = new.get(&key)
                && to != old
            {
                moves.entry(old).or_insert(to);
            }
        }
    }
    moves
}

/// Wires on `sheet` (other than `moved`) whose ends sit on a moving point, reshaped to follow.
pub fn follow_wires(design: &Design, sheet: ObjectId, moves: &BTreeMap<Point, Point>, moved: &BTreeSet<ObjectId>) -> Vec<SchObject> {
    if moves.is_empty() {
        return Vec::new();
    }
    // Vertices other objects attach to must not shift when a wire is straightened.
    let mut fixed: BTreeSet<Point> = BTreeSet::new();
    for o in design.sheet_objects(sheet).filter(|o| !moved.contains(&o.id)) {
        fixed.extend(connection_points(design, o).into_iter().map(|(_, p)| p));
    }
    design
        .sheet_objects(sheet)
        .filter(|o| !moved.contains(&o.id))
        .filter_map(|o| {
            let SchKind::Wire(w) = &o.kind else { return None };
            let points = reshape(&w.points, moves, &fixed)?;
            let mut out = o.clone();
            out.kind = SchKind::Wire(Wire { points });
            Some(out)
        })
        .collect()
}

/// A wire's points after its ends follow `moves`, keeping horizontal and vertical runs
/// orthogonal; `None` when neither end moves.
pub fn reshape(points: &[Point], moves: &BTreeMap<Point, Point>, fixed: &BTreeSet<Point>) -> Option<Vec<Point>> {
    let (&first, &last) = (points.first()?, points.last()?);
    if points.len() < 2 {
        return None;
    }
    let (to_first, to_last) = (moves.get(&first).copied(), moves.get(&last).copied());
    if to_first.is_none() && to_last.is_none() {
        return None;
    }
    // Both ends carried by the same offset: the whole wire travels with them.
    if let (Some(a), Some(b)) = (to_first, to_last)
        && a - first == b - last
    {
        return Some(points.iter().map(|p| *p + (a - first)).collect());
    }
    let mut pts = points.to_vec();
    if let Some(to) = to_first {
        move_end(&mut pts, to, fixed);
    }
    if let Some(to) = to_last {
        pts.reverse();
        move_end(&mut pts, to, fixed);
        pts.reverse();
    }
    Some(simplify(&pts))
}

/// Move `pts[0]` to `to`, bending the wire so orthogonal runs stay orthogonal.
fn move_end(pts: &mut Vec<Point>, to: Point, fixed: &BTreeSet<Point>) {
    let (p0, p1) = (pts[0], pts[1]);
    if p0 == to {
        return;
    }
    let horizontal = p0.y == p1.y;
    let vertical = p0.x == p1.x;
    if !(horizontal || vertical) {
        pts[0] = to;
        return;
    }
    // Slide the next corner along with the end when the following run is perpendicular.
    if let Some(&p2) = pts.get(2)
        && !fixed.contains(&p1)
    {
        if horizontal && p2.x == p1.x {
            pts[1] = Point::new(p1.x, to.y);
            pts[0] = to;
            return;
        }
        if vertical && p2.y == p1.y {
            pts[1] = Point::new(to.x, p1.y);
            pts[0] = to;
            return;
        }
    }
    // Otherwise leave the end in its original direction and add a corner.
    let corner = if horizontal { Point::new(p1.x, to.y) } else { Point::new(to.x, p1.y) };
    pts[0] = corner;
    pts.insert(0, to);
}

/// Drop repeated points and vertices in the middle of a straight run.
pub fn simplify(points: &[Point]) -> Vec<Point> {
    let mut out: Vec<Point> = Vec::new();
    for &p in points {
        if out.last() == Some(&p) {
            continue;
        }
        if out.len() >= 2 {
            let (a, b) = (out[out.len() - 2], out[out.len() - 1]);
            let cross = (b.x - a.x) as i128 * (p.y - a.y) as i128 - (b.y - a.y) as i128 * (p.x - a.x) as i128;
            let ahead = (b.x - a.x) as i128 * (p.x - b.x) as i128 + (b.y - a.y) as i128 * (p.y - b.y) as i128;
            if cross == 0 && ahead >= 0 {
                out.pop();
            }
        }
        out.push(p);
    }
    if out.len() == 1 {
        out.push(out[0]);
    }
    out
}

/// Where each pad centre of `before` went in `after` (matched by footprint id and pad number).
pub fn pad_moves(design: &Design, before: &[PcbObject], after: &[PcbObject]) -> BTreeMap<Point, Point> {
    let mut moves = BTreeMap::new();
    for b in before {
        let Some(a) = after.iter().find(|a| a.id == b.id) else { continue };
        let new: BTreeMap<String, Point> = design.pads(a).into_iter().map(|p| (p.number, p.center)).collect();
        for pad in design.pads(b) {
            if let Some(&to) = new.get(&pad.number)
                && to != pad.center
            {
                moves.entry(pad.center).or_insert(to);
            }
        }
    }
    moves
}

/// Tracks (other than `moved`) with an end on a moving pad, stretched to follow it.
pub fn follow_tracks(design: &Design, moves: &BTreeMap<Point, Point>, moved: &BTreeSet<ObjectId>) -> Vec<PcbObject> {
    if moves.is_empty() {
        return Vec::new();
    }
    design
        .pcb
        .values()
        .filter(|o| !moved.contains(&o.id) && !o.is_locked())
        .filter_map(|o| {
            let PcbKind::Track(t) = &o.kind else { return None };
            let (start, end) = (moves.get(&t.start).copied(), moves.get(&t.end).copied());
            if start.is_none() && end.is_none() {
                return None;
            }
            let mut out = o.clone();
            if let PcbKind::Track(t) = &mut out.kind {
                t.start = start.unwrap_or(t.start);
                t.end = end.unwrap_or(t.end);
            }
            Some(out)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: i64, y: i64) -> Point {
        Point::new(x, y)
    }

    #[test]
    fn end_slides_corner_to_stay_orthogonal() {
        let wire = [p(0, 0), p(10, 0), p(10, 20)];
        let moves = BTreeMap::from([(p(0, 0), p(0, 5))]);
        assert_eq!(reshape(&wire, &moves, &BTreeSet::new()), Some(vec![p(0, 5), p(10, 5), p(10, 20)]));
    }

    #[test]
    fn straight_wire_gets_a_corner() {
        let wire = [p(0, 0), p(10, 0)];
        let moves = BTreeMap::from([(p(0, 0), p(0, 5))]);
        assert_eq!(reshape(&wire, &moves, &BTreeSet::new()), Some(vec![p(0, 5), p(10, 5), p(10, 0)]));
    }

    #[test]
    fn fixed_corner_is_not_moved() {
        let wire = [p(0, 0), p(10, 0), p(10, 20)];
        let moves = BTreeMap::from([(p(0, 0), p(0, 5))]);
        let fixed = BTreeSet::from([p(10, 0)]);
        assert_eq!(reshape(&wire, &moves, &fixed), Some(vec![p(0, 5), p(10, 5), p(10, 0), p(10, 20)]));
    }

    #[test]
    fn both_ends_moving_together_translate() {
        let wire = [p(0, 0), p(5, 0), p(5, 5)];
        let moves = BTreeMap::from([(p(0, 0), p(1, 1)), (p(5, 5), p(6, 6))]);
        assert_eq!(reshape(&wire, &moves, &BTreeSet::new()), Some(vec![p(1, 1), p(6, 1), p(6, 6)]));
    }

    #[test]
    fn untouched_wire_is_none() {
        assert_eq!(reshape(&[p(0, 0), p(1, 0)], &BTreeMap::from([(p(9, 9), p(0, 0))]), &BTreeSet::new()), None);
    }
}
