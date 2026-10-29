//! Components panel: search the libraries; each component shows its schematic symbol, footprint
//! and 3D body, its part numbers and status, and can be placed, exported or imported.

use egui::{Color32, Pos2, RichText, Sense, Stroke, Ui, vec2};
use hephaestus_canvas::Camera;
use hephaestus_core::geometry::{Coord, Point, Rect, Transform, mm};
use hephaestus_core::library_check::{ComponentStatus, check_library};
use hephaestus_core::{FootprintDef, Layer, Library, PadKind, PadShape, PartDef, PartKind, SymbolDef};

use crate::actions::{Action, SchToolKind};
use crate::app::{ComponentTab, HephaestusApp};
use crate::draw::{Canvas, fill_polygon, pad_outline};
use crate::panels::{readout, section};
use crate::sch::render::{self, Look};
use crate::theme::Theme;

fn symbol_preview(ui: &mut Ui, app: &HephaestusApp, symbol: &SymbolDef) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 150.0), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, app.theme.sch.sheet);
    let mut cam = Camera::default();
    cam.fit(rect, symbol.bounds().inflated(hephaestus_core::geometry::mil(150.0)), 0.06);
    let cv = Canvas { painter: &painter, cam: &cam, vp: rect };
    render::symbol(&cv, symbol, &Transform::default(), &app.theme.sch, Look::default(), 1.0);
}

pub fn footprint_preview(ui: &mut Ui, app: &HephaestusApp, fp: &FootprintDef) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 130.0), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, Color32::BLACK);
    let mut cam = Camera::default();
    let bounds = fp.pads.iter().map(|p| Rect::from_center(p.position, p.width / 2, p.height / 2)).fold(fp.courtyard, Rect::union);
    cam.fit(rect, bounds.inflated(hephaestus_core::geometry::mil(20.0)), 0.08);
    let cv = Canvas { painter: &painter, cam: &cam, vp: rect };
    for g in &fp.graphics {
        if matches!(g.layer, Layer::TopOverlay | Layer::Mechanical(15)) {
            let c = app.prefs.layer_color(g.layer);
            cv.graphic(&g.shape, &Transform::default(), c, Color32::TRANSPARENT, c);
        }
    }
    for pad in &fp.pads {
        let color = if matches!(pad.kind, PadKind::Smd) {
            app.prefs.layer_color(Layer::TopCopper)
        } else {
            app.prefs.layer_color(Layer::MultiLayer)
        };
        let corner = match pad.shape {
            PadShape::Round | PadShape::Oval => pad.width.min(pad.height) / 2,
            PadShape::Rect => 0,
            PadShape::RoundRect { ratio } => (pad.width.min(pad.height) as f64 * ratio) as i64,
        };
        let pts: Vec<egui::Pos2> =
            pad_outline(pad.position, pad.width, pad.height, corner, pad.rotation).iter().map(|p| cv.pos(*p)).collect();
        fill_polygon(&painter, &pts, color);
        if let Some(d) = pad.kind.drill() {
            painter.circle_filled(cv.pos(pad.position), cv.len(d / 2), app.theme.pcb.hole);
        }
        if cv.len(pad.width.min(pad.height)) > 12.0 {
            painter.text(cv.pos(pad.position), egui::Align2::CENTER_CENTER, &pad.number, egui::FontId::proportional(10.0), Color32::WHITE);
        }
    }
}

/// Body size (width, length, height) of a footprint's 3D model: the generated box description
/// (`generated:box 1.6x0.8x0.45mm`), else the courtyard and model height.
fn body_size(fp: &FootprintDef) -> Option<(Coord, Coord, Coord)> {
    let model = fp.model.as_ref()?;
    let parsed = model.source.strip_prefix("generated:box ").and_then(|d| d.strip_suffix("mm")).and_then(|d| {
        let v: Vec<f64> = d.split('x').filter_map(|x| x.trim().parse().ok()).collect();
        match v.as_slice() {
            [w, l, h] => Some((mm(*w), mm(*l), mm(*h))),
            _ => None,
        }
    });
    Some(parsed.unwrap_or((fp.courtyard.width(), fp.courtyard.height(), model.height)))
}

