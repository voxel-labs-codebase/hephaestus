//! Engineering canvas infrastructure shared by the schematic and PCB editors.
//!
//! The canvas knows nothing about schematics or boards. It provides:
//! - [`Camera`]: world (integer nanometres, Y up) ↔ screen transforms, zoom about a point, fit;
//! - [`draw_grid`]: an adaptive line or dot grid that thins out instead of turning into noise;
//! - [`Navigation`]: Altium's mouse and keyboard navigation (right/middle-drag pan, Ctrl+wheel
//!   zoom, wheel scroll, Shift+wheel horizontal scroll, PgUp/PgDn zoom);
//! - [`SelectionBox`]: rubber-band selection with Altium's window (left-to-right, fully inside)
//!   and crossing (right-to-left, touching) modes;
//! - [`snap_to_grid`] and [`nearest_hotspot`] for grid and electrical snapping.
//!
//! Each editor owns its own interaction model on top of these pieces.

use egui::{Color32, Pos2, Rect, Stroke, Vec2};
use hephaestus_geometry::{Coord, Point};
use serde::{Deserialize, Serialize};

/// Smallest and largest zoom in screen pixels per nanometre (≈ 1 px/m … 1 px/10 nm).
const MIN_SCALE: f64 = 1.0e-9;
const MAX_SCALE: f64 = 0.1;

/// Maps world coordinates (nanometres, Y up) to a screen rectangle.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Camera {
    /// The world point at the centre of the viewport.
    pub center: (f64, f64),
    /// Screen points per nanometre.
    pub scale: f64,
}

impl Default for Camera {
    fn default() -> Self {
        Self { center: (0.0, 0.0), scale: 1.0e-5 }
    }
}

impl Camera {
    pub fn to_screen(&self, viewport: Rect, p: Point) -> Pos2 {
        self.to_screen_f(viewport, p.x as f64, p.y as f64)
    }

    pub fn to_screen_f(&self, viewport: Rect, x: f64, y: f64) -> Pos2 {
        let c = viewport.center();
        Pos2::new(c.x + ((x - self.center.0) * self.scale) as f32, c.y - ((y - self.center.1) * self.scale) as f32)
    }

    /// The world point under a screen position, unrounded.
    pub fn to_world_f(&self, viewport: Rect, pos: Pos2) -> (f64, f64) {
        let c = viewport.center();
        (self.center.0 + (pos.x - c.x) as f64 / self.scale, self.center.1 - (pos.y - c.y) as f64 / self.scale)
    }

    pub fn to_world(&self, viewport: Rect, pos: Pos2) -> Point {
        let (x, y) = self.to_world_f(viewport, pos);
        Point::new(x.round() as Coord, y.round() as Coord)
    }

    /// A world length in screen points.
    pub fn len(&self, nm: Coord) -> f32 {
        (nm as f64 * self.scale) as f32
    }

    /// The world rectangle visible in the viewport.
    pub fn visible(&self, viewport: Rect) -> hephaestus_geometry::Rect {
        let a = self.to_world(viewport, viewport.left_top());
        let b = self.to_world(viewport, viewport.right_bottom());
        hephaestus_geometry::Rect::from_corners(a, b)
    }

    /// Zoom by `factor` keeping the world point under `anchor` fixed.
    pub fn zoom_about(&mut self, viewport: Rect, anchor: Pos2, factor: f64) {
        if !factor.is_finite() || factor <= 0.0 {
            return;
        }
        let before = self.to_world_f(viewport, anchor);
        self.scale = (self.scale * factor).clamp(MIN_SCALE, MAX_SCALE);
        let after = self.to_world_f(viewport, anchor);
        self.center.0 += before.0 - after.0;
        self.center.1 += before.1 - after.1;
    }

    /// Pan by a screen-space drag.
    pub fn pan(&mut self, delta: Vec2) {
        self.center.0 -= delta.x as f64 / self.scale;
        self.center.1 += delta.y as f64 / self.scale;
    }

