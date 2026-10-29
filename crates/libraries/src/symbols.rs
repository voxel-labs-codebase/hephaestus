//! Schematic symbol generators, drawn on the 100 mil schematic grid in Altium's proportions.

use hephaestus_core::geometry::{Coord, Point, mil};
use hephaestus_core::{PinDef, PinDirection, PinType, SymbolDef, SymbolGraphic};

const LINE: Coord = 0;

fn p(x: f64, y: f64) -> Point {
    Point::new(mil(x), mil(y))
}

fn pin(number: &str, name: &str, pin_type: PinType, at: Point, direction: PinDirection, length: f64) -> PinDef {
    PinDef {
        number: number.into(),
        name: name.into(),
        pin_type,
        position: at,
        direction,
        length: mil(length),
        show_name: true,
        show_number: true,
    }
}

fn line(points: &[(f64, f64)]) -> SymbolGraphic {
    SymbolGraphic::Line { points: points.iter().map(|&(x, y)| p(x, y)).collect(), width: LINE }
}

/// A two-pin horizontal part with hot spots at ±200 mil.
fn two_pin(name: &str, description: &str, graphics: Vec<SymbolGraphic>, pin_types: [PinType; 2], numbers: [&str; 2]) -> SymbolDef {
    let mut pins = vec![
        pin(numbers[0], numbers[0], pin_types[0], p(-200.0, 0.0), PinDirection::Left, 100.0),
        pin(numbers[1], numbers[1], pin_types[1], p(200.0, 0.0), PinDirection::Right, 100.0),
    ];
    for pin in &mut pins {
        pin.show_name = false;
        pin.show_number = false;
    }
    SymbolDef {
        name: name.into(),
        description: description.into(),
        pins,
        graphics,
        designator_position: p(-100.0, 90.0),
        value_position: p(-100.0, -170.0),
        hide_pin_names: true,
    }
}

pub fn resistor() -> SymbolDef {
    let zig =
        line(&[(-100.0, 0.0), (-83.0, 40.0), (-50.0, -40.0), (-17.0, 40.0), (17.0, -40.0), (50.0, 40.0), (83.0, -40.0), (100.0, 0.0)]);
    two_pin("Resistor", "Resistor", vec![zig], [PinType::Passive; 2], ["1", "2"])
}

pub fn capacitor() -> SymbolDef {
    let graphics = vec![
        line(&[(-100.0, 0.0), (-20.0, 0.0)]),
        line(&[(20.0, 0.0), (100.0, 0.0)]),
        SymbolGraphic::Line { points: vec![p(-20.0, -80.0), p(-20.0, 80.0)], width: mil(10.0) },
        SymbolGraphic::Line { points: vec![p(20.0, -80.0), p(20.0, 80.0)], width: mil(10.0) },
    ];
    two_pin("Capacitor", "Non-polarized capacitor", graphics, [PinType::Passive; 2], ["1", "2"])
}

pub fn capacitor_polarized() -> SymbolDef {
    let graphics = vec![
        line(&[(-100.0, 0.0), (-20.0, 0.0)]),
        line(&[(25.0, 0.0), (100.0, 0.0)]),
        SymbolGraphic::Line { points: vec![p(-20.0, -80.0), p(-20.0, 80.0)], width: mil(10.0) },
        SymbolGraphic::Arc { center: p(85.0, 0.0), radius: mil(60.0), start: 132.0, end: 228.0, width: mil(10.0) },
        line(&[(-70.0, 50.0), (-40.0, 50.0)]),
        line(&[(-55.0, 35.0), (-55.0, 65.0)]),
    ];
    two_pin("Capacitor Polarized", "Polarized capacitor (pin 1 positive)", graphics, [PinType::Passive; 2], ["1", "2"])
}

pub fn inductor() -> SymbolDef {
    let graphics: Vec<SymbolGraphic> = (0..4)
        .map(|i| SymbolGraphic::Arc { center: p(-75.0 + 50.0 * i as f64, 0.0), radius: mil(25.0), start: 0.0, end: 180.0, width: LINE })
        .collect();
    two_pin("Inductor", "Inductor", graphics, [PinType::Passive; 2], ["1", "2"])
}

