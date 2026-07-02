//! Heartbeat probing and connection-state projection for the NWC wallet runtime.

use super::*;

use nmp_core::substrate::WalletKernelAccess;
use nmp_core::OutboundMessage;
use nmp_network::role::RelayRole;
use nmp_nwc::NwcMethod;
use serde_json::json;

use super::runtime_utils::encode_frame;
use crate::status::{NwcConnectionState, WalletStatus};

/// Result of a [`WalletRuntime::tick_heartbeat`] call.
pub struct HeartbeatOutbound {
    /// Ready-to-send frames (REQ resubscription during reconnect, if any).
    pub ready_frames: Vec<OutboundMessage>,
    /// `true` when the runtime wants a `get_info` probe to be sent for this
    /// relay. The caller must invoke `build_get_info_probe` (which needs
    /// `&mut Kernel`) after the `tick_heartbeat` lock window closes.
    pub needs_probe: bool,
    /// `true` when `connection_state` changed and the snapshot must be
    /// re-synced. Caller calls `sync_connection_state(kernel)`.
    pub state_changed: bool,
}

impl WalletRuntime {
    /// Heartbeat tick — called from the host-side `on_idle_tick` on every
    /// actor loop iteration.
    ///
    /// Returns outbound frames to send (zero, one probe, or a full
    /// resubscription batch) and a boolean indicating whether the snapshot
    /// should be marked dirty (`true` when `connection_state` changed).
    ///
    /// ## D8 compliance
    ///
    /// No sleep or blocking call inside. The decision is a pure wall-clock
    /// comparison of `now_secs` against the stored `last_probe_sent_secs`.
    /// The actor drives this from its idle section at ~250 ms cadence; the
    /// `HEARTBEAT_CADENCE_SECS` gate ensures probes fire at most once per
    /// window.
    ///
    /// ## Protocol
    ///
    /// 1. If no probe has been sent yet (or `last_probe_sent_secs == 0`) and
    ///    `HEARTBEAT_CADENCE_SECS` have elapsed since connect, send the first
    ///    probe.
    /// 2. On subsequent ticks: if `probe_outstanding` is still `true` when a
    ///    new cadence window opens, the previous probe timed out → increment
    ///    `consecutive_failures`.
    /// 3. When `consecutive_failures >= HEARTBEAT_MAX_FAILURES`, call
    ///    `resubscribe` and transition `connection_state` to `Reconnecting`.
    ///    After a second resubscribe round with no response (i.e. after ≥
    ///    `2 * HEARTBEAT_MAX_FAILURES` failures total), transition to
    ///    `TransportLost`.
    /// 4. Any successful response in `handle_nwc_text` resets
    ///    `consecutive_failures` to 0 and `connection_state` to `Connected`.
    pub fn tick_heartbeat(
        &mut self,
        now_secs: u64,
        cadence_secs: u64,
        max_failures: u32,
    ) -> HeartbeatOutbound {
        let conn = match self.connection.as_mut() {
            Some(c) => c,
            None => {
                return HeartbeatOutbound {
                    ready_frames: Vec::new(),
                    needs_probe: false,
                    state_changed: false,
                }
            }
        };

        // Before the first cadence window has elapsed, arm the baseline.
        if conn.last_probe_sent_secs == 0 {
            // Record "just connected" as the baseline so the first probe fires
            // ~cadence_secs after connect.
            conn.last_probe_sent_secs = now_secs;
            return HeartbeatOutbound {
                ready_frames: Vec::new(),
                needs_probe: false,
                state_changed: false,
            };
        }

        let elapsed = now_secs.saturating_sub(conn.last_probe_sent_secs);
        if elapsed < cadence_secs {
            // Still within the current cadence window — nothing to do.
            return HeartbeatOutbound {
                ready_frames: Vec::new(),
                needs_probe: false,
                state_changed: false,
            };
        }

        // A new cadence window opened. If a probe from the *previous* window
        // is still outstanding, that probe failed.
        let prev_state = conn.connection_state.clone();
        if conn.probe_outstanding {
            conn.consecutive_failures = conn.consecutive_failures.saturating_add(1);
            tracing::warn!(
                consecutive_failures = conn.consecutive_failures,
                last_probe_sent_secs = conn.last_probe_sent_secs,
                now_secs = now_secs,
                "nwc: heartbeat probe unanswered — consecutive failure #{n}",
                n = conn.consecutive_failures,
            );
        }

        // Transition connection_state based on failure count.
        let resubscribe_needed;
        if conn.consecutive_failures >= max_failures {
            // Use the total consecutive count to distinguish first-round vs.
            // second-round failure (≥ 2× threshold = TransportLost).
            if conn.consecutive_failures >= max_failures * 2 {
                conn.connection_state = Some(NwcConnectionState::TransportLost);
                // Do not keep resubscribing past TransportLost — the relay is
                // considered unreachable; flooding the outbound queue would be
                // wasteful. The user must manually reconnect.
                resubscribe_needed = false;
            } else {
                conn.connection_state = Some(NwcConnectionState::Reconnecting);
                resubscribe_needed = true;
            }
        } else {
            // Failure count below threshold — state stays at whatever it was.
            resubscribe_needed = false;
        }

        let state_changed = conn.connection_state != prev_state;

        // Advance the probe window baseline and arm the outstanding flag.
        conn.last_probe_sent_secs = now_secs;
        conn.probe_outstanding = true;

        // Capture fields needed to build the REQ frame (if resubscribing).
        let relay = conn.relay_url.clone();
        let sub_id = conn.sub_id.clone();
        let wallet_pubkey_hex = conn.wallet_pubkey_hex.clone();
        let client_pubkey_hex = conn.client_pubkey_hex.clone();

        let mut ready_frames = Vec::new();

        if resubscribe_needed {
            // Re-send REQ so the relay forwards kind:23195 again.
            let req_filter = json!({
                "kinds": [23195u32],
                "authors": [&wallet_pubkey_hex],
                "#p": [&client_pubkey_hex],
            });
            match encode_frame(&json!(["REQ", &sub_id, &req_filter])) {
                Ok(req_msg) => {
                    ready_frames.push(OutboundMessage::new(
                        RelayRole::Wallet,
                        relay.clone(),
                        req_msg,
                    ));
                }
                Err(e) => {
                    tracing::warn!("nwc: heartbeat REQ encode failed: {e}");
                }
            }
        }

        // Always request a get_info probe at the cadence boundary.
        HeartbeatOutbound {
            ready_frames,
            needs_probe: true,
            state_changed,
        }
    }

