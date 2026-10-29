//! The Hephaestus built-in library and library file I/O.
//!
//! Every symbol and footprint here is generated in code from public package dimensions
//! (IPC-7351 nominal lands, JEDEC outlines) and manufacturer pinouts, so the library carries no
//! third-party artwork. Libraries are stored as JSON (`*.pcblib.json`) with the same schema as
//! the project library, so agents and version control can read them.

pub mod footprints;
pub mod symbols;

use std::path::Path;

use hephaestus_core::{Library, PartDef, PartKind, PinType};

use symbols::bp;

/// The current built-in library revision; bump when parts change.
pub const BUILTIN_VERSION: u32 = 2;

fn part(name: &str, description: &str, category: &str, symbol: &str, footprints: &[&str], prefix: &str, value: &str) -> PartDef {
    PartDef {
        description: description.into(),
        category: category.into(),
        footprints: footprints.iter().map(|f| f.to_string()).collect(),
        default_value: value.into(),
        ..PartDef::new(name, symbol, footprints.first().copied().unwrap_or_default(), prefix)
    }
}

/// A generic family: placing it creates one component per value and footprint.
fn generic(part: PartDef) -> PartDef {
    PartDef { kind: PartKind::Generic, ..part }
}

/// A specific part with its manufacturer part number and other parameters.
fn specific(mut part: PartDef, manufacturer: &str, mpn: &str, params: &[(&str, &str)]) -> PartDef {
    part.manufacturer = manufacturer.into();
    part.mpn = mpn.into();
    part.parameters = params.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    part
}

