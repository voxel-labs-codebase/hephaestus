//! A demonstration project — an NE555 astable LED flasher — built entirely through commands, as
//! an agent would build it. Used by `hephaestus --demo`, the examples folder and tests.

use hephaestus_commands::{Command, ComponentRef, Source};
use hephaestus_core::geometry::{Point, mil};
use hephaestus_core::{Design, Layer, PowerStyle};

use crate::{ApiError, Session};

fn p(x: f64, y: f64) -> Point {
    Point::new(mil(x), mil(y))
}

struct Builder<'a> {
    s: &'a mut Session,
}

impl Builder<'_> {
    fn run(&mut self, c: Command) -> Result<(), ApiError> {
        self.s.editor.execute(&c, Source::Script)?;
        Ok(())
    }

    fn place(&mut self, part: &str, des: &str, value: &str, x: f64, y: f64, rot: f64) -> Result<(), ApiError> {
        self.run(Command::PlaceComponent {
            part: part.into(),
            sheet: None,
            position: p(x, y),
            rotation: rot,
            mirror: false,
            designator: Some(des.into()),
            value: Some(value.into()),
            footprint: None,
            parameters: Default::default(),
        })
    }

    fn wire(&mut self, pts: &[(f64, f64)]) -> Result<(), ApiError> {
        self.run(Command::AddWire { sheet: None, points: pts.iter().map(|&(x, y)| p(x, y)).collect() })
    }

    fn power(&mut self, net: &str, x: f64, y: f64) -> Result<(), ApiError> {
        let ground = net == "GND";
        self.run(Command::AddPowerPort {
            sheet: None,
            position: p(x, y),
            net: net.into(),
            style: Some(if ground { PowerStyle::PowerGround } else { PowerStyle::Bar }),
            rotation: Some(if ground { 270.0 } else { 90.0 }),
        })
    }

    fn label(&mut self, text: &str, x: f64, y: f64) -> Result<(), ApiError> {
        self.run(Command::AddNetLabel { sheet: None, position: p(x, y), text: text.into(), rotation: 0.0 })
    }

    fn fp(&mut self, des: &str, x: f64, y: f64, rot: f64) -> Result<(), ApiError> {
        self.run(Command::PlaceFootprint {
            component: ComponentRef::Designator(des.into()),
            position: p(x, y),
            rotation: Some(rot),
            side: None,
        })
    }
}