    /// Build and enqueue a `get_info` heartbeat probe for the connected relay.
    ///
    /// Returns `None` when no connection is active or frame encoding fails.
    /// The caller (`WalletInterceptor::on_idle_tick`) calls this after
    /// `tick_heartbeat` returns `needs_probe = true`, using a kernel reference
    /// that was not available inside the Kernel-free `tick_heartbeat` body.
    pub fn build_get_info_probe(
        &mut self,
        kernel: &dyn WalletKernelAccess,
    ) -> Option<OutboundMessage> {
        let relay = self.connection.as_ref()?.relay_url.clone();
        super::request_builder::build_request(
            self,
            kernel,
            &relay,
            NwcMethod::GetInfo,
            json!({}),
            None,
        )
        .map(|(msg, _id)| msg)
    }

    /// Push the current `connection_state` into the `status_slot` and mark the
    /// snapshot dirty. Called by the host interceptor when `tick_heartbeat`
    /// reports `state_changed = true`.
    pub fn sync_connection_state(&self, kernel: &dyn WalletKernelAccess) {
        sync_wallet_status(self, kernel);
    }
}

pub(super) fn sync_wallet_status(wallet: &WalletRuntime, kernel: &dyn WalletKernelAccess) {
    let status = wallet.connection.as_ref().map(|c| {
        let balance_sats = c.balance_msats.map(|m| m / 1000);
        WalletStatus {
            status: c.status.clone(),
            relay_url: c.relay_url.clone(),
            wallet_pubkey_hex: c.wallet_pubkey_hex.clone(),
            balance_msats: c.balance_msats,
            balance_sats,
            is_ready: c.status == "ready",
            is_connected: c.status == "connecting" || c.status == "ready",
            // V-79: project the real-time transport-health state.
            connection_state: c.connection_state.clone(),
        }
    });
    // D6 poison-lock recovery: a panicking thread must not permanently brick
    // the status projection.  Recover the guard via `unwrap_or_else` so a
    // single actor panic never leaves the slot locked forever.  Log the
    // recovery so it is observable without crashing the actor thread.
    //
    // `mark_changed_since_emit` is called ONLY when the slot write succeeded.
    // Calling it after a skipped write would tell the snapshot machinery there
    // is new data to emit when in fact the slot still holds its prior value —
    // that is a stale-balance defect (D6: poison is not fatal, but we must not
    // lie about what we wrote).
    let mut slot = match wallet.status_slot.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            tracing::error!(
                "nwc: status_slot lock was poisoned — recovering; \
                 wallet projection may be temporarily stale"
            );
            poisoned.into_inner()
        }
    };
    *slot = status;
    drop(slot); // release before marking dirty
    kernel.mark_changed_since_emit();
}