/// The built-in library: common passives, discretes, ICs and connectors.
pub fn builtin() -> Library {
    use PinType::*;
    let mut lib = Library::new("Hephaestus Basic");
    lib.version = BUILTIN_VERSION;

    let syms = [
        symbols::resistor(),
        symbols::capacitor(),
        symbols::capacitor_polarized(),
        symbols::inductor(),
        symbols::diode("Diode", false),
        symbols::diode("LED", true),
        symbols::crystal(),
        symbols::transistor("NPN", false, ["1", "3", "2"]),
        symbols::transistor("NMOS", true, ["1", "3", "2"]),
        symbols::opamp("Op Amp", ["3", "4", "1", "5", "2"]),
        symbols::test_point(),
        symbols::ic_box(
            "NE555",
            "Precision timer",
            &[bp("4", "RESET", Input), bp("5", "CONT", Input), bp("6", "THRES", Input), bp("2", "TRIG", Input)],
            &[bp("8", "VCC", Power), bp("3", "OUT", Output), bp("7", "DISCH", OpenCollector), bp("1", "GND", Power)],
            800.0,
        ),
        symbols::ic_box(
            "LM358",
            "Dual operational amplifier",
            &[bp("3", "IN1+", Input), bp("2", "IN1-", Input), bp("5", "IN2+", Input), bp("6", "IN2-", Input)],
            &[bp("8", "V+", Power), bp("1", "OUT1", Output), bp("7", "OUT2", Output), bp("4", "V-", Power)],
            800.0,
        ),
        symbols::ic_box(
            "AMS1117",
            "1 A low-dropout linear regulator",
            &[bp("3", "VIN", Power)],
            &[bp("2", "VOUT", Power), bp("1", "GND", Power)],
            800.0,
        ),
        symbols::ic_box(
            "ATmega328P",
            "8-bit AVR microcontroller, 32 KB flash",
            &[
                bp("29", "PC6/RESET", Input),
                bp("7", "PB6/XTAL1", Bidirectional),
                bp("8", "PB7/XTAL2", Bidirectional),
                bp("", "", Passive),
                bp("4", "VCC", Power),
                bp("6", "VCC", Power),
                bp("18", "AVCC", Power),
                bp("20", "AREF", Passive),
                bp("", "", Passive),
                bp("3", "GND", Power),
                bp("5", "GND", Power),
                bp("21", "GND", Power),
                bp("", "", Passive),
                bp("19", "ADC6", Input),
                bp("22", "ADC7", Input),
            ],
            &[
                bp("12", "PB0", Bidirectional),
                bp("13", "PB1", Bidirectional),
                bp("14", "PB2", Bidirectional),
                bp("15", "PB3/MOSI", Bidirectional),
                bp("16", "PB4/MISO", Bidirectional),
                bp("17", "PB5/SCK", Bidirectional),
                bp("", "", Passive),
                bp("23", "PC0", Bidirectional),
                bp("24", "PC1", Bidirectional),
                bp("25", "PC2", Bidirectional),
                bp("26", "PC3", Bidirectional),
                bp("27", "PC4/SDA", Bidirectional),
                bp("28", "PC5/SCL", Bidirectional),
                bp("", "", Passive),
                bp("30", "PD0/RXD", Bidirectional),
                bp("31", "PD1/TXD", Bidirectional),
                bp("32", "PD2", Bidirectional),
                bp("1", "PD3", Bidirectional),
                bp("2", "PD4", Bidirectional),
                bp("9", "PD5", Bidirectional),
                bp("10", "PD6", Bidirectional),
                bp("11", "PD7", Bidirectional),
            ],
            1400.0,
        ),
    ];
    for s in syms {
        lib.symbols.insert(s.name.clone(), s);
    }
    for n in [2, 3, 4, 6, 8] {
        let s = symbols::header(n);
        lib.symbols.insert(s.name.clone(), s);
    }

    let fps = [
        footprints::chip("0402", 1.0, (0.6, 0.6), (1.0, 0.5)),
        footprints::chip("0603", 1.6, (0.9, 0.95), (1.6, 0.8)),
        footprints::chip("0805", 1.9, (1.0, 1.45), (2.0, 1.25)),
        footprints::chip("1206", 2.9, (1.15, 1.8), (3.2, 1.6)),
        footprints::polarized_chip("LED-0603", 1.6, (0.9, 0.95), (1.6, 0.8)),
        footprints::polarized_chip("SOD-123", 3.3, (0.9, 1.2), (2.7, 1.6)),
        footprints::polarized_chip("CASE-A-3216", 3.0, (1.4, 1.35), (3.2, 1.6)),
        footprints::sot23(3),
        footprints::sot23(5),
        footprints::sot223(),
        footprints::soic(8, 1.27, 5.4, (1.55, 0.6), (3.9, 4.9)),
        footprints::qfp(32, 0.8, 8.4, (1.5, 0.55), 7.0),
        footprints::dip(8),
        footprints::axial("AXIAL-0.4", 10.16, (6.3, 2.5), 1.6, 0.8, false),
        footprints::axial("RAD-0.1", 2.54, (5.0, 5.0), 1.6, 0.8, true),
        footprints::axial("HC49-US", 4.88, (11.0, 4.6), 1.6, 0.8, false),
        footprints::radial_led(),
        footprints::mounting_hole(),
        footprints::test_point(),
    ];
    for f in fps {
        lib.footprints.insert(f.name.clone(), f);
    }
    for n in [2, 3, 4, 6, 8] {
        let f = footprints::header(n);
        lib.footprints.insert(f.name.clone(), f);
    }

    const TI_555: &str = "https://www.ti.com/lit/ds/symlink/ne555.pdf";
    const TI_358: &str = "https://www.ti.com/lit/ds/symlink/lm358.pdf";
    let mut parts = vec![
        generic(part(
            "RES",
            "Resistor, thick film",
            "Passives/Resistors",
            "Resistor",
            &["0603", "0402", "0805", "1206", "AXIAL-0.4"],
            "R",
            "10k",
        )),
        generic(part(
            "CAP",
            "Capacitor, ceramic MLCC",
            "Passives/Capacitors",
            "Capacitor",
            &["0603", "0402", "0805", "1206"],
            "C",
            "100nF",
        )),
        generic(part(
            "CAP_POL",
            "Capacitor, polarized (electrolytic/tantalum)",
            "Passives/Capacitors",
            "Capacitor Polarized",
            &["CASE-A-3216", "RAD-0.1"],
            "C",
            "10uF",
        )),
        generic(part("IND", "Inductor", "Passives/Inductors", "Inductor", &["0805", "1206"], "L", "10uH")),
        specific(
            part("DIODE", "Switching diode", "Discretes/Diodes", "Diode", &["SOD-123"], "D", "1N4148W"),
            "Diodes Inc.",
            "1N4148W-7-F",
            &[],
        ),
        generic(part("LED", "Light-emitting diode", "Discretes/LEDs", "LED", &["LED-0603", "LED-5MM"], "D", "Red")),
        specific(
            part("NPN", "NPN transistor, 40 V 200 mA", "Discretes/Transistors", "NPN", &["SOT-23"], "Q", "MMBT3904"),
            "onsemi",
            "MMBT3904LT1G",
            &[],
        ),
        specific(
            part("NMOS", "N-channel MOSFET, 60 V 115 mA", "Discretes/Transistors", "NMOS", &["SOT-23"], "Q", "2N7002"),
            "onsemi",
            "2N7002LT1G",
            &[],
        ),
        generic(part("CRYSTAL", "Quartz crystal", "Frequency Control", "Crystal", &["HC49-US"], "Y", "16MHz")),
        specific(
            part("NE555", "Precision timer, SOIC-8", "ICs/Timers", "NE555", &["SOIC-8"], "U", "NE555"),
            "Texas Instruments",
            "NE555DR",
            &[("Datasheet", TI_555)],
        ),
        specific(
            part("NE555P", "Precision timer, DIP-8", "ICs/Timers", "NE555", &["DIP-8"], "U", "NE555"),
            "Texas Instruments",
            "NE555P",
            &[("Datasheet", TI_555)],
        ),
        specific(
            part("LM358", "Dual operational amplifier, SOIC-8", "ICs/Amplifiers", "LM358", &["SOIC-8"], "U", "LM358"),
            "Texas Instruments",
            "LM358DR",
            &[("Datasheet", TI_358)],
        ),
        specific(
            part("LM358P", "Dual operational amplifier, DIP-8", "ICs/Amplifiers", "LM358", &["DIP-8"], "U", "LM358"),
            "Texas Instruments",
            "LM358P",
            &[("Datasheet", TI_358)],
        ),
        specific(
            part("OPAMP_SOT23-5", "Single op-amp, rail-to-rail", "ICs/Amplifiers", "Op Amp", &["SOT-23-5"], "U", "LMV321"),
            "Texas Instruments",
            "LMV321IDBVR",
            &[],
        ),
        specific(
            part("AMS1117-3.3", "3.3 V 1 A LDO regulator", "ICs/Power", "AMS1117", &["SOT-223"], "U", "AMS1117-3.3"),
            "Advanced Monolithic Systems",
            "AMS1117-3.3",
            &[],
        ),
        specific(
            part("ATMEGA328P-AU", "AVR microcontroller, TQFP-32", "ICs/Microcontrollers", "ATmega328P", &["TQFP-32"], "U", "ATmega328P-AU"),
            "Microchip",
            "ATMEGA328P-AU",
            &[(
                "Datasheet",
                "https://ww1.microchip.com/downloads/en/DeviceDoc/Atmel-7810-Automotive-Microcontrollers-ATmega328P_Datasheet.pdf",
            )],
        ),
        generic(part("TP", "Test point", "Mechanical", "Test Point", &["TP-1.5MM"], "TP", "TP")),
    ];
    // The regulator's tab is VOUT: pin 2 drives both pad 2 and the tab pad 4.
    if let Some(reg) = parts.iter_mut().find(|p| p.name == "AMS1117-3.3") {
        reg.pin_map.insert("2".into(), vec!["2".into(), "4".into()]);
    }
    for n in [2, 3, 4, 6, 8] {
        parts.push(generic(part(
            &format!("HDR1X{n}"),
            &format!("Pin header, 1x{n}, 2.54 mm"),
            "Connectors/Headers",
            &format!("Header {n}"),
            &[&format!("HDR-1x{n}")],
            "J",
            &format!("Header {n}"),
        )));
    }
    for p in parts {
        lib.parts.insert(p.name.clone(), p);
    }
    lib
}

