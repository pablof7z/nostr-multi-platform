//! In-process kernel bridge with claim support.
//!
//! Mirrors nmp-desktop/src/bridge.rs but adds EventClaimSink so embeds
//! resolve reactively (ADR-0034). The gallery spawns the actor in-process
//! (Rust→Rust, no FFI), drives it via ActorCommand, and decodes snapshot
//! pushes into serde_json::Value for EmbedHostState.

use std::collections::BTreeSet;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

use nmp_content::EventClaimSink;
use nmp_core::testing::{spawn_actor, ActorCommand};
use nmp_core::{decode_update_frame, UpdateEnvelope};
use serde_json::Value;

/// Shared latest snapshot cell. The reader thread writes; the egui frame reads.
pub type SharedSnapshot = Arc<Mutex<Option<Value>>>;

/// Handle the UI keeps to dispatch actions and claims into the kernel.
pub struct GalleryBridge {
    tx: Sender<ActorCommand>,
    pub latest: SharedSnapshot,
}

impl GalleryBridge {
    /// Spawn the actor, start it, and wire a reader thread that repaints
    /// egui_ctx whenever a new snapshot lands.
    #[must_use]
    pub fn start(egui_ctx: egui::Context) -> Self {
        let (tx, rx) = spawn_actor();
        let latest: SharedSnapshot = Arc::new(Mutex::new(None));

        let reader_latest = Arc::clone(&latest);
        thread::spawn(move || {
            for frame in rx {
                let env: UpdateEnvelope = match decode_update_frame(&frame) {
                    Ok(e) => e,
                    Err(_) => continue,
                };
                let UpdateEnvelope::Snapshot(v) = env else {
                    continue;
                };
                if let Ok(mut slot) = reader_latest.lock() {
                    *slot = Some(v);
                }
                egui_ctx.request_repaint();
            }
        });

        let _ = tx.send(ActorCommand::Start {
            visible_limit: 80,
            emit_hz: 4,
        });

        Self { tx, latest }
    }

    /// Read the current snapshot Value (clone — the UI never holds the lock
    /// across a frame, and never mutates kernel state: D7).
    #[must_use]
    pub fn snapshot_value(&self) -> Option<Value> {
        self.latest.lock().ok().and_then(|s| s.clone())
    }

    /// Claim a profile (kind:0) for the given pubkey.
    pub fn claim_profile(&self, pubkey: &str, consumer_id: &str) {
        let _ = self.tx.send(ActorCommand::ClaimProfile {
            pubkey: pubkey.to_string(),
            consumer_id: consumer_id.to_string(),
        });
    }

    /// Release a previously claimed profile.
    pub fn release_profile(&self, pubkey: &str, consumer_id: &str) {
        let _ = self.tx.send(ActorCommand::ReleaseProfile {
            pubkey: pubkey.to_string(),
            consumer_id: consumer_id.to_string(),
        });
    }

    /// Claim an event (nevent/note/naddr) by URI.
    pub fn claim_event(&self, uri: &str, consumer_id: &str) {
        let _ = self.tx.send(ActorCommand::ClaimEvent {
            uri: uri.to_string(),
            consumer_id: consumer_id.to_string(),
        });
    }

    /// Release a previously claimed event.
    pub fn release_event(&self, uri: &str, consumer_id: &str) {
        let _ = self.tx.send(ActorCommand::ReleaseEvent {
            uri: uri.to_string(),
            consumer_id: consumer_id.to_string(),
        });
    }
}

impl EventClaimSink for GalleryBridge {
    fn claim(&self, uri: &str, consumer_id: &str) {
        self.claim_event(uri, consumer_id);
    }

    fn release(&self, uri: &str, consumer_id: &str) {
        self.release_event(uri, consumer_id);
    }
}
