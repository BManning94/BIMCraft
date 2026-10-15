//! `bimcraft`: the BIMCraft desktop app.

// A release build on Windows is a window, not a console program: no console opens behind it.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::process::ExitCode;

use bimcraft_core::engine::Session;
use bimcraft_core::sample::SAMPLE;
use bimcraft_ui_egui::{BimApp, Status, lang};
use serde_json::json;

const USAGE: &str = "USAGE: bimcraft [FILE] [--sample]
  FILE      a .bimcraft project to open, or an .ifc file to import
  --sample  start on the sample house";

fn main() -> ExitCode {
    let language = lang::from_env();
    let mut session = Session::new();
    // A blank project starts in the interface's language: its levels named in it.
    if std::env::args().len() == 1 {
        let _ = session.execute("new", &json!({"language": language.code()}));
    }
    let mut problem = None;
    for arg in std::env::args().skip(1) {
        let done = match arg.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            "--sample" => session.script(SAMPLE).map(|_| ()),
            flag if flag.starts_with('-') => {
                eprintln!("bimcraft: unknown option `{flag}`\n\n{USAGE}");
                return ExitCode::FAILURE;
            }
            file => session.execute("open", &json!({ "path": file })).map(|_| ()),
        };
        problem = done.err().or(problem);
    }
    // What the arguments built is where the user starts: nothing to undo, nothing unsaved.
    let (model, path) = ((*session.model).clone(), session.path.clone());
    session.reset(model, path);
    let mut app = BimApp::new(session);
    app.lang = language;
    if let Some(text) = problem {
        app.status = Status { text, error: true };
    }
    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_title("BIMCraft")
        .with_inner_size([1440.0, 900.0])
        .with_min_inner_size([900.0, 560.0]);
    // The logo, for the title bar and the taskbar.
    if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../../../assets/app-icon/bimcraft-256.png")) {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    match eframe::run_native("BIMCraft", options, Box::new(|_| Ok(Box::new(app)))) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("bimcraft: {e}");
            ExitCode::FAILURE
        }
    }
}