/// Build the demo into a fresh session.
pub fn flasher() -> Result<Session, ApiError> {
    let mut session = Session::with_design(Design::new("LED_Flasher"));
    let mut b = Builder { s: &mut session };
    let sheet = b.s.design().sheets_in_order().first().map(|s| s.id).ok_or_else(|| ApiError::new("internal", "no sheet"))?;
    b.run(Command::UpdateSheet {
        sheet,
        name: Some("Timer".into()),
        size: Some(hephaestus_core::SheetSize::A4),
        title: Some("555 LED Flasher".into()),
        revision: Some("A".into()),
        orientation: None,
        title_block: None,
        show_zones: None,
    })?;

    // Schematic (mils). U1 pins: RESET 4300,4200 · CONT 4300,4100 · THRES 4300,4000 · TRIG 4300,3900
    // VCC 5700,4200 · OUT 5700,4100 · DISCH 5700,4000 · GND 5700,3900.
    b.place("NE555", "U1", "NE555", 5000.0, 4000.0, 0.0)?;
    b.place("RES", "R1", "10k", 6300.0, 4800.0, 90.0)?;
    b.place("RES", "R2", "47k", 6300.0, 3600.0, 90.0)?;
    b.place("CAP", "C1", "10uF", 4000.0, 2800.0, 90.0)?;
    b.place("CAP", "C2", "10nF", 3600.0, 3800.0, 90.0)?;
    b.place("RES", "R3", "330R", 7400.0, 4100.0, 0.0)?;
    b.place("LED", "D1", "Red", 8100.0, 4100.0, 0.0)?;
    b.place("CAP", "C3", "100nF", 7000.0, 5000.0, 90.0)?;
    b.place("HDR1X2", "J1", "PWR", 2600.0, 5000.0, 0.0)?;

    // Discharge: R1 → DISCH, R2 tees in.
    b.wire(&[(6300.0, 4600.0), (6300.0, 4000.0), (5700.0, 4000.0)])?;
    b.wire(&[(6300.0, 3800.0), (6300.0, 4000.0)])?;
    b.power("VCC", 6300.0, 5000.0)?;
    // Threshold/trigger node to R2 and C1.
    b.wire(&[(4300.0, 4000.0), (4000.0, 4000.0), (4000.0, 3200.0), (6300.0, 3200.0), (6300.0, 3400.0)])?;
    b.wire(&[(4300.0, 3900.0), (4000.0, 3900.0)])?;
    b.wire(&[(4000.0, 3000.0), (4000.0, 3200.0)])?;
    b.power("GND", 4000.0, 2600.0)?;
    // Control voltage capacitor.
    b.wire(&[(4300.0, 4100.0), (3600.0, 4100.0), (3600.0, 4000.0)])?;
    b.power("GND", 3600.0, 3600.0)?;
    // Reset to VCC, supply pins.
    b.wire(&[(4300.0, 4200.0), (4100.0, 4200.0), (4100.0, 4400.0)])?;
    b.power("VCC", 4100.0, 4400.0)?;
    b.wire(&[(5700.0, 4200.0), (5900.0, 4200.0), (5900.0, 4400.0)])?;
    b.power("VCC", 5900.0, 4400.0)?;
    b.wire(&[(5700.0, 3900.0), (5900.0, 3900.0), (5900.0, 3700.0)])?;
    b.power("GND", 5900.0, 3700.0)?;
    // Output via net label to the LED driver.
    b.wire(&[(5700.0, 4100.0), (6100.0, 4100.0)])?;
    b.label("OUT", 5800.0, 4100.0)?;
    b.wire(&[(6900.0, 4100.0), (7200.0, 4100.0)])?;
    b.label("OUT", 6950.0, 4100.0)?;
    b.wire(&[(7600.0, 4100.0), (7900.0, 4100.0)])?;
    b.wire(&[(8300.0, 4100.0), (8500.0, 4100.0), (8500.0, 3800.0)])?;
    b.power("GND", 8500.0, 3800.0)?;
    // Decoupling.
    b.wire(&[(7000.0, 5200.0), (7000.0, 5400.0)])?;
    b.power("VCC", 7000.0, 5400.0)?;
    b.wire(&[(7000.0, 4800.0), (7000.0, 4600.0)])?;
    b.power("GND", 7000.0, 4600.0)?;
    // Power input.
    b.wire(&[(2300.0, 5100.0), (2000.0, 5100.0), (2000.0, 5300.0)])?;
    b.power("VCC", 2000.0, 5300.0)?;
    b.wire(&[(2300.0, 5000.0), (2100.0, 5000.0), (2100.0, 4800.0)])?;
    b.power("GND", 2100.0, 4800.0)?;
    b.run(Command::AddText {
        sheet: None,
        position: p(2000.0, 2200.0),
        text: "f ≈ 1.44 / ((R1 + 2·R2)·C1) ≈ 1.4 Hz".into(),
        size: Some(mil(80.0)),
    })?;

    // Board: 2000 × 1500 mil, two layers.
    b.run(Command::SetBoardOutline { outline: vec![p(0.0, 0.0), p(2000.0, 0.0), p(2000.0, 1500.0), p(0.0, 1500.0)] })?;
    b.run(Command::UpdatePcb {})?;
    b.fp("J1", 250.0, 750.0, 0.0)?;
    b.fp("C3", 650.0, 1150.0, 0.0)?;
    b.fp("U1", 1000.0, 750.0, 0.0)?;
    b.fp("C2", 600.0, 450.0, 90.0)?;
    b.fp("R1", 1450.0, 1100.0, 90.0)?;
    b.fp("R2", 1450.0, 750.0, 90.0)?;
    b.fp("C1", 1450.0, 350.0, 90.0)?;
    b.fp("R3", 1750.0, 1150.0, 90.0)?;
    b.fp("D1", 1750.0, 650.0, 90.0)?;
    // Route what the direct router can do cleanly; the rest stays as ratsnest for interactive
    // routing (a refused net is expected, not an error).
    for net in ["NetC1_2", "OUT", "NetD1_2", "NetR1_1", "NetC2_2", "VCC", "GND"] {
        let _ = b.run(Command::RouteNet { net: net.into(), layer: Some(Layer::TopCopper), width: None });
    }
    for (x, label) in [(200.0, "LED FLASHER"), (1300.0, "REV A")] {
        b.run(Command::AddBoardText { layer: Layer::TopOverlay, position: p(x, 1380.0), text: label.into(), height: mil(50.0) })?;
    }
    session.editor.mark_saved();
    Ok(session)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hephaestus_connectivity::schematic::compile;

    #[test]
    fn demo_builds_with_the_intended_netlist() {
        let s = flasher().unwrap();
        let d = s.design();
        assert!(d.integrity_problems().is_empty());
        let n = compile(d);
        let vcc = n.net("VCC").unwrap();
        let pins: Vec<String> = vcc.nodes.iter().map(|p| format!("{}.{}", p.designator, p.pin)).collect();
        assert_eq!(pins, ["C3.2", "J1.1", "R1.2", "U1.4", "U1.8"]);
        let gnd = n.net("GND").unwrap();
        assert_eq!(gnd.nodes.len(), 6);
        assert_eq!(n.net("OUT").unwrap().nodes.len(), 2);
        assert!(n.unconnected_pins.is_empty(), "{:?}", n.unconnected_pins);
        assert!(n.dangling_wire_ends.is_empty(), "{:?}", n.dangling_wire_ends);
        let erc = hephaestus_rules::erc::check(d);
        assert_eq!(erc.errors, 0, "{:#?}", erc.violations);
        let drc = hephaestus_rules::drc::check(d);
        let bad: Vec<_> = drc.violations.iter().filter(|v| v.rule != "Un-Routed Net").collect();
        assert!(bad.is_empty(), "{bad:#?}");
        assert!(d.pcb.values().filter(|o| matches!(o.kind, hephaestus_core::PcbKind::Track(_))).count() >= 4);
    }
}
