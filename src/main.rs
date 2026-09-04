#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod document;
mod fs_tree;
mod markdown;
mod preview;
mod registry;
mod state;
mod theme;

use std::path::PathBuf;

/// Window + taskbar icon (blue "L"). This overrides eframe's default
/// black "e" icon; the same artwork lives in `assets/app.ico` (multi-size)
/// for Explorer/shortcuts.
const APP_ICON_PNG: &[u8] = include_bytes!("../assets/app-icon.png");

const HELP: &str = "\
Lihati \u{2014} a minimal Markdown editor for Windows

Usage:
  lihati [FILE.md]        Open a Markdown file (also used by \"Open with\")
  lihati --register       Register Lihati in the \"Open with\" menu for .md files
  lihati --unregister     Remove the file association registration
  lihati --version        Print version
  lihati --help           Show this help
";

fn main() -> Result<(), String> {
    let mut flags: Vec<String> = Vec::new();
    let mut file: Option<PathBuf> = None;

    for arg in std::env::args_os().skip(1) {
        let as_str = arg.to_string_lossy();
        if as_str.starts_with('-') && as_str.len() > 1 {
            flags.push(as_str.into_owned());
        } else if file.is_none() {
            file = Some(PathBuf::from(arg));
        }
    }

    for flag in &flags {
        match flag.as_str() {
            "--register" => return registry::register(),
            "--unregister" => return registry::unregister(),
            "--help" | "-h" => {
                print!("{}", HELP);
                return Ok(());
            }
            "--version" | "-V" => {
                println!("lihati {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            _ => {}
        }
    }

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1300.0, 850.0])
            .with_min_inner_size([720.0, 480.0])
            .with_title("Lihati")
            .with_icon(
                eframe::icon_data::from_png_bytes(APP_ICON_PNG).expect("invalid app icon"),
            ),
        persist_window: true,
        ..Default::default()
    };

    eframe::run_native(
        "Lihati",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, file)))),
    )
    .map_err(|e| format!("eframe error: {e}"))
}
