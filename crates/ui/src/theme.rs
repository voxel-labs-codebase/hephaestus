//! Colours, metrics and the egui style. Widgets take colours from [`Theme`], never literals.
//!
//! The chrome follows Altium Designer's dark theme: near-black document area, dark grey panels
//! with thin borders, compact 20–22 px rows, a restrained blue for selection and Altium's amber
//! for the active tool and document. Schematic sheets stay light (Altium draws sheets on white
//! paper even in the dark theme) and PCB layers use Altium's default layer colours.

use egui::{Color32, CornerRadius, FontFamily, FontId, Margin, Stroke, TextStyle, Visuals};
use hephaestus_core::Layer;

const fn hex(rgb: u32) -> Color32 {
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

#[derive(Clone, Debug)]
pub struct Theme {
    pub app_bg: Color32,
    pub chrome: Color32,
    pub panel: Color32,
    pub panel_alt: Color32,
    pub header: Color32,
    pub input: Color32,
    pub border: Color32,
    pub divider: Color32,
    pub text: Color32,
    pub text_strong: Color32,
    pub text_dim: Color32,
    pub text_disabled: Color32,
    pub hover: Color32,
    pub pressed: Color32,
    pub selection: Color32,
    pub selection_text: Color32,
    pub accent: Color32,
    /// Active tool button (Altium's blue-grey highlight).
    pub tool_selected: Color32,
    pub tool_selected_border: Color32,
    pub error: Color32,
    pub warning: Color32,
    pub ok: Color32,
    pub agent: Color32,
    pub sch: SchColors,
    pub pcb: PcbColors,
}

#[derive(Clone, Debug)]
pub struct SchColors {
    pub outside: Color32,
    pub sheet: Color32,
    pub grid: Color32,
    pub grid_major: Color32,
    pub border: Color32,
    pub border_text: Color32,
    pub wire: Color32,
    pub bus: Color32,
    pub junction: Color32,
    pub body_line: Color32,
    pub body_fill: Color32,
    pub graphic: Color32,
    pub pin: Color32,
    pub pin_text: Color32,
    pub designator: Color32,
    pub value: Color32,
    pub net_label: Color32,
    pub power: Color32,
    pub port_fill: Color32,
    pub port_line: Color32,
    pub no_erc: Color32,
    pub text: Color32,
    pub selection: Color32,
    pub hover: Color32,
    pub hotspot: Color32,
    pub highlight: Color32,
    pub error_mark: Color32,
    /// Pins, wire ends and labels that touch nothing.
    pub unconnected: Color32,
}

#[derive(Clone, Debug)]
pub struct PcbColors {
    pub outside: Color32,
    pub board: Color32,
    pub grid: Color32,
    pub grid_major: Color32,
    pub outline: Color32,
    pub hole: Color32,
    pub pad_text: Color32,
    pub ratsnest: Color32,
    pub selection: Color32,
    pub hover: Color32,
    pub violation: Color32,
    pub clearance: Color32,
    pub highlight: Color32,
}

impl Theme {
    pub fn dark() -> Self {
        Self {
            app_bg: hex(0x1b1c1e),
            chrome: hex(0x2b2d30),
            panel: hex(0x252628),
            panel_alt: hex(0x2a2b2e),
            header: hex(0x323438),
            input: hex(0x1e1f21),
            border: hex(0x3c3e43),
            divider: hex(0x34363a),
            text: hex(0xcfd1d4),
            text_strong: hex(0xeeeeee),
            text_dim: hex(0x8f949a),
            text_disabled: hex(0x5d6166),
            hover: hex(0x3a3d42),
            pressed: hex(0x45494f),
            selection: hex(0x2c5d93),
            selection_text: hex(0xffffff),
            accent: hex(0xf2a33a),
            tool_selected: hex(0x31475f),
            tool_selected_border: hex(0x4f7bb0),
            error: hex(0xe5534b),
            warning: hex(0xe0b341),
            ok: hex(0x5cb85c),
            agent: hex(0x9f7aea),
            sch: SchColors {
                outside: hex(0x3b3d40),
                sheet: hex(0xfffdf6),
                grid: hex(0xebe8de),
                grid_major: hex(0xdcd8cb),
                border: hex(0x000000),
                border_text: hex(0x000000),
                wire: hex(0x000080),
                bus: hex(0x000080),
                junction: hex(0x000080),
                body_line: hex(0x800000),
                body_fill: hex(0xffffb0),
                graphic: hex(0x000080),
                pin: hex(0x000000),
                pin_text: hex(0x000000),
                designator: hex(0x000080),
                value: hex(0x000000),
                net_label: hex(0x000000),
                power: hex(0x000080),
                port_fill: hex(0xffffb0),
                port_line: hex(0x800000),
                no_erc: hex(0xff0000),
                text: hex(0x000000),
                selection: hex(0x00b400),
                hover: hex(0x2f7de1),
                hotspot: hex(0xff0000),
                highlight: hex(0xff8000),
                error_mark: hex(0xff0000),
                unconnected: hex(0xe00000),
            },
            pcb: PcbColors {
                outside: hex(0x262626),
                board: hex(0x000000),
                grid: hex(0x2e2e2e),
                grid_major: hex(0x4a4a4a),
                outline: hex(0xff00ff),
                hole: hex(0x2b2b2b),
                pad_text: hex(0xffffff),
                ratsnest: hex(0xb4b4ff),
                selection: hex(0xffffff),
                hover: hex(0x00ffff),
                violation: hex(0x00ff00),
                clearance: hex(0xffffff),
                highlight: hex(0xffffff),
            },
        }
    }

    /// Altium's default layer colours.
    pub fn default_layer_color(layer: Layer) -> Color32 {
        match layer {
            Layer::TopCopper => hex(0xff0000),
            Layer::BottomCopper => hex(0x0000ff),
            Layer::Inner(n) => [hex(0xbc8e00), hex(0x70dbfa), hex(0x00cc66), hex(0x9966ff), hex(0x00ffff), hex(0x800080)]
                [(n as usize).saturating_sub(1) % 6],
            Layer::TopOverlay => hex(0xffff00),
            Layer::BottomOverlay => hex(0x808000),
            Layer::TopPaste => hex(0x808080),
            Layer::BottomPaste => hex(0x800000),
            Layer::TopSolder => hex(0x800080),
            Layer::BottomSolder => hex(0xff00ff),
            Layer::Mechanical(1) => hex(0xff00ff),
            Layer::Mechanical(13) => hex(0xa020f0),
            Layer::Mechanical(15) => hex(0x00a000),
            Layer::Mechanical(_) => hex(0x808000),
            Layer::KeepOut => hex(0xff00ff),
            Layer::MultiLayer => hex(0xc0c0c0),
            Layer::DrillGuide => hex(0x800000),
            Layer::DrillDrawing => hex(0xff002a),
        }
    }

    pub fn font(size: f32) -> FontId {
        FontId::new(size, FontFamily::Proportional)
    }

    pub fn mono(size: f32) -> FontId {
        FontId::new(size, FontFamily::Monospace)
    }

    /// Install the style on a context.
    pub fn apply(&self, ctx: &egui::Context) {
        let mut style = (*ctx.global_style()).clone();
        style.text_styles = [
            (TextStyle::Small, Self::font(10.5)),
            (TextStyle::Body, Self::font(12.5)),
            (TextStyle::Button, Self::font(12.5)),
            (TextStyle::Heading, Self::font(14.0)),
            (TextStyle::Monospace, Self::mono(12.0)),
        ]
        .into();
        style.spacing.item_spacing = egui::vec2(6.0, 3.0);
        style.spacing.button_padding = egui::vec2(6.0, 2.0);
        style.spacing.interact_size = egui::vec2(20.0, 20.0);
        style.spacing.menu_margin = Margin::same(4);
        style.spacing.window_margin = Margin::same(8);
        style.spacing.indent = 14.0;
        style.spacing.combo_height = 300.0;
        style.spacing.scroll.bar_width = 8.0;
        style.interaction.tooltip_delay = 0.4;

        let mut v = Visuals::dark();
        v.panel_fill = self.panel;
        v.window_fill = self.panel_alt;
        v.extreme_bg_color = self.input;
        v.faint_bg_color = self.panel_alt;
        v.code_bg_color = self.input;
        v.window_stroke = Stroke::new(1.0, self.border);
        v.window_corner_radius = CornerRadius::same(3);
        v.menu_corner_radius = CornerRadius::same(2);
        v.window_shadow = egui::Shadow { offset: [0, 4], blur: 12, spread: 0, color: Color32::from_black_alpha(110) };
        v.popup_shadow = egui::Shadow { offset: [0, 3], blur: 8, spread: 0, color: Color32::from_black_alpha(100) };
        v.selection.bg_fill = self.selection;
        v.selection.stroke = Stroke::new(1.0, self.selection_text);
        v.hyperlink_color = hex(0x6aa9ff);
        v.override_text_color = None;
        let w = &mut v.widgets;
        craft_ui::theme::WidgetPalette {
            noninteractive: self.panel,
            inactive: self.header,
            hovered: self.hover,
            active: self.pressed,
            open: self.hover,
        }
        .apply_to(w, CornerRadius::same(2), Stroke::new(1.0, self.text));
        w.noninteractive.bg_stroke = Stroke::new(1.0, self.divider);
        w.noninteractive.fg_stroke = Stroke::new(1.0, self.text);
        w.inactive.bg_stroke = Stroke::new(1.0, self.border);
        w.hovered.bg_stroke = Stroke::new(1.0, hex(0x5a5e66));
        w.active.bg_stroke = Stroke::new(1.0, self.accent);
        w.hovered.expansion = 0.0;
        w.active.expansion = 0.0;
        style.visuals = v;
        ctx.set_global_style(style);
    }
}