/// An isometric view of the footprint's pads and 3D body on a piece of board.
fn model_preview(ui: &mut Ui, app: &HephaestusApp, fp: &FootprintDef, prefix: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 170.0), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, Color32::from_rgb(28, 30, 34));
    let Some((bw, bl, bh)) = body_size(fp) else {
        painter.text(rect.center(), egui::Align2::CENTER_CENTER, "No 3D model", Theme::font(12.0), app.theme.text_dim);
        return;
    };
    let (c30, s30) = (30f64.to_radians().cos(), 30f64.to_radians().sin());
    let iso = |x: Coord, y: Coord, z: Coord| -> (f64, f64) { ((x - y) as f64 * c30, -((x + y) as f64 * s30 + z as f64)) };
    let pad_bounds = fp.pads.iter().map(|p| Rect::from_center(p.position, p.width / 2, p.height / 2)).fold(fp.courtyard, Rect::union);
    let board = pad_bounds.inflated(mm(0.6).max(pad_bounds.width().min(pad_bounds.height()) / 6));
    let corners = [
        (board.min.x, board.min.y, 0),
        (board.max.x, board.min.y, 0),
        (board.max.x, board.max.y, 0),
        (board.min.x, board.max.y, 0),
        (-bw / 2, -bl / 2, bh),
        (bw / 2, bl / 2, bh),
    ];
    let pts: Vec<(f64, f64)> = corners.iter().map(|&(x, y, z)| iso(x, y, z)).collect();
    let (min_x, max_x) = pts.iter().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p.0), b.max(p.0)));
    let (min_y, max_y) = pts.iter().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p.1), b.max(p.1)));
    let inner = rect.shrink(14.0);
    let k = ((inner.width() as f64) / (max_x - min_x).max(1.0)).min((inner.height() as f64) / (max_y - min_y).max(1.0));
    let (cx, cy) = ((min_x + max_x) / 2.0, (min_y + max_y) / 2.0);
    let to = |x: Coord, y: Coord, z: Coord| -> Pos2 {
        let (u, v) = iso(x, y, z);
        Pos2::new(inner.center().x + ((u - cx) * k) as f32, inner.center().y + ((v - cy) * k) as f32)
    };
    let face = |pts: &[(Coord, Coord, Coord)], fill: Color32| {
        let v: Vec<Pos2> = pts.iter().map(|&(x, y, z)| to(x, y, z)).collect();
        fill_polygon(&painter, &v, fill);
        painter.add(egui::Shape::closed_line(v, Stroke::new(0.6, fill.gamma_multiply(1.4))));
    };
    // Board, then copper pads, then the body's two visible sides and its top.
    face(&[corners[0], corners[1], corners[2], corners[3]], Color32::from_rgb(22, 82, 48));
    for pad in &fp.pads {
        let corner = match pad.shape {
            PadShape::Round | PadShape::Oval => pad.width.min(pad.height) / 2,
            PadShape::Rect => 0,
            PadShape::RoundRect { ratio } => (pad.width.min(pad.height) as f64 * ratio) as Coord,
        };
        let outline: Vec<(Coord, Coord, Coord)> =
            pad_outline(pad.position, pad.width, pad.height, corner, pad.rotation).iter().map(|p| (p.x, p.y, 0)).collect();
        face(&outline, Color32::from_rgb(214, 170, 76));
        if let Some(d) = pad.kind.drill() {
            let hole: Vec<(Coord, Coord, Coord)> = (0..16)
                .map(|i| {
                    let a = (i as f64 * 22.5).to_radians();
                    let p = pad.position + Point::new((a.cos() * d as f64 / 2.0) as Coord, (a.sin() * d as f64 / 2.0) as Coord);
                    (p.x, p.y, 0)
                })
                .collect();
            face(&hole, Color32::from_rgb(12, 12, 12));
        }
    }
    let body = match prefix {
        "C" => Color32::from_rgb(176, 140, 92),
        "D" if fp.name.contains("LED") => Color32::from_rgb(200, 40, 40),
        "J" => Color32::from_rgb(40, 40, 44),
        _ => Color32::from_rgb(46, 46, 50),
    };
    let (x0, x1, y0, y1) = (-bw / 2, bw / 2, -bl / 2, bl / 2);
    face(&[(x0, y0, 0), (x1, y0, 0), (x1, y0, bh), (x0, y0, bh)], body.gamma_multiply(0.8));
    face(&[(x0, y0, 0), (x0, y1, 0), (x0, y1, bh), (x0, y0, bh)], body.gamma_multiply(0.65));
    face(&[(x0, y0, bh), (x1, y0, bh), (x1, y1, bh), (x0, y1, bh)], body);
    let (w, l, h) = (bw as f64 / 1e6, bl as f64 / 1e6, bh as f64 / 1e6);
    painter.text(
        rect.right_bottom() - vec2(6.0, 4.0),
        egui::Align2::RIGHT_BOTTOM,
        format!("{w:.2} × {l:.2} × {h:.2} mm (generated)"),
        Theme::font(10.5),
        app.theme.text_dim,
    );
}

