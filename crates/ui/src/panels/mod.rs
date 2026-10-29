//! Dockable panels and the compact property-grid widgets they share.

pub mod agents;
pub mod components;
pub mod layers;
pub mod messages;
pub mod navigator;
pub mod nets;
pub mod output;
pub mod projects;
pub mod properties;

use std::collections::HashMap;

use egui::{Color32, RichText, Ui, vec2};
use hephaestus_core::geometry::{Coord, Unit, format_coord, parse_length};

use crate::theme::Theme;

/// Width of the label column in property grids.
pub const LABEL_WIDTH: f32 = 92.0;

/// Text being edited in property fields, keyed by object and field so a half-typed value
/// survives repaints and is committed only on Enter or focus loss.
#[derive(Default)]
pub struct EditState {
    pub buffers: HashMap<String, String>,
}

/// An Altium-style collapsible section header.
pub fn section(ui: &mut Ui, theme: &Theme, title: &str, add: impl FnOnce(&mut Ui)) {
    egui::CollapsingHeader::new(RichText::new(title).strong().color(theme.text_strong).size(12.0)).id_salt(title).default_open(true).show(
        ui,
        |ui| {
            ui.spacing_mut().item_spacing.y = 3.0;
            add(ui);
        },
    );
}

/// A label/value row.
pub fn row(ui: &mut Ui, theme: &Theme, label: &str, add: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(LABEL_WIDTH, 20.0), egui::Sense::hover());
        ui.painter().text(r.left_center(), egui::Align2::LEFT_CENTER, label, Theme::font(12.0), theme.text_dim);
        add(ui);
    });
}

pub fn readout(ui: &mut Ui, theme: &Theme, label: &str, value: impl Into<String>) {
    row(ui, theme, label, |ui| {
        ui.label(RichText::new(value.into()).color(theme.text));
    });
}

/// A single-line text field that reports a new value on Enter or when focus leaves.
pub fn text_field(ui: &mut Ui, edit: &mut EditState, key: &str, current: &str) -> Option<String> {
    let buf = edit.buffers.entry(key.to_string()).or_insert_with(|| current.to_string());
    let resp = ui.add(egui::TextEdit::singleline(buf).desired_width(ui.available_width().max(60.0)).margin(vec2(4.0, 2.0)));
    if resp.lost_focus() {
        let value = buf.clone();
        edit.buffers.remove(key);
        if value != current {
            return Some(value);
        }
    } else if !resp.has_focus() {
        *buf = current.to_string();
    }
    None
}

/// A length field in display units; accepts unit suffixes.
pub fn length_field(ui: &mut Ui, edit: &mut EditState, key: &str, current: Coord, unit: Unit) -> Option<Coord> {
    let shown = format_coord(current, unit);
    let typed = text_field(ui, edit, key, &shown)?;
    parse_length(&typed, unit)
}

/// A labelled length row.
pub fn length_row(ui: &mut Ui, theme: &Theme, edit: &mut EditState, label: &str, key: &str, current: Coord, unit: Unit) -> Option<Coord> {
    let mut out = None;
    row(ui, theme, label, |ui| out = length_field(ui, edit, key, current, unit));
    out
}

pub fn text_row(ui: &mut Ui, theme: &Theme, edit: &mut EditState, label: &str, key: &str, current: &str) -> Option<String> {
    let mut out = None;
    row(ui, theme, label, |ui| out = text_field(ui, edit, key, current));
    out
}

/// A thin search box with a placeholder.
pub fn search_box(ui: &mut Ui, text: &mut String, hint: &str) -> egui::Response {
    ui.add(egui::TextEdit::singleline(text).hint_text(hint).desired_width(ui.available_width()).margin(vec2(4.0, 3.0)))
}

/// A table header row.
pub fn header_row(ui: &mut Ui, theme: &Theme, columns: &[(&str, f32)]) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 19.0), egui::Sense::hover());
    ui.painter().rect_filled(r, 0.0, theme.header);
    let mut x = r.left() + 4.0;
    for (name, w) in columns {
        ui.painter().text(egui::pos2(x, r.center().y), egui::Align2::LEFT_CENTER, *name, Theme::font(11.5), theme.text_dim);
        x += w;
    }
}

/// A selectable table row; returns the response.
pub fn table_row(ui: &mut Ui, theme: &Theme, selected: bool, cells: &[(String, f32, Color32)]) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 19.0), egui::Sense::click());
    let fill = if selected {
        theme.selection
    } else if resp.hovered() {
        theme.hover
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(r, 0.0, fill);
    let mut x = r.left() + 4.0;
    for (text, w, color) in cells {
        let clip = egui::Rect::from_min_max(egui::pos2(x, r.top()), egui::pos2((x + w - 4.0).min(r.right()), r.bottom()));
        ui.painter().with_clip_rect(clip).text(
            egui::pos2(x, r.center().y),
            egui::Align2::LEFT_CENTER,
            text,
            Theme::font(12.0),
            if selected { theme.selection_text } else { *color },
        );
        x += w;
    }
    resp
}