    /// Show `area` with `margin` (fraction of the viewport) around it.
    pub fn fit(&mut self, viewport: Rect, area: hephaestus_geometry::Rect, margin: f32) {
        let w = (area.width().max(1)) as f64;
        let h = (area.height().max(1)) as f64;
        let vw = (viewport.width() * (1.0 - 2.0 * margin)).max(10.0) as f64;
        let vh = (viewport.height() * (1.0 - 2.0 * margin)).max(10.0) as f64;
        self.scale = (vw / w).min(vh / h).clamp(MIN_SCALE, MAX_SCALE);
        let c = area.center();
        self.center = (c.x as f64, c.y as f64);
    }

    /// Centre on a point without changing zoom.
    pub fn center_on(&mut self, p: Point) {
        self.center = (p.x as f64, p.y as f64);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GridStyle {
    Lines,
    Dots,
}

/// Visual grid parameters.
#[derive(Clone, Copy, Debug)]
pub struct GridLook {
    pub style: GridStyle,
    pub minor: Color32,
    pub major: Color32,
    /// Every n-th line is drawn in the major colour.
    pub major_every: i64,
}

/// Draw a grid of pitch `spacing` (nanometres) limited to `clip` (screen). The pitch is
/// multiplied by `major_every` until lines are at least 6 points apart, so zoomed-out views stay
/// readable.
pub fn draw_grid(painter: &egui::Painter, camera: &Camera, viewport: Rect, clip: Rect, spacing: Coord, origin: Point, look: GridLook) {
    if spacing <= 0 {
        return;
    }
    let mut step = spacing;
    let mut every = look.major_every.max(2);
    while camera.len(step) < 6.0 {
        step = step.saturating_mul(look.major_every.max(2));
        every = look.major_every.max(2);
        if step > 1_000_000_000_000 {
            return;
        }
    }
    let world = camera.visible(viewport);
    let first = |min: Coord, o: Coord| ((min - o).div_euclid(step)) * step + o;
    let x0 = first(world.min.x.max(i64::MIN / 4), origin.x);
    let y0 = first(world.min.y.max(i64::MIN / 4), origin.y);
    let count_x = ((world.max.x - x0) / step).clamp(0, 2000);
    let count_y = ((world.max.y - y0) / step).clamp(0, 2000);
    let major = |v: Coord, o: Coord| ((v - o) / step) % every == 0;
    match look.style {
        GridStyle::Lines => {
            for i in 0..=count_x {
                let x = x0 + i * step;
                let sx = camera.to_screen(viewport, Point::new(x, 0)).x;
                let color = if major(x, origin.x) { look.major } else { look.minor };
                painter.line_segment([Pos2::new(sx, clip.top()), Pos2::new(sx, clip.bottom())], Stroke::new(1.0, color));
            }
            for j in 0..=count_y {
                let y = y0 + j * step;
                let sy = camera.to_screen(viewport, Point::new(0, y)).y;
                let color = if major(y, origin.y) { look.major } else { look.minor };
                painter.line_segment([Pos2::new(clip.left(), sy), Pos2::new(clip.right(), sy)], Stroke::new(1.0, color));
            }
        }
        GridStyle::Dots => {
            if count_x * count_y > 250_000 {
                return;
            }
            for i in 0..=count_x {
                let x = x0 + i * step;
                for j in 0..=count_y {
                    let y = y0 + j * step;
                    let p = camera.to_screen(viewport, Point::new(x, y));
                    if clip.contains(p) {
                        let m = major(x, origin.x) && major(y, origin.y);
                        painter.rect_filled(
                            Rect::from_center_size(p, Vec2::splat(if m { 2.0 } else { 1.0 })),
                            0.0,
                            if m { look.major } else { look.minor },
                        );
                    }
                }
            }
        }
    }
}

/// Snap a world point to the grid around `origin`.
pub fn snap_to_grid(p: Point, grid: Coord, origin: Point) -> Point {
    (p - origin).snapped(grid) + origin
}

/// The closest hotspot within `radius_px` screen points of `cursor`, for electrical snapping.
pub fn nearest_hotspot(
    camera: &Camera,
    viewport: Rect,
    cursor: Pos2,
    radius_px: f32,
    hotspots: impl IntoIterator<Item = Point>,
) -> Option<Point> {
    let mut best: Option<(f32, Point)> = None;
    for h in hotspots {
        let d = camera.to_screen(viewport, h).distance(cursor);
        if d <= radius_px && best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, h));
        }
    }
    best.map(|(_, p)| p)
}

