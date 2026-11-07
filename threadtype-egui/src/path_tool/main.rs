#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use pathweaver_core::{appearance, appearance_assets, appearance_effects, corners, model, outline};
mod appearance_ui;
mod editor;
mod icons;
#[path = "../shared_ui.rs"]
mod shared_ui;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        multisampling: 4,
        viewport: egui::ViewportBuilder::default()
            .with_title("PathWeaver")
            .with_inner_size([1380.0, 900.0])
            .with_min_inner_size([980.0, 640.0]),
        ..Default::default()
    };
    eframe::run_native(
        "PathWeaver",
        options,
        Box::new(|cc| Ok(Box::new(editor::Editor::new(cc)))),
    )
}
