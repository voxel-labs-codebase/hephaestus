//! Dockable panels and persisted workspace preferences.

use std::collections::BTreeSet;
use std::path::PathBuf;

use craft_ui::docking::{Layout, Node, Permissions};
use craft_ui::layout::{SplitAxis, SplitSize};
use hephaestus_canvas::{GridStyle, Navigation};
use hephaestus_core::Layer;
use hephaestus_core::geometry::{Coord, Unit, mil};
use serde::{Deserialize, Serialize};

use crate::icons::Icon;

/// Every dockable panel. `Documents` is the protected centre (tabs and canvases).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum PanelId {
    Documents,
    Projects,
    Components,
    Navigator,
    Properties,
    Layers,
    Nets,
    Messages,
    Output,
    Agents,
}

impl PanelId {
    pub const ALL: [PanelId; 9] = [
        PanelId::Projects,
        PanelId::Components,
        PanelId::Navigator,
        PanelId::Properties,
        PanelId::Layers,
        PanelId::Nets,
        PanelId::Messages,
        PanelId::Output,
        PanelId::Agents,
    ];

    pub fn title(self) -> &'static str {
        match self {
            PanelId::Documents => "Documents",
            PanelId::Projects => "Projects",
            PanelId::Components => "Components",
            PanelId::Navigator => "Navigator",
            PanelId::Properties => "Properties",
            PanelId::Layers => "View Configuration",
            PanelId::Nets => "PCB",
            PanelId::Messages => "Messages",
            PanelId::Output => "Output",
            PanelId::Agents => "Agents",
        }
    }

    pub fn icon(self) -> Icon {
        match self {
            PanelId::Documents => Icon::Schematic,
            PanelId::Projects => Icon::Project,
            PanelId::Components => Icon::Component,
            PanelId::Navigator => Icon::Search,
            PanelId::Properties => Icon::Settings,
            PanelId::Layers => Icon::Layers,
            PanelId::Nets => Icon::Net,
            PanelId::Messages => Icon::Warning,
            PanelId::Output => Icon::Info,
            PanelId::Agents => Icon::Agent,
        }
    }

    pub fn permissions(self) -> Permissions {
        if self == PanelId::Documents { Permissions::PROTECTED } else { Permissions::default() }
    }
}

fn tabs(panels: &[PanelId]) -> Node<PanelId> {
    Node::Tabs { panels: panels.to_vec(), active: 0 }
}

fn split(axis: SplitAxis, size: SplitSize, first: Node<PanelId>, second: Node<PanelId>) -> Node<PanelId> {
    Node::Split { axis, size, first: Box::new(first), second: Box::new(second) }
}

/// Altium's default arrangement: Projects/Components/Navigator on the left, Properties and the
/// View Configuration on the right, Messages under the documents.
pub fn default_layout() -> Layout<PanelId> {
    let center = split(
        SplitAxis::Vertical,
        SplitSize::FixedSecond(170.0),
        tabs(&[PanelId::Documents]),
        tabs(&[PanelId::Messages, PanelId::Output, PanelId::Agents]),
    );
    let right = tabs(&[PanelId::Properties, PanelId::Layers, PanelId::Nets]);
    let left = tabs(&[PanelId::Projects, PanelId::Components, PanelId::Navigator]);
    let inner = split(SplitAxis::Horizontal, SplitSize::FixedSecond(290.0), center, right);
    Layout { root: Some(split(SplitAxis::Horizontal, SplitSize::FixedFirst(250.0), left, inner)), floating: Vec::new() }
}

/// Preferences that persist between sessions.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub layout: Layout<PanelId>,
    pub sch_unit: Unit,
    pub pcb_unit: Unit,
    pub sch_grid: Coord,
    pub pcb_grid: Coord,
    pub pcb_grid_style: GridStyle,
    pub sch_grid_visible: bool,
    pub layer_colors: Vec<(Layer, [u8; 3])>,
    pub hidden_layers: BTreeSet<Layer>,
    pub show_ratsnest: bool,
    pub show_pad_numbers: bool,
    pub show_net_names: bool,
    pub navigation: Navigation,
    pub cross_select: bool,
    pub recent: Vec<PathBuf>,
    pub autosave_seconds: u64,
    pub control_port: u16,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            layout: default_layout(),
            sch_unit: Unit::Mil,
            pcb_unit: Unit::Mil,
            sch_grid: mil(100.0),
            pcb_grid: mil(5.0),
            pcb_grid_style: GridStyle::Lines,
            sch_grid_visible: true,
            layer_colors: Vec::new(),
            hidden_layers: [
                Layer::TopPaste,
                Layer::BottomPaste,
                Layer::TopSolder,
                Layer::BottomSolder,
                Layer::DrillGuide,
                Layer::DrillDrawing,
                Layer::Mechanical(13),
            ]
            .into(),
            show_ratsnest: true,
            show_pad_numbers: true,
            show_net_names: true,
            navigation: Navigation::default(),
            cross_select: true,
            recent: Vec::new(),
            autosave_seconds: 120,
            control_port: hephaestus_agent_api::control::DEFAULT_PORT,
        }
    }
}

impl Prefs {
    pub fn layer_color(&self, layer: Layer) -> egui::Color32 {
        self.layer_colors
            .iter()
            .find(|(l, _)| *l == layer)
            .map(|(_, c)| egui::Color32::from_rgb(c[0], c[1], c[2]))
            .unwrap_or_else(|| crate::theme::Theme::default_layer_color(layer))
    }

    pub fn set_layer_color(&mut self, layer: Layer, color: egui::Color32) {
        self.layer_colors.retain(|(l, _)| *l != layer);
        self.layer_colors.push((layer, [color.r(), color.g(), color.b()]));
    }

    pub fn layer_visible(&self, layer: Layer) -> bool {
        !self.hidden_layers.contains(&layer)
    }

    pub fn remember_recent(&mut self, path: PathBuf) {
        self.recent.retain(|p| p != &path);
        self.recent.insert(0, path);
        self.recent.truncate(10);
    }

    /// Make sure every panel is reachable after loading an older layout.
    pub fn sanitize(&mut self) {
        if self.layout.validate().is_err() || !self.layout.contains(&PanelId::Documents) {
            self.layout = default_layout();
        }
    }
}