/// Navigation preferences.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Navigation {
    /// Altium: the plain wheel scrolls and Ctrl+wheel zooms. When true the plain wheel zooms.
    pub wheel_zooms: bool,
    pub zoom_step: f64,
}

impl Default for Navigation {
    fn default() -> Self {
        Self { wheel_zooms: false, zoom_step: 1.25 }
    }
}

/// What a navigation pass did, so editors can suppress their own handling.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NavigationResult {
    pub panned: bool,
    pub zoomed: bool,
}

impl Navigation {
    /// Handle pan and zoom input for a canvas `response` covering `viewport`.
    pub fn handle(&self, ui: &egui::Ui, response: &egui::Response, camera: &mut Camera, viewport: Rect) -> NavigationResult {
        let mut result = NavigationResult::default();
        if response.dragged_by(egui::PointerButton::Secondary) || response.dragged_by(egui::PointerButton::Middle) {
            camera.pan(response.drag_delta());
            result.panned = true;
        }
        let hover = response.hover_pos();
        // Raw wheel events: egui's smoothed scroll would also turn Ctrl+wheel into its own zoom.
        let (scroll, pinch, modifiers) = ui.input(|i| {
            let mut scroll = Vec2::ZERO;
            let mut pinch = 1.0f32;
            for e in &i.events {
                match e {
                    egui::Event::MouseWheel { unit, delta, .. } => {
                        let k = match unit {
                            egui::MouseWheelUnit::Point => 1.0,
                            egui::MouseWheelUnit::Line => 50.0,
                            egui::MouseWheelUnit::Page => 400.0,
                        };
                        scroll += *delta * k;
                    }
                    egui::Event::Zoom(f) => pinch *= *f,
                    _ => {}
                }
            }
            (scroll, pinch, i.modifiers)
        });
        if let Some(pos) = hover {
            if (pinch - 1.0).abs() > f32::EPSILON {
                camera.zoom_about(viewport, pos, pinch as f64);
                result.zoomed = true;
            }
            if scroll != Vec2::ZERO {
                let amount = if scroll.y != 0.0 { scroll.y } else { scroll.x };
                if modifiers.ctrl || modifiers.command || self.wheel_zooms {
                    let factor = self.zoom_step.powf((amount / 50.0).clamp(-4.0, 4.0) as f64);
                    camera.zoom_about(viewport, pos, factor);
                    result.zoomed = true;
                } else if modifiers.shift {
                    camera.pan(Vec2::new(amount, 0.0));
                    result.panned = true;
                } else {
                    camera.pan(scroll);
                    result.panned = true;
                }
            }
            let (page_up, page_down) = ui.input(|i| (i.key_pressed(egui::Key::PageUp), i.key_pressed(egui::Key::PageDown)));
            if response.hovered() {
                if page_up && !modifiers.ctrl {
                    camera.zoom_about(viewport, pos, self.zoom_step * self.zoom_step);
                    result.zoomed = true;
                }
                if page_down && !modifiers.ctrl {
                    camera.zoom_about(viewport, pos, 1.0 / (self.zoom_step * self.zoom_step));
                    result.zoomed = true;
                }
            }
        }
        result
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionMode {
    /// Dragged left-to-right: objects fully inside are selected.
    Window,
    /// Dragged right-to-left: objects touching are selected.
    Crossing,
}

/// A rubber-band selection in progress.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SelectionBox {
    pub start: Pos2,
    pub end: Pos2,
}

impl SelectionBox {
    pub fn mode(&self) -> SelectionMode {
        if self.end.x >= self.start.x { SelectionMode::Window } else { SelectionMode::Crossing }
    }

    pub fn screen_rect(&self) -> Rect {
        Rect::from_two_pos(self.start, self.end)
    }

    pub fn world_rect(&self, camera: &Camera, viewport: Rect) -> hephaestus_geometry::Rect {
        hephaestus_geometry::Rect::from_corners(camera.to_world(viewport, self.start), camera.to_world(viewport, self.end))
    }

