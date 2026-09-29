#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod export;
mod font_export;
mod geometry;
mod model;
mod shared_ui;

fn main() -> eframe::Result<()> {
    // パニックを記録してクラッシュ原因を特定できるようにする
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let backtrace = std::backtrace::Backtrace::force_capture();
        let msg = format!("{info}\n\n{backtrace}\n");
        let _ = std::fs::write("/tmp/threadtype-panic.log", &msg);
        default_hook(info);
    }));

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1320.0, 900.0])
            .with_min_inner_size([900.0, 620.0])
            .with_title("ThreadType"),
        ..Default::default()
    };
    eframe::run_native(
        "ThreadType",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    )
}
