//! `nmp-gallery-desktop` — component gallery for NMP desktop widgets.
//!
//! Run: `cargo run -p nmp-gallery-desktop`

use eframe::NativeOptions;
use egui::ViewportBuilder;

mod components;
mod gallery;

use gallery::GalleryApp;

fn main() -> eframe::Result<()> {
    let options = NativeOptions {
        viewport: ViewportBuilder::default()
            .with_inner_size([800.0, 600.0])
            .with_min_inner_size([400.0, 300.0])
            .with_title("NMP Desktop Component Gallery"),
        ..Default::default()
    };

    eframe::run_native(
        "nmp-gallery-desktop",
        options,
        Box::new(|_cc| Ok(Box::new(GalleryApp::new()))),
    )
}
