//! Electrical rule check on the compiled netlist.

use std::collections::BTreeMap;

use hephaestus_connectivity::schematic::{Netlist, compile};
use hephaestus_core::{Design, PinType};

use crate::{Check, Report, Severity, Violation};

fn v(rule: &str, severity: Severity, message: String) -> Violation {
    Violation { check: Check::Erc, rule: rule.into(), severity, message, objects: Vec::new(), location: None, sheet: None, layer: None }
}

/// Run every ERC check on a freshly compiled netlist.
pub fn check(design: &Design) -> Report {
    check_netlist(design, &compile(design))
}

pub fn check_netlist(design: &Design, netlist: &Netlist) -> Report {
    let mut out = Vec::new();

    for pin in &netlist.unconnected_pins {
        let (rule, severity) = match pin.pin_type {
            PinType::Input => ("Floating input pin", Severity::Error),
            PinType::Power => ("Unconnected power pin", Severity::Error),
            _ => ("Unconnected pin", Severity::Warning),
        };
        let mut x = v(rule, severity, format!("Pin {}-{} ({}) is not connected", pin.designator, pin.pin, pin.pin_name));
        x.objects = vec![pin.symbol, pin.component];
        x.location = Some(pin.position);
        x.sheet = Some(pin.sheet);
        out.push(x);
    }

    for (pin, connected) in &netlist.no_connect_pins {
        if *connected {
            let mut x = v(
                "No ERC on connected pin",
                Severity::Warning,
                format!("Pin {}-{} has a No ERC marker but is connected", pin.designator, pin.pin),
            );
            x.objects = vec![pin.symbol];
            x.location = Some(pin.position);
            x.sheet = Some(pin.sheet);
            out.push(x);
        }
    }

    for net in &netlist.nets {
        let first = net.nodes.first();
        let place = |x: &mut Violation| {
            x.location = first.map(|n| n.position);
            x.sheet = first.map(|n| n.sheet);
            x.objects = net.nodes.iter().map(|n| n.symbol).collect();
        };
        let count = |t: PinType| net.nodes.iter().filter(|n| n.pin_type == t).count();
        let outputs = count(PinType::Output);
        if outputs > 1 {
            let mut x = v("Conflicting outputs", Severity::Error, format!("Net {} has {outputs} output pins driving it", net.name));
            place(&mut x);
            out.push(x);
        }
        if outputs > 0 && net.power {
            let mut x = v("Output on power net", Severity::Error, format!("Net {} connects an output pin to a power port", net.name));
            place(&mut x);
            out.push(x);
        }
        let inputs = count(PinType::Input);
        let drivers = outputs
            + count(PinType::Bidirectional)
            + count(PinType::Power)
            + count(PinType::OpenCollector)
            + count(PinType::OpenEmitter)
            + count(PinType::Passive)
            + count(PinType::HiZ);
        if inputs > 0 && drivers == 0 && !net.power {
            let has_port =
                design.schematic.values().any(|o| net.objects.contains(&o.id) && matches!(o.kind, hephaestus_core::SchKind::Port(_)));
            if !has_port {
                let mut x = v("Net has no driving source", Severity::Warning, format!("Net {} has only input pins", net.name));
                place(&mut x);
                out.push(x);
            }
        }
        if net.nodes.len() == 1 && !net.power {
            let mut x = v(
                "Net with only one pin",
                Severity::Warning,
                format!("Net {} connects to a single pin ({}-{})", net.name, net.nodes[0].designator, net.nodes[0].pin),
            );
            place(&mut x);
            out.push(x);
        }
        if net.names.len() > 1 {
            let mut x = v(
                "Net has multiple names",
                Severity::Warning,
                format!(
                    "Net {} is also named {}",
                    net.name,
                    net.names.iter().filter(|n| **n != net.name).cloned().collect::<Vec<_>>().join(", ")
                ),
            );
            place(&mut x);
            x.objects = net.objects.clone();
            out.push(x);
        }
    }

    for &object in &netlist.floating_objects {
        let Some(o) = design.schematic.get(&object) else { continue };
        let mut x = v("Floating net label", Severity::Warning, format!("{} {} touches no wire or pin", o.kind.type_name(), object));
        x.objects = vec![object];
        x.location = Some(o.anchor());
        x.sheet = Some(o.sheet);
        out.push(x);
    }

    for (sheet, wire, point) in &netlist.dangling_wire_ends {
        let mut x = v("Unconnected wire end", Severity::Warning, format!("Wire {wire} ends without a connection"));
        x.objects = vec![*wire];
        x.location = Some(*point);
        x.sheet = Some(*sheet);
        out.push(x);
    }

    let mut designators: BTreeMap<String, Vec<_>> = BTreeMap::new();
    for c in design.components.values() {
        designators.entry(c.designator.to_ascii_uppercase()).or_default().push(c.id);
        if c.designator.ends_with('?') {
            let mut x = v("Unannotated component", Severity::Error, format!("{} has no designator number", c.designator));
            x.objects = vec![c.id];
            out.push(x);
        }
    }
    for (d, ids) in designators {
        if ids.len() > 1 {
            let mut x = v("Duplicate designator", Severity::Error, format!("{} components share designator {d}", ids.len()));
            x.objects = ids;
            out.push(x);
        }
    }
    for problem in design.integrity_problems() {
        out.push(v("Library reference", Severity::Error, problem));
    }
    Report::new(out)
}
