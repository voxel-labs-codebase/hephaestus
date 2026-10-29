//! The board: outline, layer stack, design rules and placed PCB objects.

use std::collections::BTreeMap;

use hephaestus_geometry::{Coord, Point, Rect, mil, mm};
use serde::{Deserialize, Serialize};

use crate::{Layer, ObjectId};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Side {
    #[default]
    Top,
    Bottom,
}

/// One copper layer of the stack and the dielectric below it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StackLayer {
    pub layer: Layer,
    pub name: String,
    /// Copper thickness in nanometres (35 µm = 1 oz).
    pub copper_thickness: Coord,
    /// Dielectric below this layer (absent under the bottom layer).
    #[serde(default)]
    pub dielectric: Option<Dielectric>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Dielectric {
    pub material: String,
    pub thickness: Coord,
    pub constant: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LayerStack {
    pub layers: Vec<StackLayer>,
}

impl LayerStack {
    /// A conventional FR-4 stack with `copper` layers and 1.6 mm total thickness.
    pub fn standard(copper: u8) -> Self {
        let stack = Layer::copper_stack(copper.max(2));
        let count = stack.len();
        let gap = mm(1.6) / (count as Coord - 1).max(1);
        let layers = stack
            .into_iter()
            .enumerate()
            .map(|(i, layer)| StackLayer {
                layer,
                name: layer.to_string(),
                copper_thickness: mm(0.035),
                dielectric: (i + 1 < count).then(|| Dielectric {
                    material: if i == 0 || i + 2 == count { "FR-4 Prepreg".into() } else { "FR-4 Core".into() },
                    thickness: gap,
                    constant: 4.2,
                }),
            })
            .collect();
        Self { layers }
    }

    pub fn copper_count(&self) -> u8 {
        self.layers.len().min(32) as u8
    }

    pub fn copper_layers(&self) -> Vec<Layer> {
        self.layers.iter().map(|l| l.layer).collect()
    }

    pub fn contains(&self, layer: Layer) -> bool {
        self.layers.iter().any(|l| l.layer == layer)
    }

    pub fn total_thickness(&self) -> Coord {
        self.layers.iter().map(|l| l.copper_thickness + l.dielectric.as_ref().map_or(0, |d| d.thickness)).sum()
    }
}

/// A named group of nets with its own routing widths and clearance.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NetClass {
    pub name: String,
    pub nets: Vec<String>,
    pub track_width: Coord,
    pub clearance: Coord,
}

/// Board-wide design rules (Altium's Design › Rules, simplified to the common set).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DesignRules {
    pub clearance: Coord,
    pub min_track_width: Coord,
    pub preferred_track_width: Coord,
    pub max_track_width: Coord,
    pub min_via_diameter: Coord,
    pub min_via_drill: Coord,
    pub preferred_via_diameter: Coord,
    pub preferred_via_drill: Coord,
    pub min_annular_ring: Coord,
    pub board_edge_clearance: Coord,
    pub min_hole_size: Coord,
    pub max_hole_size: Coord,
    #[serde(default)]
    pub net_classes: Vec<NetClass>,
}

impl Default for DesignRules {
    fn default() -> Self {
        Self {
            clearance: mil(6.0),
            min_track_width: mil(6.0),
            preferred_track_width: mil(10.0),
            max_track_width: mil(200.0),
            min_via_diameter: mil(20.0),
            min_via_drill: mil(10.0),
            preferred_via_diameter: mil(28.0),
            preferred_via_drill: mil(14.0),
            min_annular_ring: mil(5.0),
            board_edge_clearance: mil(10.0),
            min_hole_size: mil(10.0),
            max_hole_size: mil(250.0),
            net_classes: Vec::new(),
        }
    }
}

impl DesignRules {
    pub fn net_class(&self, net: &str) -> Option<&NetClass> {
        self.net_classes.iter().find(|c| c.nets.iter().any(|n| n == net))
    }