/// Anode on the left (pin 2), cathode on the right (pin 1), matching footprint pad 1 = cathode.
pub fn diode(name: &str, led: bool) -> SymbolDef {
    let mut graphics = vec![
        line(&[(-100.0, 0.0), (-40.0, 0.0)]),
        line(&[(40.0, 0.0), (100.0, 0.0)]),
        SymbolGraphic::Polygon { points: vec![p(-40.0, 50.0), p(-40.0, -50.0), p(40.0, 0.0)], width: LINE, filled: true },
        SymbolGraphic::Line { points: vec![p(40.0, -50.0), p(40.0, 50.0)], width: mil(8.0) },
    ];
    if led {
        for dx in [0.0, 35.0] {
            graphics.push(line(&[(-5.0 + dx, 60.0), (35.0 + dx, 100.0)]));
            graphics.push(SymbolGraphic::Polygon {
                points: vec![p(35.0 + dx, 100.0), p(17.0 + dx, 96.0), p(31.0 + dx, 82.0)],
                width: LINE,
                filled: true,
            });
        }
    }
    let mut symbol = two_pin(name, if led { "Light-emitting diode" } else { "Diode" }, graphics, [PinType::Passive; 2], ["2", "1"]);
    symbol.pins[0].name = "A".into();
    symbol.pins[1].name = "K".into();
    symbol
}

pub fn crystal() -> SymbolDef {
    let graphics = vec![
        line(&[(-100.0, 0.0), (-50.0, 0.0)]),
        line(&[(50.0, 0.0), (100.0, 0.0)]),
        SymbolGraphic::Line { points: vec![p(-50.0, -60.0), p(-50.0, 60.0)], width: mil(8.0) },
        SymbolGraphic::Line { points: vec![p(50.0, -60.0), p(50.0, 60.0)], width: mil(8.0) },
        SymbolGraphic::Rect { min: p(-30.0, -80.0), max: p(30.0, 80.0), width: LINE, filled: false },
    ];
    two_pin("Crystal", "Quartz crystal", graphics, [PinType::Passive; 2], ["1", "2"])
}

/// A bipolar or MOSFET transistor: base/gate left, collector/drain top, emitter/source bottom.
pub fn transistor(name: &str, mosfet: bool, numbers: [&str; 3]) -> SymbolDef {
    let (left, top, bottom) = if mosfet { ("G", "D", "S") } else { ("B", "C", "E") };
    let mut graphics = vec![SymbolGraphic::Circle { center: p(30.0, 0.0), radius: mil(110.0), width: LINE, filled: false }];
    if mosfet {
        graphics.extend([
            line(&[(-100.0, 0.0), (-20.0, 0.0), (-20.0, 60.0)]),
            SymbolGraphic::Line { points: vec![p(10.0, -70.0), p(10.0, 70.0)], width: mil(10.0) },
            line(&[(10.0, 50.0), (100.0, 50.0), (100.0, 100.0)]),
            line(&[(10.0, -50.0), (100.0, -50.0), (100.0, -100.0)]),
            line(&[(10.0, 0.0), (100.0, 0.0), (100.0, -50.0)]),
            SymbolGraphic::Polygon { points: vec![p(10.0, 0.0), p(45.0, 18.0), p(45.0, -18.0)], width: LINE, filled: true },
        ]);
    } else {
        graphics.extend([
            line(&[(-100.0, 0.0), (0.0, 0.0)]),
            SymbolGraphic::Line { points: vec![p(0.0, -60.0), p(0.0, 60.0)], width: mil(10.0) },
            line(&[(0.0, 30.0), (100.0, 100.0)]),
            line(&[(0.0, -30.0), (100.0, -100.0)]),
            SymbolGraphic::Polygon { points: vec![p(100.0, -100.0), p(60.0, -95.0), p(80.0, -65.0)], width: LINE, filled: true },
        ]);
    }
    let mut pins = vec![
        pin(numbers[0], left, PinType::Input, p(-200.0, 0.0), PinDirection::Left, 100.0),
        pin(numbers[1], top, PinType::Passive, p(100.0, 200.0), PinDirection::Up, 100.0),
        pin(numbers[2], bottom, PinType::Passive, p(100.0, -200.0), PinDirection::Down, 100.0),
    ];
    for pin in &mut pins {
        pin.show_name = false;
    }
    SymbolDef {
        name: name.into(),
        description: if mosfet { "N-channel MOSFET".into() } else { "NPN bipolar transistor".into() },
        pins,
        graphics,
        designator_position: p(160.0, 40.0),
        value_position: p(160.0, -60.0),
        hide_pin_names: true,
    }
}

