//! Hephaestus desktop entry point.
//!
//! ```text
//! hephaestus [PROJECT.pcbprj | DESIGN.hephaestus.json]
//!          [--demo] [--port N] [--document pcb|NAME]
//!          [--screenshot OUT.png [--exit]] [--select R1,U1] [--panel NAME] [--size WxH]
//!          [--component NAME[:schematic|footprint|3d]]
//! hephaestus --write-demo FOLDER     write the example project and exit
//! ```

use std::path::PathBuf;

use hephaestus_ui::{HephaestusApp, StartOptions};

fn usage() -> ! {
    eprintln!(
        "usage: hephaestus [PROJECT] [--demo] [--port N] [--document pcb|SHEET] [--screenshot OUT.png [--exit]] [--select R1,U1] [--panel NAME] [--size WxH] [--component NAME[:VIEW]]\n       hephaestus --write-demo FOLDER"
    );
    std::process::exit(2)
}

fn main() {
    let mut start = StartOptions::default();
    let mut size = [1600.0f32, 960.0];
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--demo" => start.demo = true,
            "--exit" => start.exit_after_screenshot = true,
            "--port" => start.control_port = args.next().and_then(|v| v.parse().ok()).or_else(|| usage()),
            "--document" => start.document = Some(args.next().unwrap_or_else(|| usage())),
            "--screenshot" => start.screenshot = Some(PathBuf::from(args.next().unwrap_or_else(|| usage()))),
            "--select" => start.select = args.next().unwrap_or_else(|| usage()).split(',').map(|s| s.trim().to_string()).collect(),
            "--panel" => start.panel = Some(args.next().unwrap_or_else(|| usage())),
            "--component" => start.component = Some(args.next().unwrap_or_else(|| usage())),
            "--size" => {
                let v = args.next().unwrap_or_else(|| usage());
                let mut it = v.split('x').filter_map(|n| n.parse::<f32>().ok());
                if let (Some(w), Some(h)) = (it.next(), it.next()) {
                    size = [w, h];
                }
            }
            "--write-demo" => {
                let folder = PathBuf::from(args.next().unwrap_or_else(|| usage()));
                match hephaestus_agent_api::demo::flasher().and_then(|mut s| s.save(Some(&folder)).map(|_| s)) {
                    Ok(s) => {
                        println!("{}", s.files.as_ref().map(|f| f.manifest_path().display().to_string()).unwrap_or_default());
                        return;
                    }
                    Err(e) => {
                        eprintln!("hephaestus: {e}");
                        std::process::exit(1);
                    }
                }
            }
            "-h" | "--help" => usage(),
            other if !other.starts_with('-') => start.open = Some(PathBuf::from(other)),
            _ => usage(),
        }
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Hephaestus")
            .with_inner_size(size)
            .with_min_inner_size([900.0, 560.0])
            .with_app_id("hephaestus"),
        // Screenshot runs must not inherit (or overwrite) the user's saved workspace.
        persist_window: start.screenshot.is_none(),
        persistence_path: start
            .screenshot
            .as_ref()
            .map(|_| std::env::temp_dir().join(format!("hephaestus-shot-{}.ron", std::process::id()))),
        ..Default::default()
    };
    let result = eframe::run_native("Hephaestus", options, Box::new(move |cc| Ok(Box::new(HephaestusApp::new(cc, start)))));
    if let Err(e) = result {
        eprintln!("hephaestus: {e}");
        std::process::exit(1);
    }
}
