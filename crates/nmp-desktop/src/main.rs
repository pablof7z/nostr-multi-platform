//! `nmp-desktop` — a native desktop shell that runs the NMP kernel
//! **in-process** (Rust calling Rust; no FFI seam).
//!
//! It spawns the kernel actor (`nmp_core::testing::spawn_actor`), drives it
//! with the generic `ActorCommand` surface, and renders the JSON `KernelUpdate`
//! snapshots as a live Nostr timeline + compose box. No app nouns are added to
//! `nmp-core` (D0); the UI holds no state beyond the latest snapshot (D7);
//! rendering is best-effort (D1).
//!
//! Run: `cargo run -p nmp-desktop`

mod app;
mod bridge;
mod render;
mod snapshot;

use app::DesktopApp;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([900.0, 720.0])
            .with_min_inner_size([520.0, 400.0])
            .with_title("NMP — Nostr Multi-Platform (in-process kernel)"),
        ..Default::default()
    };

    eframe::run_native(
        "nmp-desktop",
        options,
        Box::new(|cc| Ok(Box::new(DesktopApp::new(cc)))),
    )
}