    /// Whether an object with `bounds` is selected by this box.
    pub fn selects(&self, camera: &Camera, viewport: Rect, bounds: &hephaestus_geometry::Rect) -> bool {
        let area = self.world_rect(camera, viewport);
        match self.mode() {
            SelectionMode::Window => area.contains_rect(bounds),
            SelectionMode::Crossing => area.intersects(bounds),
        }
    }

    /// Altium draws window selection solid and crossing selection dashed.
    pub fn paint(&self, painter: &egui::Painter, window: Color32, crossing: Color32) {
        let r = self.screen_rect();
        match self.mode() {
            SelectionMode::Window => {
                painter.rect_filled(r, 0.0, window.gamma_multiply(0.12));
                painter.rect_stroke(r, 0.0, Stroke::new(1.0, window), egui::StrokeKind::Inside);
            }
            SelectionMode::Crossing => {
                painter.rect_filled(r, 0.0, crossing.gamma_multiply(0.12));
                let pts = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
                painter.extend(egui::Shape::dashed_line(&pts, Stroke::new(1.0, crossing), 4.0, 3.0));
            }
        }
    }
}

/// A small cross cursor at a snapped world point (Altium's "Small 90" cursor).
pub fn paint_cursor(painter: &egui::Painter, at: Pos2, size: f32, color: Color32) {
    painter.line_segment([at - Vec2::new(size, 0.0), at + Vec2::new(size, 0.0)], Stroke::new(1.0, color));
    painter.line_segment([at - Vec2::new(0.0, size), at + Vec2::new(0.0, size)], Stroke::new(1.0, color));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn viewport() -> Rect {
        Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0))
    }

    #[test]
    fn world_screen_round_trip_and_y_up() {
        let cam = Camera { center: (1000.0, 1000.0), scale: 0.01 };
        let s = cam.to_screen(viewport(), Point::new(2000, 2000));
        assert_eq!(s, Pos2::new(410.0, 290.0));
        assert_eq!(cam.to_world(viewport(), s), Point::new(2000, 2000));
    }

    #[test]
    fn zoom_keeps_anchor_fixed() {
        let mut cam = Camera { center: (0.0, 0.0), scale: 0.01 };
        let anchor = Pos2::new(600.0, 100.0);
        let before = cam.to_world(viewport(), anchor);
        cam.zoom_about(viewport(), anchor, 2.0);
        assert_eq!(cam.to_world(viewport(), anchor), before);
        assert_eq!(cam.scale, 0.02);
    }

    #[test]
    fn fit_shows_the_whole_area() {
        let mut cam = Camera::default();
        let area = hephaestus_geometry::Rect::from_corners(Point::new(0, 0), Point::new(1_000_000, 500_000));
        cam.fit(viewport(), area, 0.05);
        let tl = cam.to_screen(viewport(), Point::new(0, 500_000));
        let br = cam.to_screen(viewport(), Point::new(1_000_000, 0));
        assert!(viewport().contains(tl) && viewport().contains(br));
    }

    #[test]
    fn selection_modes_follow_drag_direction() {
        let cam = Camera { center: (0.0, 0.0), scale: 1.0 };
        let window = SelectionBox { start: Pos2::new(390.0, 290.0), end: Pos2::new(410.0, 310.0) };
        let crossing = SelectionBox { start: window.end, end: window.start };
        let inside = hephaestus_geometry::Rect::from_center(Point::ZERO, 5, 5);
        let straddling = hephaestus_geometry::Rect::from_center(Point::new(10, 0), 5, 5);
        assert!(window.selects(&cam, viewport(), &inside));
        assert!(!window.selects(&cam, viewport(), &straddling));
        assert!(crossing.selects(&cam, viewport(), &straddling));
    }

    #[test]
    fn hotspot_snapping_picks_the_closest_in_range() {
        let cam = Camera { center: (0.0, 0.0), scale: 1.0 };
        let hs = [Point::new(5, 0), Point::new(-3, 0), Point::new(100, 0)];
        assert_eq!(nearest_hotspot(&cam, viewport(), Pos2::new(400.0, 300.0), 8.0, hs), Some(Point::new(-3, 0)));
        assert_eq!(nearest_hotspot(&cam, viewport(), Pos2::new(600.0, 300.0), 8.0, hs), None);
    }
}
