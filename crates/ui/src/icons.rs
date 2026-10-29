//! Toolbar and panel icons drawn with painter primitives (original artwork, resolution
//! independent, tinted by the caller). EDA tools have no stock icon font, so drawing them keeps
//! every glyph consistent and free of third-party licences.

use egui::{Align2, Color32, Painter, Pos2, Rect, Shape, Stroke, Vec2, vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Icon {
    Select,
    Wire,
    Bus,
    NetLabel,
    PowerVcc,
    PowerGnd,
    Port,
    Junction,
    NoErc,
    Text,
    Part,
    Route,
    Track,
    Via,
    Polygon,
    Keepout,
    Measure,
    BoardShape,
    Rotate,
    Mirror,
    Delete,
    ZoomFit,
    ZoomIn,
    ZoomOut,
    Save,
    Open,
    New,
    Undo,
    Redo,
    Compile,
    Check,
    Grid,
    Layers,
    Schematic,
    Pcb,
    Library,
    Folder,
    Project,
    Component,
    Net,
    Agent,
    Export,
    Lock,
    Eye,
    EyeOff,
    Error,
    Warning,
    Info,
    Align,
    Distribute,
    Settings,
    Search,
    Units,
    Highlight,
}

fn s(w: f32, c: Color32) -> Stroke {
    Stroke::new(w, c)
}

const fn hex(rgb: u32) -> Color32 {
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

/// Altium-style colours for an icon, `(primary, secondary)`: blue wiring, red copper and power,
/// yellow part bodies with red outlines, green build and board actions. `None` for icons that
/// take the caller's tint (status glyphs, generic tools).
pub fn palette(icon: Icon) -> Option<(Color32, Color32)> {
    const BLUE: Color32 = hex(0x4d94ff);
    const RED: Color32 = hex(0xf04c46);
    const YELLOW: Color32 = hex(0xf6d55c);
    const GOLD: Color32 = hex(0xe0b040);
    const GREEN: Color32 = hex(0x5cbf60);
    const LIGHT: Color32 = hex(0xe6e6e6);
    const STEEL: Color32 = hex(0x8ab4f8);
    Some(match icon {
        Icon::Select | Icon::Text | Icon::New => (LIGHT, LIGHT),
        Icon::Wire | Icon::Bus | Icon::Junction | Icon::Net | Icon::PowerGnd => (BLUE, BLUE),
        Icon::NetLabel => (hex(0x9cc3ff), RED),
        Icon::PowerVcc => (RED, BLUE),
        Icon::Port | Icon::Part | Icon::Component => (hex(0xd9534f), YELLOW),
        Icon::NoErc | Icon::Delete | Icon::Track | Icon::Polygon => (RED, RED),
        Icon::Route => (RED, GOLD),
        Icon::Via | Icon::Lock => (GOLD, GOLD),
        Icon::Keepout => (hex(0xff4fd8), hex(0xff4fd8)),
        Icon::Measure | Icon::Highlight => (hex(0xf2d24a), hex(0xf2d24a)),
        Icon::BoardShape | Icon::Compile | Icon::Check | Icon::Export => (GREEN, GREEN),
        Icon::Rotate | Icon::Mirror | Icon::Save | Icon::Undo | Icon::Redo => (STEEL, STEEL),
        Icon::Open | Icon::Folder | Icon::Project => (hex(0xe8b84a), hex(0xe8b84a)),
        Icon::Layers => (RED, BLUE),
        Icon::Schematic => (hex(0xf0a030), BLUE),
        Icon::Pcb => (GREEN, GOLD),
        Icon::Library => (hex(0xc8a26a), hex(0xc8a26a)),
        Icon::Agent => (hex(0x9f7aea), hex(0x9f7aea)),
        Icon::ZoomFit | Icon::ZoomIn | Icon::ZoomOut | Icon::Search | Icon::Eye | Icon::EyeOff | Icon::Grid => (hex(0xd0d0d0), STEEL),
        _ => return None,
    })
}

/// Paint `icon` centred in `rect` in a single colour (designed on a 16×16 grid).
pub fn paint(p: &Painter, rect: Rect, icon: Icon, c: Color32) {
    paint_two(p, rect, icon, c, c);
}

/// Paint `icon` in its own Altium-style colours.
pub fn paint_colored(p: &Painter, rect: Rect, icon: Icon, fallback: Color32) {
    let (c, c2) = palette(icon).unwrap_or((fallback, fallback));
    paint_two(p, rect, icon, c, c2);
}

/// Paint `icon` with `c` and accents (fills, second strokes) in `c2`.
pub fn paint_two(p: &Painter, rect: Rect, icon: Icon, c: Color32, c2: Color32) {
    let k = rect.width().min(rect.height()) / 16.0;
    let o = rect.center() - vec2(8.0, 8.0) * k;
    let at = |x: f32, y: f32| o + vec2(x, y) * k;
    let line_in = |pts: &[(f32, f32)], w: f32, col: Color32| {
        let v: Vec<Pos2> = pts.iter().map(|&(x, y)| at(x, y)).collect();
        p.add(Shape::line(v, s(w * k.max(0.8), col)));
    };
    let line = |pts: &[(f32, f32)], w: f32| line_in(pts, w, c);
    let poly = |pts: &[(f32, f32)], fill: bool| {
        let v: Vec<Pos2> = pts.iter().map(|&(x, y)| at(x, y)).collect();
        if fill {
            p.add(Shape::convex_polygon(v, c, Stroke::NONE));
        } else {
            p.add(Shape::closed_line(v, s(1.2 * k, c)));
        }
    };
    let circle = |x: f32, y: f32, r: f32, fill: bool| {
        if fill {
            p.circle_filled(at(x, y), r * k, c);
        } else {
            p.circle_stroke(at(x, y), r * k, s(1.2 * k, c));
        }
    };
    let rect_at = |x0: f32, y0: f32, x1: f32, y1: f32| Rect::from_two_pos(at(x0, y0), at(x1, y1));
    match icon {
        Icon::Select => poly(&[(4.0, 2.0), (4.0, 13.0), (7.0, 10.0), (9.5, 14.5), (11.0, 13.8), (8.6, 9.4), (12.5, 9.2)], true),
        Icon::Wire => {
            line(&[(2.0, 12.0), (8.0, 12.0), (8.0, 4.0), (14.0, 4.0)], 1.6);
            circle(8.0, 12.0, 1.0, true);
        }
        Icon::Bus => line(&[(2.0, 12.0), (8.0, 12.0), (8.0, 4.0), (14.0, 4.0)], 3.0),
        Icon::NetLabel => {
            line_in(&[(1.5, 12.5), (14.5, 12.5)], 1.6, c2);
            p.text(at(8.0, 7.5), Align2::CENTER_CENTER, "Net", egui::FontId::proportional(7.0 * k), c);
        }
        Icon::PowerVcc => {
            line_in(&[(8.0, 14.0), (8.0, 5.0)], 1.4, c2);
            line(&[(3.5, 5.0), (12.5, 5.0)], 2.2);
            p.text(at(8.0, 2.2), Align2::CENTER_CENTER, "VCC", egui::FontId::proportional(4.5 * k), c);
        }
        Icon::PowerGnd => {
            line(&[(8.0, 2.0), (8.0, 8.0)], 1.4);
            line(&[(3.0, 8.0), (13.0, 8.0)], 1.6);
            line(&[(5.0, 10.5), (11.0, 10.5)], 1.4);
            line(&[(7.0, 13.0), (9.0, 13.0)], 1.4);
        }
        Icon::Port => {
            let v: Vec<Pos2> = [(2.0, 5.0), (11.0, 5.0), (14.5, 8.0), (11.0, 11.0), (2.0, 11.0)].iter().map(|&(x, y)| at(x, y)).collect();
            p.add(Shape::convex_polygon(v, c2, s(1.2 * k, c)));
        }
        Icon::Junction => {
            line(&[(2.0, 8.0), (14.0, 8.0)], 1.2);
            line(&[(8.0, 8.0), (8.0, 14.0)], 1.2);
            circle(8.0, 8.0, 2.4, true);
        }
        Icon::NoErc => {
            line(&[(3.5, 3.5), (12.5, 12.5)], 1.8);
            line(&[(12.5, 3.5), (3.5, 12.5)], 1.8);
        }
        Icon::Text => {
            line(&[(3.0, 3.5), (13.0, 3.5)], 1.8);
            line(&[(8.0, 3.5), (8.0, 13.5)], 1.8);
        }
        Icon::Part => {
            p.rect_filled(rect_at(4.5, 3.0, 11.5, 13.0), 0.0, c2);
            p.rect_stroke(rect_at(4.5, 3.0, 11.5, 13.0), 0.0, s(1.3 * k, c), egui::StrokeKind::Middle);
            for y in [5.5, 8.0, 10.5] {
                line_in(&[(1.5, y), (4.5, y)], 1.2, hex(0xb8bcc4));
                line_in(&[(11.5, y), (14.5, y)], 1.2, hex(0xb8bcc4));
            }
        }
        Icon::Route => {
            line(&[(2.0, 13.0), (6.5, 13.0), (11.5, 8.0), (11.5, 3.0)], 2.2);
            p.circle_filled(at(2.5, 13.0), 1.8 * k, c2);
            p.circle_filled(at(11.5, 3.0), 1.8 * k, c2);
        }
        Icon::Track => line(&[(2.0, 13.0), (14.0, 3.0)], 2.4),
        Icon::Via => {
            circle(8.0, 8.0, 5.5, true);
            p.circle_filled(at(8.0, 8.0), 2.2 * k, Color32::from_black_alpha(220));
        }
        Icon::Polygon => {
            let pts = [(2.0, 4.0), (12.0, 2.0), (14.0, 10.0), (6.0, 14.0)];
            let v: Vec<Pos2> = pts.iter().map(|&(x, y)| at(x, y)).collect();
            p.add(Shape::convex_polygon(v, c.gamma_multiply(0.35), s(1.3 * k, c)));
        }
        Icon::Keepout => {
            p.rect_stroke(rect_at(2.5, 2.5, 13.5, 13.5), 0.0, s(1.3 * k, c), egui::StrokeKind::Middle);
            line(&[(2.5, 9.0), (9.0, 2.5)], 1.0);
            line(&[(7.0, 13.5), (13.5, 7.0)], 1.0);
            line(&[(2.5, 13.5), (13.5, 2.5)], 1.0);
        }
        Icon::Measure => {
            line(&[(1.5, 11.0), (14.5, 11.0)], 1.3);
            for x in [2.0, 5.0, 8.0, 11.0, 14.0] {
                line(&[(x, 11.0), (x, if x == 8.0 { 6.0 } else { 8.0 })], 1.1);
            }
        }
        Icon::BoardShape => poly(&[(2.0, 3.0), (14.0, 3.0), (14.0, 9.0), (9.0, 13.0), (2.0, 13.0)], false),
        Icon::Rotate => {
            let pts: Vec<(f32, f32)> = (0..=10)
                .map(|i| {
                    let a = (40.0 + 28.0 * i as f32).to_radians();
                    (8.0 + 5.0 * a.cos(), 8.0 - 5.0 * a.sin())
                })
                .collect();
            line(&pts, 1.5);
            poly(&[(10.0, 2.0), (14.5, 3.5), (11.0, 6.5)], true);
        }
        Icon::Mirror => {
            line(&[(8.0, 1.5), (8.0, 14.5)], 1.0);
            poly(&[(6.5, 4.0), (6.5, 12.0), (2.0, 12.0)], true);
            poly(&[(9.5, 4.0), (9.5, 12.0), (14.0, 12.0)], false);
        }
        Icon::Delete => {
            line(&[(4.0, 4.0), (12.0, 12.0)], 1.8);
            line(&[(12.0, 4.0), (4.0, 12.0)], 1.8);
        }
        Icon::ZoomFit | Icon::ZoomIn | Icon::ZoomOut | Icon::Search => {
            p.circle_filled(at(7.0, 7.0), 4.5 * k, c2.gamma_multiply(0.3));
            circle(7.0, 7.0, 4.5, false);
            line(&[(10.3, 10.3), (14.0, 14.0)], 2.0);
            match icon {
                Icon::ZoomIn => {
                    line(&[(4.8, 7.0), (9.2, 7.0)], 1.2);
                    line(&[(7.0, 4.8), (7.0, 9.2)], 1.2);
                }
                Icon::ZoomOut => line(&[(4.8, 7.0), (9.2, 7.0)], 1.2),
                Icon::ZoomFit => {
                    p.rect_stroke(rect_at(5.0, 5.0, 9.0, 9.0), 0.0, s(1.0 * k, c), egui::StrokeKind::Middle);
                }
                _ => {}
            }
        }
        Icon::Save => {
            p.rect_stroke(rect_at(2.5, 2.5, 13.5, 13.5), 1.0 * k, s(1.3 * k, c), egui::StrokeKind::Middle);
            p.rect_filled(rect_at(5.0, 2.5, 11.0, 6.5), 0.0, c);
            p.rect_stroke(rect_at(4.5, 9.0, 11.5, 13.5), 0.0, s(1.1 * k, c), egui::StrokeKind::Middle);
        }
        Icon::Open | Icon::Folder => {
            poly(&[(1.5, 4.0), (6.0, 4.0), (7.5, 5.5), (14.5, 5.5), (14.5, 13.0), (1.5, 13.0)], false);
            if icon == Icon::Open {
                line(&[(3.5, 13.0), (5.5, 8.0), (15.0, 8.0), (13.0, 13.0)], 1.2);
            }
        }
        Icon::New => {
            poly(&[(3.5, 1.5), (10.0, 1.5), (13.0, 4.5), (13.0, 14.5), (3.5, 14.5)], false);
            line(&[(10.0, 1.5), (10.0, 4.5), (13.0, 4.5)], 1.1);
        }
        Icon::Undo | Icon::Redo => {
            let flip = if icon == Icon::Redo { -1.0 } else { 1.0 };
            let fx = |x: f32| if flip < 0.0 { 16.0 - x } else { x };
            let pts: Vec<(f32, f32)> = (0..=8)
                .map(|i| {
                    let a = (90.0 - 22.5 * i as f32).to_radians();
                    (fx(8.0 + 4.5 * a.cos()), 9.0 - 4.5 * a.sin())
                })
                .collect();
            line(&pts, 1.5);
            line(&[(fx(8.0), 4.5), (fx(3.5), 4.5)], 1.5);
            poly(&[(fx(2.0), 4.5), (fx(5.0), 2.0), (fx(5.0), 7.0)], true);
        }
        Icon::Compile => {
            line(&[(3.0, 8.5), (6.5, 12.0), (13.0, 4.0)], 2.0);
        }
        Icon::Check => {
            circle(8.0, 8.0, 6.0, false);
            line(&[(5.0, 8.3), (7.3, 10.6), (11.2, 5.6)], 1.6);
        }
        Icon::Grid => {
            for v in [3.0, 8.0, 13.0] {
                line(&[(v, 2.0), (v, 14.0)], 0.9);
                line(&[(2.0, v), (14.0, v)], 0.9);
            }
        }
        Icon::Layers => {
            for (i, col) in [(6.0, c2), (3.0, hex(0xb0b0b0)), (0.0, c)] {
                let v: Vec<Pos2> =
                    [(8.0, 2.0 + i), (14.0, 5.0 + i), (8.0, 8.0 + i), (2.0, 5.0 + i)].iter().map(|&(x, y)| at(x, y)).collect();
                p.add(Shape::convex_polygon(v, col.gamma_multiply(0.35), s(1.1 * k, col)));
            }
        }
        Icon::Schematic => {
            poly(&[(2.5, 1.5), (13.5, 1.5), (13.5, 14.5), (2.5, 14.5)], false);
            line_in(&[(4.5, 6.0), (7.0, 6.0), (7.0, 10.0), (11.5, 10.0)], 1.1, c2);
            p.rect_filled(rect_at(8.0, 4.5, 11.5, 7.5), 0.0, c);
        }
        Icon::Pcb => {
            p.rect_filled(rect_at(1.5, 2.5, 14.5, 13.5), 1.0 * k, c.gamma_multiply(0.35));
            p.rect_stroke(rect_at(1.5, 2.5, 14.5, 13.5), 1.0 * k, s(1.1 * k, c), egui::StrokeKind::Middle);
            line_in(&[(4.0, 11.0), (7.0, 8.0), (12.0, 8.0)], 1.3, c2);
            p.circle_filled(at(4.0, 11.0), 1.3 * k, c2);
            p.circle_filled(at(12.0, 5.0), 1.3 * k, c2);
        }
        Icon::Library => {
            for (x, h) in [(2.5, 12.0), (6.0, 11.0), (9.5, 12.5)] {
                p.rect_stroke(rect_at(x, 14.0 - h, x + 2.8, 14.0), 0.0, s(1.1 * k, c), egui::StrokeKind::Middle);
            }
            line(&[(12.5, 3.5), (14.5, 13.5)], 1.2);
        }
        Icon::Project => {
            poly(&[(1.5, 4.0), (6.0, 4.0), (7.5, 5.5), (14.5, 5.5), (14.5, 13.0), (1.5, 13.0)], true);
        }
        Icon::Component => {
            p.rect_filled(rect_at(4.0, 4.0, 12.0, 12.0), 1.0 * k, c2);
            p.rect_stroke(rect_at(4.0, 4.0, 12.0, 12.0), 1.0 * k, s(1.1 * k, c), egui::StrokeKind::Middle);
            for v in [5.5, 8.0, 10.5] {
                line(&[(2.0, v), (4.0, v)], 1.0);
                line(&[(12.0, v), (14.0, v)], 1.0);
            }
        }
        Icon::Net => {
            line(&[(2.0, 12.0), (6.0, 4.0), (10.0, 12.0), (14.0, 4.0)], 1.3);
            for (x, y) in [(2.0, 12.0), (6.0, 4.0), (10.0, 12.0), (14.0, 4.0)] {
                circle(x, y, 1.4, true);
            }
        }
        Icon::Agent => {
            p.rect_stroke(rect_at(3.0, 5.0, 13.0, 13.5), 2.0 * k, s(1.3 * k, c), egui::StrokeKind::Middle);
            line(&[(8.0, 5.0), (8.0, 2.5)], 1.2);
            circle(8.0, 2.2, 1.0, true);
            circle(6.0, 9.0, 1.1, true);
            circle(10.0, 9.0, 1.1, true);
        }
        Icon::Export => {
            line(&[(8.0, 10.0), (8.0, 2.0)], 1.5);
            line(&[(5.0, 5.0), (8.0, 2.0), (11.0, 5.0)], 1.5);
            line(&[(3.0, 9.0), (3.0, 14.0), (13.0, 14.0), (13.0, 9.0)], 1.3);
        }
        Icon::Lock => {
            p.rect_filled(rect_at(3.5, 7.5, 12.5, 14.0), 1.0 * k, c);
            let pts: Vec<(f32, f32)> = (0..=8)
                .map(|i| {
                    let a = (180.0 - 22.5 * i as f32).to_radians();
                    (8.0 + 3.0 * a.cos(), 7.5 - 4.0 * a.sin())
                })
                .collect();
            line(&pts, 1.5);
        }
        Icon::Eye | Icon::EyeOff => {
            let top: Vec<(f32, f32)> = (0..=12)
                .map(|i| {
                    let t = i as f32 / 12.0;
                    (2.0 + 12.0 * t, 8.0 - 4.5 * (t * std::f32::consts::PI).sin())
                })
                .collect();
            let bot: Vec<(f32, f32)> = (0..=12)
                .map(|i| {
                    let t = i as f32 / 12.0;
                    (2.0 + 12.0 * t, 8.0 + 4.5 * (t * std::f32::consts::PI).sin())
                })
                .collect();
            line(&top, 1.2);
            line(&bot, 1.2);
            circle(8.0, 8.0, 2.0, true);
            if icon == Icon::EyeOff {
                line(&[(2.5, 13.5), (13.5, 2.5)], 1.5);
            }
        }
        Icon::Error => {
            p.circle_filled(rect.center(), 6.5 * k, c);
            p.line_segment([at(5.5, 5.5), at(10.5, 10.5)], s(1.6 * k, Color32::WHITE));
            p.line_segment([at(10.5, 5.5), at(5.5, 10.5)], s(1.6 * k, Color32::WHITE));
        }
        Icon::Warning => {
            poly(&[(8.0, 1.5), (15.0, 14.0), (1.0, 14.0)], true);
            p.line_segment([at(8.0, 5.5), at(8.0, 10.0)], s(1.6 * k, Color32::BLACK));
            p.circle_filled(at(8.0, 12.0), 0.9 * k, Color32::BLACK);
        }
        Icon::Info => {
            circle(8.0, 8.0, 6.5, true);
            p.line_segment([at(8.0, 7.0), at(8.0, 12.0)], s(1.6 * k, Color32::BLACK));
            p.circle_filled(at(8.0, 4.8), 0.9 * k, Color32::BLACK);
        }
        Icon::Align => {
            line(&[(2.5, 1.5), (2.5, 14.5)], 1.3);
            p.rect_filled(rect_at(3.5, 3.0, 12.0, 6.5), 0.0, c);
            p.rect_filled(rect_at(3.5, 9.5, 9.0, 13.0), 0.0, c);
        }
        Icon::Distribute => {
            for x in [2.0, 6.5, 11.0] {
                p.rect_filled(rect_at(x, 4.0, x + 3.0, 12.0), 0.0, c);
            }
        }
        Icon::Settings => {
            circle(8.0, 8.0, 3.0, false);
            for i in 0..8 {
                let a = (i as f32 * 45.0).to_radians();
                line(&[(8.0 + 4.5 * a.cos(), 8.0 + 4.5 * a.sin()), (8.0 + 6.5 * a.cos(), 8.0 + 6.5 * a.sin())], 1.8);
            }
        }
        Icon::Units => {
            p.text(at(8.0, 8.0), Align2::CENTER_CENTER, "mm", egui::FontId::proportional(8.0 * k), c);
        }
        Icon::Highlight => {
            line(&[(3.0, 13.0), (11.0, 5.0)], 3.0);
            poly(&[(10.0, 3.0), (13.0, 6.0), (14.5, 2.5)], true);
        }
    }
}

/// An icon button using the shared craft-ui widget.
pub fn button(ui: &mut egui::Ui, icon: Icon, tooltip: &str, selected: bool, size: f32, theme: &crate::theme::Theme) -> egui::Response {
    let style = craft_ui::buttons::IconButtonStyle {
        normal: craft_ui::buttons::ButtonVisuals { fill: Color32::TRANSPARENT, stroke: Stroke::NONE, icon: theme.text },
        hovered: craft_ui::buttons::ButtonVisuals { fill: theme.hover, stroke: Stroke::new(1.0, theme.border), icon: theme.text_strong },
        pressed: Some(craft_ui::buttons::ButtonVisuals {
            fill: theme.pressed,
            stroke: Stroke::new(1.0, theme.border),
            icon: theme.text_strong,
        }),
        selected: craft_ui::buttons::ButtonVisuals {
            fill: theme.tool_selected,
            stroke: Stroke::new(1.0, theme.tool_selected_border),
            icon: theme.accent,
        },
        corner_radius: egui::CornerRadius::same(2),
        focus_stroke: Stroke::new(1.0, theme.accent),
    };
    let enabled = ui.is_enabled();
    let response = craft_ui::buttons::IconButton::new(tooltip, Vec2::splat(size), &style).selected(selected).show(ui, |ui, rect, tint| {
        let rect = rect.shrink(size * 0.17);
        if enabled {
            paint_colored(ui.painter(), rect, icon, tint);
        } else {
            paint(ui.painter(), rect, icon, theme.text_disabled);
        }
    });
    response.on_hover_text(tooltip)
}

/// A tiny filled triangle marking a drop-down on toolbar buttons.
pub fn dropdown_marker(p: &Painter, rect: Rect, c: Color32) {
    let b = rect.right_bottom() - vec2(2.0, 2.0);
    p.add(Shape::convex_polygon(vec![b, b - vec2(4.0, 0.0), b - vec2(0.0, 4.0)], c, Stroke::NONE));
}
