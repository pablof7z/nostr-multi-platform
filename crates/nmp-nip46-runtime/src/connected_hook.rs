//! `Nip46ConnectedHook` — [`RelayConnectedHook`] implementation.
//!
//! When the bunker relay (re)connects, this hook:
//!
//! 1. Locks the runtime briefly, calls `on_relay_connected` to collect REQ
//!    replay effects.
//! 2. For each [`Effect::Subscribe`]: calls
//!    `CommandSender::set_reconnect_preamble(RelayRole::Signer, ...)` to
//!    register the REQ frame as the worker's reconnect preamble.  On every
//!    subsequent (re)connect the worker injects this preamble at the FRONT of
//!    its outbound queue BEFORE the actor's `Opened` hook can post any EVENT
//!    commands.  This is the structural REQ-before-EVENT guarantee.
//!    The first connect's REQ was already sent by the session-init `Subscribe`
//!    effect in the interceptor; registering here arms all future reconnects.
//! 3. Reports a `"connected"` connection-state-changed event (preserves the
//!    V-14 / signer_broker:76 mapping).
//!
//! D8 — `on_relay_connected` MUST NOT block: the hook spawns no threads and
//! does no I/O.  The `CommandSender::set_reconnect_preamble` call posts one
//! `ActorCommand::SetReconnectPreamble` and returns immediately; the actor
//! thread forwards it to the pool/worker on its next command-processing cycle.

use std::time::{SystemTime, UNIX_EPOCH};

use nmp_core::substrate::RelayConnectedHook;
use nmp_core::CommandSender;
use nmp_network::role::RelayRole;
use nmp_nip46::Effect;

use crate::runtime::Nip46RuntimeHandle;

/// Connected-hook that registers the NIP-46 subscription as the worker
/// preamble on every relay connect, guaranteeing REQ-before-EVENT ordering.
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
        // #2976 — also read the account's user pubkey (learned at SignerReady)
        // so the "connected" health event below is attributed per-identity.
        let (effects, user_pubkey_hex) = {
            let Ok(mut guard) = self.runtime.lock() else {
                return;
            };
            let Some(rt) = guard.as_mut() else { return };
            let effects = rt.on_relay_connected(relay_url, is_reconnect, now);
            (effects, rt.user_pubkey().map(|pk| pk.to_hex()))
        }; // lock released

        if effects.is_empty() {
            return;
        }

        // ── Phase 2: register preamble / enqueue outbound (no lock held) ─
        for effect in effects {
            match effect {
                Effect::Subscribe {
                    relay_url: eff_url,
                    frame,
                } => {
                    // REQ-before-EVENT fix: register the REQ as the worker's
                    // reconnect preamble instead of posting it via
                    // enqueue_outbound.  The worker will inject this frame at
                    // the FRONT of its pending queue on every (re)connect
                    // BEFORE the actor's Opened hook can enqueue sign EVENTs.
                    // The first connect's REQ was sent by the session-init
                    // Subscribe effect (interceptor.rs translate_effects);
                    // this call arms subsequent reconnects structurally.
                    command_sender.set_reconnect_preamble(RelayRole::Signer, eff_url, vec![frame]);
                }
                Effect::SendFrame {
                    relay_url: eff_url,
                    text,
                } => {
                    // Reconnect can also emit SendFrame effects (e.g. a
                    // `connect` resend).  Deliver them in order via the
                    // existing enqueue path.
                    command_sender.enqueue_outbound(RelayRole::Signer, eff_url, text);
                }
                Effect::Progress {
                    stage,
                    code,
                    detail,
                } => {
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
        // #2976 — attributed to this session's account (`None` only if the
        // reconnect somehow fires before SignerReady learned the identity).
        command_sender.bunker_connection_state_changed(user_pubkey_hex, "connected".to_string(), None);
    }
}

// ─── helpers ─────────────────────────────────────────────────────────────────

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
