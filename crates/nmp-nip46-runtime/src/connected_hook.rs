//! `Nip46ConnectedHook` — [`RelayConnectedHook`] implementation.
//!
//! When the bunker relay (re)connects, this hook:
//!
//! 1. Locks the runtime briefly, calls `on_relay_connected` to collect REQ
//!    replay effects (and arms the 60 s step deadline).
//! 2. For each [`Effect::Subscribe`]: posts
//!    `CommandSender::enqueue_outbound(RelayRole::Signer, ...)` so the actor
//!    thread delivers the REQ frame BEFORE any EVENT can arrive on the new
//!    socket.  This is the relay-lifetime contract: **REQ-before-EVENT on
//!    every (re)connect**.
//! 3. Reports a `"connected"` connection-state-changed event (preserves the
//!    V-14 / signer_broker:76 mapping).
//!
//! D8 — `on_relay_connected` MUST NOT block: the hook spawns no threads and
//! does no I/O.  The `CommandSender::enqueue_outbound` call posts one
//! `ActorCommand::EnqueueOutbound` per frame and returns immediately; the
//! actor thread dispatches these in order (channel FIFO) before the relay can
//! deliver the first EVENT.

use std::time::{SystemTime, UNIX_EPOCH};

use nmp_core::substrate::RelayConnectedHook;
use nmp_core::CommandSender;
use nmp_network::role::RelayRole;
use nmp_nip46::Effect;

use crate::runtime::Nip46RuntimeHandle;

/// Connected-hook that replays the NIP-46 subscription on every relay connect.
pub(crate) struct Nip46ConnectedHook {
    pub(crate) runtime: Nip46RuntimeHandle,
}

impl RelayConnectedHook for Nip46ConnectedHook {
    fn on_relay_connected(
        &self,
        relay_url: &str,
        is_reconnect: bool,
        command_sender: CommandSender,
    ) {
        let now = now_secs();

        // ── Phase 1: drive on_relay_connected under lock ──────────────────
        let effects = {
            let Ok(mut guard) = self.runtime.lock() else { return };
            let Some(rt) = guard.as_mut() else { return };
            rt.on_relay_connected(relay_url, is_reconnect, now)
        }; // lock released

        if effects.is_empty() {
            return;
        }

        // ── Phase 2: enqueue outbound (no lock held) ──────────────────────
        // Post each outbound frame as `EnqueueOutbound` so the actor thread
        // delivers them in FIFO order on the (possibly fresh) socket before
        // any EVENT can arrive.  Using the channel guarantees ordering with
        // subsequent relay-event deliveries that also arrive through the
        // actor's single inbox.
        for effect in effects {
            match effect {
                Effect::Subscribe { relay_url: eff_url, frame } => {
                    // Replay the REQ subscription.  This is the critical
                    // REQ-before-EVENT guarantee: the actor processes this
                    // `EnqueueOutbound` command before the relay worker can
                    // deliver any inbound EVENT (the channel is FIFO and the
                    // relay worker also sends through the pool, not directly
                    // into the actor inbox).
                    command_sender.enqueue_outbound(
                        RelayRole::Signer,
                        eff_url,
                        frame,
                    );
                }
                Effect::SendFrame { relay_url: eff_url, text } => {
                    // Reconnect can also emit SendFrame effects (e.g. a
                    // `connect` resend).  Deliver them in order.
                    command_sender.enqueue_outbound(
                        RelayRole::Signer,
                        eff_url,
                        text,
                    );
                }
                Effect::Progress { stage, code, detail } => {
                    command_sender.bunker_handshake_progress(stage, code, detail);
                }
                Effect::Error { error } => {
                    tracing::warn!(
                        error = %error,
                        "nip46-runtime: error on relay connected"
                    );
                    command_sender.bunker_handshake_progress(
                        "failed".to_string(),
                        None,
                        Some(error.to_string()),
                    );
                }
                // SignerReady and DeliverResponse cannot arrive from
                // on_relay_connected (no relay event was decoded).
                Effect::SignerReady(_) | Effect::DeliverResponse { .. } => {}
            }
        }

        // Report relay connection state for UI liveness (V-14 / signer_broker:76).
        command_sender.bunker_connection_state_changed("connected".to_string(), None);
    }
}

// ─── helpers ─────────────────────────────────────────────────────────────────

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
