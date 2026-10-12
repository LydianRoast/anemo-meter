#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1060.0, 780.0])
            .with_min_inner_size([360.0, 520.0]),
        ..Default::default()
    };
    eframe::run_native(
        "ANE-200W Console",
        options,
        Box::new(|cc| Ok(Box::new(ane_app::app::Console::new(cc, None)))),
    )
}