#[derive(Debug)]
pub enum LibraryIoError {
    Io(std::io::Error),
    Format(String),
}

impl std::fmt::Display for LibraryIoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LibraryIoError::Io(e) => write!(f, "{e}"),
            LibraryIoError::Format(e) => write!(f, "invalid library: {e}"),
        }
    }
}

impl std::error::Error for LibraryIoError {}

/// Read a `*.pcblib.json` library and check that every part resolves.
pub fn load(path: &Path) -> Result<Library, LibraryIoError> {
    let text = std::fs::read_to_string(path).map_err(LibraryIoError::Io)?;
    let lib: Library = serde_json::from_str(&text).map_err(|e| LibraryIoError::Format(e.to_string()))?;
    let problems: Vec<String> = lib.parts.values().flat_map(|p| lib.check_part(p)).collect();
    if let Some(first) = problems.first() {
        return Err(LibraryIoError::Format(format!("{first} ({} problems)", problems.len())));
    }
    Ok(lib)
}

pub fn save(lib: &Library, path: &Path) -> Result<(), LibraryIoError> {
    let text = serde_json::to_string_pretty(lib).map_err(|e| LibraryIoError::Format(e.to_string()))?;
    std::fs::write(path, text).map_err(LibraryIoError::Io)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_part_resolves() {
        let lib = builtin();
        for part in lib.parts.values() {
            assert!(lib.check_part(part).is_empty(), "{:?}", lib.check_part(part));
        }
        assert!(lib.parts.len() >= 20);
    }

    #[test]
    fn every_symbol_pin_number_is_unique_and_on_grid() {
        let grid = hephaestus_core::geometry::mil(100.0);
        for symbol in builtin().symbols.values() {
            let mut numbers: Vec<&str> = symbol.pins.iter().map(|p| p.number.as_str()).collect();
            numbers.sort_unstable();
            let before = numbers.len();
            numbers.dedup();
            assert_eq!(before, numbers.len(), "duplicate pin in {}", symbol.name);
            for pin in &symbol.pins {
                assert_eq!(pin.position.x % grid, 0, "{} pin {} off grid", symbol.name, pin.number);
                assert_eq!(pin.position.y % grid, 0, "{} pin {} off grid", symbol.name, pin.number);
            }
        }
    }

    #[test]
    fn atmega_has_all_32_pins() {
        let lib = builtin();
        let symbol = &lib.symbols["ATmega328P"];
        assert_eq!(symbol.pins.len(), 32);
        assert_eq!(lib.footprints["TQFP-32"].pads.len(), 32);
    }

    #[test]
    fn regulator_tab_maps_to_vout() {
        let lib = builtin();
        let part = &lib.parts["AMS1117-3.3"];
        assert_eq!(part.pads_for_pin("2"), vec!["2", "4"]);
        assert_eq!(part.pin_for_pad(&lib.symbols["AMS1117"], "4"), Some("2"));
    }

    #[test]
    fn search_matches_all_terms() {
        let lib = builtin();
        let hits: Vec<&str> = lib.search("op amp sot").iter().map(|p| p.name.as_str()).collect();
        assert_eq!(hits, vec!["OPAMP_SOT23-5"]);
        assert!(lib.search("texas").len() >= 2);
        assert_eq!(lib.search("ne555dr").len(), 1);
    }

    #[test]
    fn library_file_round_trip() {
        let dir = std::env::temp_dir().join(format!("hephaestus-lib-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("basic.pcblib.json");
        save(&builtin(), &path).unwrap();
        assert_eq!(load(&path).unwrap(), builtin());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
