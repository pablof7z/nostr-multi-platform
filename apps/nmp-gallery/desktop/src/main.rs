//! `nmp-gallery-desktop` — component gallery for NMP desktop widgets.
//!
//! Mirrors nmp-gallery-tui: in-process kernel, reactive embeds,
//! same component registry and examples. Run:
//!     cargo run -p nmp-gallery-desktop

use eframe::NativeOptions;
use egui::ViewportBuilder;

mod app;
mod bridge;
mod components;
mod render;

use app::GalleryApp;

fn main() -> eframe::Result<()> {
    let options = NativeOptions {
        viewport: ViewportBuilder::default()
            .with_inner_size([1100.0, 750.0])
            .with_min_inner_size([600.0, 400.0])
            .with_title("NMP Desktop Component Gallery"),
        ..Default::default()
    };

    eframe::run_native(
        "nmp-gallery-desktop",
        options,
        Box::new(|cc| Ok(Box::new(GalleryApp::new(cc)))),
    )
}
