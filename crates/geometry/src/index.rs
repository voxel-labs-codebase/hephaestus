use std::collections::HashMap;

use crate::{Coord, Point, Rect};

/// Most cells one item may occupy; larger items go to an overflow list checked on every query.
const MAX_CELLS_PER_ITEM: i64 = 256;

/// A uniform-grid spatial hash for hit testing and clearance queries.
///
/// Built once per design revision and queried many times per frame. Items are identified by the
/// caller's key; queries may return an item more than once if it spans several cells, so callers
/// that care deduplicate (see [`GridIndex::query_unique`]).
#[derive(Clone, Debug)]
pub struct GridIndex<K> {
    cell: Coord,
    cells: HashMap<(i64, i64), Vec<usize>>,
    large: Vec<usize>,
    items: Vec<(K, Rect)>,
}

impl<K: Clone + PartialEq> GridIndex<K> {
    /// `cell` is the grid pitch; pick something near the typical object size.
    pub fn new(cell: Coord) -> Self {
        Self { cell: cell.max(1), cells: HashMap::new(), large: Vec::new(), items: Vec::new() }
    }

    fn cell_of(&self, p: Point) -> (i64, i64) {
        (p.x.div_euclid(self.cell), p.y.div_euclid(self.cell))
    }

    pub fn insert(&mut self, key: K, bounds: Rect) {
        let index = self.items.len();
        self.items.push((key, bounds));
        let (x0, y0) = self.cell_of(bounds.min);
        let (x1, y1) = self.cell_of(bounds.max);
        if (x1 - x0 + 1).saturating_mul(y1 - y0 + 1) > MAX_CELLS_PER_ITEM {
            self.large.push(index);
            return;
        }
        for x in x0..=x1 {
            for y in y0..=y1 {
                self.cells.entry((x, y)).or_default().push(index);
            }
        }
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Items whose bounds intersect `area`, each once.
    pub fn query_unique(&self, area: Rect) -> Vec<&K> {
        let mut hits: Vec<usize> = Vec::new();
        let (x0, y0) = self.cell_of(area.min);
        let (x1, y1) = self.cell_of(area.max);
        let span = (x1 - x0 + 1).saturating_mul(y1 - y0 + 1);
        if span > (self.cells.len() as i64).max(1) * 4 {
            // A huge query: scanning everything is cheaper than walking empty cells.
            hits.extend(0..self.items.len());
        } else {
            for x in x0..=x1 {
                for y in y0..=y1 {
                    if let Some(list) = self.cells.get(&(x, y)) {
                        hits.extend_from_slice(list);
                    }
                }
            }
            hits.extend_from_slice(&self.large);
        }
        hits.sort_unstable();
        hits.dedup();
        hits.into_iter().filter_map(|i| self.items.get(i)).filter(|(_, b)| b.intersects(&area)).map(|(k, _)| k).collect()
    }

    pub fn query_point(&self, p: Point, tolerance: Coord) -> Vec<&K> {
        self.query_unique(Rect::from_center(p, tolerance, tolerance))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_items_by_area_once() {
        let mut index = GridIndex::new(100);
        index.insert("a", Rect::from_corners(Point::new(0, 0), Point::new(250, 250)));
        index.insert("b", Rect::from_corners(Point::new(1000, 1000), Point::new(1010, 1010)));
        index.insert("huge", Rect::from_corners(Point::new(-1_000_000, -1_000_000), Point::new(1_000_000, 1_000_000)));
        let hits = index.query_unique(Rect::from_corners(Point::new(0, 0), Point::new(300, 300)));
        assert_eq!(hits, vec![&"a", &"huge"]);
        let hits = index.query_point(Point::new(1005, 1005), 1);
        assert_eq!(hits, vec![&"b", &"huge"]);
        assert_eq!(index.query_unique(Rect::from_corners(Point::new(-1_000_000, -1_000_000), Point::new(1_000_000, 1_000_000))).len(), 3);
    }
}