    /// The clearance between two nets: the larger of the classes' clearances and the default.
    pub fn clearance_between(&self, a: Option<&str>, b: Option<&str>) -> Coord {
        let class = |n: Option<&str>| n.and_then(|n| self.net_class(n)).map_or(0, |c| c.clearance);
        self.clearance.max(class(a)).max(class(b))
    }

    pub fn track_width_for(&self, net: Option<&str>) -> Coord {
        net.and_then(|n| self.net_class(n)).map_or(self.preferred_track_width, |c| c.track_width)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BoardSettings {
    /// Board shape as a closed polygon (counter-clockwise), Altium's Board Shape.
    pub outline: Vec<Point>,
    /// Relative origin shown in coordinates (Edit › Origin › Set).
    #[serde(default)]
    pub origin: Point,
    pub stack: LayerStack,
    pub rules: DesignRules,
}

impl Default for BoardSettings {
    fn default() -> Self {
        let (w, h) = (mil(3000.0), mil(2000.0));
        Self {
            outline: vec![Point::new(0, 0), Point::new(w, 0), Point::new(w, h), Point::new(0, h)],
            origin: Point::ZERO,
            stack: LayerStack::standard(2),
            rules: DesignRules::default(),
        }
    }
}

impl BoardSettings {
    pub fn outline_bounds(&self) -> Option<Rect> {
        Rect::bounding(self.outline.iter().copied())
    }
}

/// A placed footprint, the PCB view of a [`crate::Component`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Footprint {
    /// The logical component; `None` for mechanical parts (mounting holes, fiducials).
    pub component: Option<ObjectId>,
    pub footprint: String,
    pub position: Point,
    #[serde(default)]
    pub rotation: f64,
    #[serde(default)]
    pub side: Side,
    #[serde(default)]
    pub locked: bool,
    /// Pad number to net name, assigned by Update PCB (and by agents). Pads absent here are
    /// unconnected.
    #[serde(default)]
    pub pad_nets: BTreeMap<String, String>,
}

/// A straight copper track segment.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub start: Point,
    pub end: Point,
    pub width: Coord,
    pub layer: Layer,
    #[serde(default)]
    pub net: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Via {
    pub position: Point,
    pub diameter: Coord,
    pub drill: Coord,
    #[serde(default)]
    pub net: Option<String>,
    /// Through vias span the full stack; blind/buried vias name their end layers.
    pub from: Layer,
    pub to: Layer,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ZoneFillMode {
    #[default]
    Solid,
    Hatched,
    /// Outline only — the pour is not filled.
    None,
}

/// A copper pour (Altium's Polygon Pour) on one layer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Zone {
    pub layer: Layer,
    #[serde(default)]
    pub net: Option<String>,
    pub outline: Vec<Point>,
    #[serde(default)]
    pub priority: u32,
    /// Overrides the rules clearance for this pour.
    #[serde(default)]
    pub clearance: Option<Coord>,
    #[serde(default)]
    pub fill_mode: ZoneFillMode,
    /// Poured copper islands computed by a zone fill. Empty until the pour is filled; Hephaestus
    /// does not yet compute fills (see ROADMAP), so this is preserved from imports only.
    #[serde(default)]
    pub filled: Vec<Vec<Point>>,
}

/// A region where routing or placement is forbidden.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Keepout {
    /// Copper layers the keep-out applies to; empty means all.
    #[serde(default)]
    pub layers: Vec<Layer>,
    pub outline: Vec<Point>,
    #[serde(default = "yes")]
    pub no_tracks: bool,
    #[serde(default = "yes")]
    pub no_vias: bool,
    #[serde(default = "yes")]
    pub no_pads: bool,
    #[serde(default = "yes")]
    pub no_pours: bool,
}

fn yes() -> bool {
    true
}

