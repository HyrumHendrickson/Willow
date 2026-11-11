//! Willow: a visual editor for static websites hosted on GitHub Pages.

mod app;
mod export;
mod model;
mod presets;
mod richtext;
mod security;
mod server;
mod snap;

use eframe::egui;

const APP_NAME: &str = "Willow";
const ICON_PNG: &[u8] = include_bytes!("../assets/icon.png");

/// Window icon decoded from the bundled PNG; the window simply has no custom icon if decoding fails.
fn icon() -> Option<egui::IconData> {
    let img = image::load_from_memory(ICON_PNG).ok()?.into_rgba8();
    let (width, height) = img.dimensions();
    Some(egui::IconData { rgba: img.into_raw(), width, height })
}

fn main() -> eframe::Result {
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([1360.0, 860.0])
        .with_min_inner_size([900.0, 600.0])
        .with_title(APP_NAME);
    if let Some(icon) = icon() {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native(APP_NAME, options, Box::new(|cc| Ok(Box::new(app::App::new(cc)))))
}
