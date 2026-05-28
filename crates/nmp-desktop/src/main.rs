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
mod message;
mod render;
mod snapshot;

use app::{update, view, DesktopApp};
use bridge::subscription;

fn main() -> iced::Result {
    iced::application(DesktopApp::new, update, view)
        .title("NMP — Nostr Multi-Platform (in-process kernel)")
        .subscription(|_state| subscription())
        .run()
}