/// A single op-amp triangle: + and − inputs on the left, output right, supplies top and bottom.
pub fn opamp(name: &str, numbers: [&str; 5]) -> SymbolDef {
    let graphics = vec![
        SymbolGraphic::Polygon { points: vec![p(-200.0, 200.0), p(-200.0, -200.0), p(200.0, 0.0)], width: LINE, filled: true },
        line(&[(-180.0, 100.0), (-140.0, 100.0)]),
        line(&[(-160.0, 120.0), (-160.0, 80.0)]),
        line(&[(-180.0, -100.0), (-140.0, -100.0)]),
    ];
    let mut pins = vec![
        pin(numbers[0], "+", PinType::Input, p(-400.0, 100.0), PinDirection::Left, 200.0),
        pin(numbers[1], "-", PinType::Input, p(-400.0, -100.0), PinDirection::Left, 200.0),
        pin(numbers[2], "OUT", PinType::Output, p(400.0, 0.0), PinDirection::Right, 200.0),
        pin(numbers[3], "V+", PinType::Power, p(0.0, 300.0), PinDirection::Up, 200.0),
        pin(numbers[4], "V-", PinType::Power, p(0.0, -300.0), PinDirection::Down, 200.0),
    ];
    for pin in &mut pins {
        pin.show_name = false;
    }
    SymbolDef {
        name: name.into(),
        description: "Operational amplifier".into(),
        pins,
        graphics,
        designator_position: p(100.0, 200.0),
        value_position: p(100.0, 120.0),
        hide_pin_names: true,
    }
}

/// One pin of a box symbol.
pub struct BoxPin<'a> {
    pub number: &'a str,
    pub name: &'a str,
    pub pin_type: PinType,
}

pub const fn bp<'a>(number: &'a str, name: &'a str, pin_type: PinType) -> BoxPin<'a> {
    BoxPin { number, name, pin_type }
}

/// An IC rectangle with pins on the left and right (top to bottom), 100 mil pitch, 300 mil pins
/// (Altium's default pin length).
pub fn ic_box(name: &str, description: &str, left: &[BoxPin], right: &[BoxPin], width: f64) -> SymbolDef {
    let rows = left.len().max(right.len()) as f64;
    let height = (rows + 1.0) * 100.0;
    let half_w = width / 2.0;
    let top = height / 2.0;
    let top = (top / 100.0).round() * 100.0;
    let mut pins = Vec::new();
    for (i, bp) in left.iter().enumerate() {
        if bp.number.is_empty() {
            continue;
        }
        pins.push(pin(bp.number, bp.name, bp.pin_type, p(-half_w - 300.0, top - 100.0 * (i as f64 + 1.0)), PinDirection::Left, 300.0));
    }
    for (i, bp) in right.iter().enumerate() {
        if bp.number.is_empty() {
            continue;
        }
        pins.push(pin(bp.number, bp.name, bp.pin_type, p(half_w + 300.0, top - 100.0 * (i as f64 + 1.0)), PinDirection::Right, 300.0));
    }
    let bottom = top - height;
    SymbolDef {
        name: name.into(),
        description: description.into(),
        pins,
        graphics: vec![SymbolGraphic::Rect { min: p(-half_w, bottom), max: p(half_w, top), width: LINE, filled: true }],
        designator_position: p(-half_w, top + 30.0),
        value_position: p(-half_w, bottom - 130.0),
        hide_pin_names: false,
    }
}

/// A single-row connector with pins on the left.
pub fn header(pins: usize) -> SymbolDef {
    let numbers: Vec<String> = (1..=pins).map(|i| i.to_string()).collect();
    let left: Vec<BoxPin> = numbers.iter().map(|n| bp(n, n, PinType::Passive)).collect();
    let mut symbol = ic_box(&format!("Header {pins}"), &format!("{pins}-pin header"), &left, &[], 200.0);
    for pin in &mut symbol.pins {
        pin.length = mil(200.0);
        pin.position = pin.position + Point::new(mil(100.0), 0);
        pin.show_name = false;
    }
    symbol
}

/// A single-pin test point.
pub fn test_point() -> SymbolDef {
    let mut p1 = pin("1", "TP", PinType::Passive, p(0.0, -200.0), PinDirection::Down, 100.0);
    p1.show_name = false;
    p1.show_number = false;
    SymbolDef {
        name: "Test Point".into(),
        description: "Test point".into(),
        pins: vec![p1],
        graphics: vec![SymbolGraphic::Circle { center: p(0.0, -60.0), radius: mil(40.0), width: LINE, filled: false }],
        designator_position: p(60.0, -40.0),
        value_position: p(60.0, -120.0),
        hide_pin_names: true,
    }
}
