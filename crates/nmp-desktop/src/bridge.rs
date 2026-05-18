//! In-process kernel bridge.
//!
//! The desktop shell runs the kernel actor on its own thread (Rust→Rust — no
//! FFI boundary) and talks to it through the generic `ActorCommand` channel,
//! exactly as the iOS FFI layer does. A reader thread drains JSON snapshots,
//! decodes them into [`Snapshot`], and parks the freshest one behind a mutex.

use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread;

use nmp_core::testing::{spawn_actor, ActorCommand};

use crate::snapshot::Snapshot;

/// Shared latest-snapshot cell. The reader thread writes; the egui frame reads.
pub type SharedSnapshot = Arc<Mutex<Option<Snapshot>>>;

/// Handle the UI keeps to dispatch actions into the kernel.
pub struct KernelBridge {
    tx: Sender<ActorCommand>,
    pub latest: SharedSnapshot,
}

impl KernelBridge {
    /// Spawn the actor, start it against the default public relays, and wire a
    /// reader thread that repaints `egui_ctx` whenever a new snapshot lands.
    pub fn start(egui_ctx: egui::Context) -> Self {
        let (tx, rx) = spawn_actor();
        let latest: SharedSnapshot = Arc::new(Mutex::new(None));

        let reader_latest = Arc::clone(&latest);
        thread::spawn(move || {
            // `rx` yields one JSON line per kernel emit until the actor stops.
            for line in rx {
                if let Some(snap) = Snapshot::parse(&line) {
                    if let Ok(mut slot) = reader_latest.lock() {
                        *slot = Some(snap);
                    }
                    egui_ctx.request_repaint();
                }
            }
        });

        // Start with the kernel defaults (80 visible items, 4 Hz emit). This
        // alone bootstraps a live seed timeline against wss://relay.primal.net.
        let _ = tx.send(ActorCommand::Start {
            visible_limit: 80,
            emit_hz: 4,
        });

        Self { tx, latest }
    }

    /// Read the current snapshot (clone — the UI never holds the lock across
    /// a frame, and never mutates kernel state: D7).
    pub fn snapshot(&self) -> Option<Snapshot> {
        self.latest.lock().ok().and_then(|s| s.clone())
    }

    /// Publish a kind:1 note with the active account (no-op server-side until
    /// an account is signed in — the kernel surfaces that via `last_error_toast`).
    pub fn publish_note(&self, content: String) {
        let _ = self.tx.send(ActorCommand::PublishNote {
            content,
            reply_to_id: None,
        });
    }

    /// Generate a fresh keypair and sign in with it (so compose can publish).
    pub fn create_account(&self) {
        let _ = self.tx.send(ActorCommand::CreateAccount);
    }

    /// Sign in with an existing `nsec…` / hex secret.
    pub fn sign_in_nsec(&self, secret: String) {
        let _ = self.tx.send(ActorCommand::SignInNsec { secret });
    }

    /// (Re)open the following-timeline for the active account.
    pub fn open_timeline(&self) {
        let _ = self.tx.send(ActorCommand::OpenTimeline);
    }
}