fn badge_color(theme: &Theme, badge: &str) -> Color32 {
    match badge {
        "broken" => theme.error,
        "update available" => theme.accent,
        "modified locally" => theme.warning,
        "no part number" => Color32::from_rgb(214, 120, 110),
        _ => theme.text_dim,
    }
}

fn badges(ui: &mut Ui, theme: &Theme, status: &ComponentStatus) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(4.0, 2.0);
        for b in status.badges() {
            let c = badge_color(theme, b);
            egui::Frame::new().stroke(Stroke::new(1.0, c)).corner_radius(3).inner_margin(egui::Margin::symmetric(4, 0)).show(ui, |ui| {
                ui.label(RichText::new(b).size(10.5).color(c));
            });
        }
    });
}

pub fn show(app: &mut HephaestusApp, ui: &mut Ui) {
    let theme = app.theme.clone();
    let mut libs: Vec<Library> = vec![app.design().library.clone()];
    libs.extend(app.session.libraries().iter().cloned());
    let project_lib = libs.first().map(|l| l.name.clone()).unwrap_or_default();
    // Status badges are recomputed only when the design changes.
    let revision = app.cache.revision;
    if app.components.status.as_ref().is_none_or(|(r, _)| *r != revision) {
        app.components.status = Some((revision, check_library(app.design(), app.session.libraries())));
    }
    let statuses: Vec<ComponentStatus> = app.components.status.as_ref().map(|(_, s)| s.clone()).unwrap_or_default();
    let lib_index = app.components.library.min(libs.len());
    let mut import = false;
    ui.horizontal(|ui| {
        let names: Vec<String> = std::iter::once("All Libraries".to_string()).chain(libs.iter().map(|l| l.name.clone())).collect();
        egui::ComboBox::from_id_salt("lib-select")
            .width((ui.available_width() - 64.0).max(80.0))
            .selected_text(names.get(lib_index).cloned().unwrap_or_default())
            .show_ui(ui, |ui| {
                for (i, n) in names.iter().enumerate() {
                    if ui.selectable_label(i == lib_index, n).clicked() {
                        app.components.library = i;
                    }
                }
            });
        import = ui.button("Import…").on_hover_text("Add components from a component pack (*.pcbcomp.json)").clicked();
    });
    crate::panels::search_box(ui, &mut app.components.query, "Search (e.g. \"resistor 0603\", \"op amp\", \"NE555DR\")");
    let pool: Vec<&Library> = if lib_index == 0 { libs.iter().collect() } else { libs.get(lib_index - 1).into_iter().collect() };
    let mut categories: Vec<String> = pool.iter().flat_map(|l| l.categories().into_iter().map(String::from)).collect();
    categories.sort();
    categories.dedup();
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(3.0, 3.0);
        if ui.selectable_label(app.components.category.is_none(), "All").clicked() {
            app.components.category = None;
        }
        for c in &categories {
            let short = c.rsplit('/').next().unwrap_or(c);
            if ui.selectable_label(app.components.category.as_deref() == Some(c.as_str()), short).on_hover_text(c).clicked() {
                app.components.category = Some(c.clone());
            }
        }
    });
    let mut parts: Vec<(String, PartDef)> = Vec::new();
    for lib in &pool {
        for p in lib.search(&app.components.query) {
            if app.components.category.as_ref().is_some_and(|c| &p.category != c) || parts.iter().any(|(_, x)| x.name == p.name) {
                continue;
            }
            parts.push((lib.name.clone(), p.clone()));
        }
    }
    let status_of = |lib: &str, name: &str| statuses.iter().find(|s| lib == project_lib && s.name == name);
    ui.label(RichText::new(format!("{} components", parts.len())).size(11.0).color(theme.text_dim));
    crate::panels::header_row(ui, &theme, &[("Component", 130.0), ("Description", 200.0)]);
    let list_height = (ui.available_height() * 0.38).max(120.0);
    let mut place = None;
    egui::ScrollArea::vertical().id_salt("parts-list").max_height(list_height).auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for (lib, p) in &parts {
            let selected = app.components.selected.as_deref() == Some(p.name.as_str());
            let flagged = status_of(lib, &p.name).and_then(|s| s.badges().first().copied());
            let name_color = flagged.map(|b| badge_color(&theme, b)).filter(|_| !selected).unwrap_or(theme.text_strong);
            let desc = match flagged {
                Some(b) => format!("{} · {b}", p.description),
                None => p.description.clone(),
            };
            let resp = crate::panels::table_row(ui, &theme, selected, &[(p.title(), 130.0, name_color), (desc, 260.0, theme.text)]);
            if resp.clicked() {
                app.components.selected = Some(p.name.clone());
            }
            if resp.double_clicked() {
                place = Some(p.name.clone());
            }
        }
    });
    ui.add_space(4.0);
    let mut export = None;
    let selected = app.components.selected.clone().and_then(|n| parts.iter().find(|(_, p)| p.name == n).cloned());
    if let Some((lib_name, part)) = selected {
        let lib = libs.iter().find(|l| l.name == lib_name);
        let in_project = lib_name == project_lib;
        let status = status_of(&lib_name, &part.name).cloned();
        egui::ScrollArea::vertical().id_salt("part-details").auto_shrink([false, false]).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(part.title()).strong().color(theme.text_strong));
                if ui
                    .add(egui::Button::new(RichText::new("Place").strong()).fill(theme.selection))
                    .on_hover_text("Place on the active schematic sheet (or double-click the row)")
                    .clicked()
                {
                    place = Some(part.name.clone());
                }
                if in_project && ui.button("Export…").on_hover_text("Save as a component pack (*.pcbcomp.json)").clicked() {
                    export = Some(part.name.clone());
                }
            });
            if let Some(st) = &status {
                badges(ui, &theme, st);
            }
            ui.horizontal(|ui| {
                for (tab, label) in
                    [(ComponentTab::Schematic, "Schematic"), (ComponentTab::Footprint, "Footprint"), (ComponentTab::Model3d, "3D")]
                {
                    if ui.selectable_label(app.components.tab == tab, label).clicked() {
                        app.components.tab = tab;
                    }
                }
            });
            let footprint = lib.and_then(|l| part.footprints.first().and_then(|f| l.footprints.get(f)));
            match app.components.tab {
                ComponentTab::Schematic => {
                    if let Some(sym) = lib.and_then(|l| l.symbols.get(&part.symbol)) {
                        symbol_preview(ui, app, sym);
                    }
                }
                ComponentTab::Footprint => {
                    if let Some(fp) = footprint {
                        footprint_preview(ui, app, fp);
                    }
                }
                ComponentTab::Model3d => {
                    if let Some(fp) = footprint {
                        model_preview(ui, app, fp, &part.designator_prefix);
                    }
                }
            }
            section(ui, &theme, "Component", |ui| {
                readout(ui, &theme, "Description", part.description.clone());
                readout(ui, &theme, "Type", if part.kind == PartKind::Generic { "Generic (any approved part)" } else { "Specific part" });
                readout(ui, &theme, "Value", part.default_value.clone());
                readout(ui, &theme, "Footprint", part.footprints.first().cloned().unwrap_or_default());
                if part.footprints.len() > 1 {
                    readout(ui, &theme, "Alternatives", part.footprints[1..].join(", "));
                }
                readout(ui, &theme, "Category", part.category.clone());
                if let Some(st) = &status {
                    readout(ui, &theme, "Used By", if st.used_by.is_empty() { "—".into() } else { st.used_by.join(", ") });
                }
            });
            section(ui, &theme, "Sourcing", |ui| {
                if !part.mpn.is_empty() {
                    readout(ui, &theme, "Manufacturer", part.manufacturer.clone());
                    readout(ui, &theme, "Part Number", part.mpn.clone());
                }
                if part.approved.is_empty() && part.mpn.is_empty() {
                    readout(ui, &theme, "Part Number", "not chosen yet");
                }
                for (i, a) in part.approved.iter().enumerate() {
                    let label = if i == 0 { "Preferred" } else { "Alternate" };
                    let mut text = format!("{} {}", a.manufacturer, a.mpn);
                    if a.jlc_basic == Some(true) {
                        text.push_str(" · JLC Basic");
                    }
                    readout(ui, &theme, label, text);
                    for (supplier, number) in &a.suppliers {
                        readout(ui, &theme, &format!("  {supplier}"), number.clone());
                    }
                }
            });
            section(ui, &theme, "Parameters", |ui| {
                for (k, v) in &part.parameters {
                    if v.starts_with("http") {
                        crate::panels::row(ui, &theme, k, |ui| {
                            ui.hyperlink_to("Open", v);
                        });
                    } else {
                        readout(ui, &theme, k, v.clone());
                    }
                }
                readout(ui, &theme, "Library", lib_name.clone());
                if let Some(o) = &part.origin {
                    readout(ui, &theme, "Origin", format!("{} › {} (v{})", o.source, o.item, o.version));
                }
            });
        });
    }
    if let Some(name) = place {
        app.components.selected = Some(name);
        app.perform(Action::SchTool(SchToolKind::Part));
    }
    if let Some(name) = export {
        let picked = rfd::FileDialog::new()
            .set_title("Export Component")
            .set_file_name(format!("{name}.{}", hephaestus_project::library_store::PACK_EXTENSION))
            .add_filter("Component pack", &["json"])
            .save_file();
        if let Some(path) = picked {
            app.status = match hephaestus_project::library_store::save_pack(&app.design().library, [name.as_str()], &path) {
                Ok(_) => format!("Exported {name} to {}", path.display()),
                Err(e) => format!("Export failed: {e}"),
            };
        }
    }
    if import {
        let picked = rfd::FileDialog::new().set_title("Import Components").add_filter("Component pack", &["json"]).pick_file();
        if let Some(path) = picked {
            match hephaestus_project::library_store::load_pack(&path) {
                Ok(library) => {
                    let count = library.parts.len();
                    if app.run(hephaestus_commands::Command::ImportLibrary { library }).is_some() {
                        app.status = format!("Imported {count} component(s) from {}", path.display());
                    }
                }
                Err(e) => app.status = format!("Import failed: {e}"),
            }
        }
    }
}
