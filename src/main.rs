mod app;
mod export;
mod model;
mod server;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1360.0, 860.0])
            .with_min_inner_size([900.0, 600.0])
            .with_title("Microschool Site Builder"),
        ..Default::default()
    };
    eframe::run_native(
        "Microschool Site Builder",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    )
}
