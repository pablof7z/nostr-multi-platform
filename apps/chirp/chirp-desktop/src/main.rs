//! `chirp-desktop` — native desktop shell for the Chirp Nostr client.
//!
//! Boots the Chirp kernel through the C-ABI FFI seam (same path as iOS and
//! the TUI), receives FlatBuffer update frames, and renders the JSON snapshot
//! projections with egui.
//!
//! Run: `cargo run -p chirp-desktop`

mod app;
mod bridge;
mod bridge_accounts;
mod diagnostics_flag;
mod dm_panel;
mod keyring;
mod relation_counts;
mod render;
mod settings;
mod snapshot;
mod snapshot_decode;
mod timeline_panel;
mod zap_amount;

use app::DesktopApp;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([960.0, 740.0])
            .with_min_inner_size([560.0, 420.0])
            .with_title("Chirp — Nostr Multi-Platform"),
        ..Default::default()
    };

    eframe::run_native(
        "chirp-desktop",
        options,
        Box::new(|cc| Ok(Box::new(DesktopApp::new(cc)))),
    )
}