impl Keepout {
    pub fn applies_to(&self, layer: Layer) -> bool {
        self.layers.is_empty() || self.layers.contains(&layer) || layer == Layer::MultiLayer
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PcbText {
    pub layer: Layer,
    pub position: Point,
    pub text: String,
    pub height: Coord,
    #[serde(default)]
    pub rotation: f64,
}

/// A straight line on a non-copper layer (overlay, mechanical, keep-out).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PcbLine {
    pub layer: Layer,
    pub start: Point,
    pub end: Point,
    pub width: Coord,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum PcbKind {
    Footprint(Footprint),
    Track(Track),
    Via(Via),
    Zone(Zone),
    Keepout(Keepout),
    Text(PcbText),
    Line(PcbLine),
}

impl PcbKind {
    pub fn type_name(&self) -> &'static str {
        match self {
            PcbKind::Footprint(_) => "Component",
            PcbKind::Track(_) => "Track",
            PcbKind::Via(_) => "Via",
            PcbKind::Zone(_) => "Polygon Pour",
            PcbKind::Keepout(_) => "Keepout Region",
            PcbKind::Text(_) => "Text",
            PcbKind::Line(_) => "Line",
        }
    }

    pub fn net(&self) -> Option<&str> {
        match self {
            PcbKind::Track(t) => t.net.as_deref(),
            PcbKind::Via(v) => v.net.as_deref(),
            PcbKind::Zone(z) => z.net.as_deref(),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PcbObject {
    pub id: ObjectId,
    #[serde(flatten)]
    pub kind: PcbKind,
}

impl PcbObject {
    pub fn translate(&mut self, by: Point) {
        match &mut self.kind {
            PcbKind::Footprint(f) => f.position = f.position + by,
            PcbKind::Track(t) => {
                t.start = t.start + by;
                t.end = t.end + by;
            }
            PcbKind::Via(v) => v.position = v.position + by,
            PcbKind::Zone(Zone { outline, filled, .. }) => {
                outline.iter_mut().for_each(|p| *p = *p + by);
                filled.iter_mut().flatten().for_each(|p| *p = *p + by);
            }
            PcbKind::Keepout(k) => k.outline.iter_mut().for_each(|p| *p = *p + by),
            PcbKind::Text(t) => t.position = t.position + by,
            PcbKind::Line(l) => {
                l.start = l.start + by;
                l.end = l.end + by;
            }
        }
    }

    pub fn rotate_about(&mut self, pivot: Point, degrees: f64) {
        let turn = |p: Point| (p - pivot).rotated(degrees) + pivot;
        match &mut self.kind {
            PcbKind::Footprint(f) => {
                f.position = turn(f.position);
                f.rotation = (f.rotation + degrees).rem_euclid(360.0);
            }
            PcbKind::Track(t) => {
                t.start = turn(t.start);
                t.end = turn(t.end);
            }
            PcbKind::Via(v) => v.position = turn(v.position),
            PcbKind::Zone(Zone { outline, filled, .. }) => {
                outline.iter_mut().for_each(|p| *p = turn(*p));
                filled.iter_mut().flatten().for_each(|p| *p = turn(*p));
            }
            PcbKind::Keepout(k) => k.outline.iter_mut().for_each(|p| *p = turn(*p)),
            PcbKind::Text(t) => {
                t.position = turn(t.position);
                t.rotation = (t.rotation + degrees).rem_euclid(360.0);
            }
            PcbKind::Line(l) => {
                l.start = turn(l.start);
                l.end = turn(l.end);
            }
        }
    }

    pub fn anchor(&self) -> Point {
        match &self.kind {
            PcbKind::Footprint(f) => f.position,
            PcbKind::Track(t) => Point::new((t.start.x + t.end.x) / 2, (t.start.y + t.end.y) / 2),
            PcbKind::Via(v) => v.position,
            PcbKind::Zone(Zone { outline, .. }) | PcbKind::Keepout(Keepout { outline, .. }) => {
                Rect::bounding(outline.iter().copied()).map(|r| r.center()).unwrap_or_default()
            }
            PcbKind::Text(t) => t.position,
            PcbKind::Line(l) => Point::new((l.start.x + l.end.x) / 2, (l.start.y + l.end.y) / 2),
        }
    }

    pub fn is_locked(&self) -> bool {
        matches!(&self.kind, PcbKind::Footprint(f) if f.locked)
    }
}
