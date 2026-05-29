//! Host-side glue for the V-38 NIP-47 wallet stack.
//!
//! `nmp-nip47` owns the runtime + action modules + status type. This module
//! is the Chirp-specific composition root: builds the [`WalletStatusSlot`],
//! the [`WalletRuntime`], registers the three action modules, installs the
//! relay-text interceptor + the runtime handle, and registers the
//! `"wallet"` snapshot projection.

use std::sync::Arc;

use nmp_core::substrate::RelayTextInterceptor;
use nmp_core::{Kernel, OutboundMessage};
use nmp_ffi::NmpApp;

use nmp_nip47::{
    install_wallet_runtime, new_wallet_runtime_handle, WalletConnectModule, WalletDisconnectModule,
    WalletPayInvoiceModule, WalletRuntime, WalletRuntimeHandle, WalletStatusSlot,
};

/// Adapter that wires the wallet runtime's [`nmp_nip47::handle_nwc_text`]
/// (via [`nmp_nip47::dispatch_nwc_relay_text`]) into the substrate-generic
/// [`RelayTextInterceptor`] trait the actor calls.
///
/// `on_idle_tick` implements two wall-clock-gated sweeps (D8 — no sleep/loop):
///
/// 1. **V-64 TTL sweep** — expires `pending_payments` entries older than
///    `PENDING_PAYMENT_TTL_SECS` and records them as timed-out action
///    failures. Fires even when the NWC relay is completely silent.
///
/// 2. **V-79 heartbeat** — sends a `get_info` probe at
///    `HEARTBEAT_CADENCE_SECS` cadence. On `HEARTBEAT_MAX_FAILURES`
///    consecutive unanswered probes, re-sends the REQ subscription
///    (`Reconnecting`). If probes still go unanswered after a second round,
///    transitions `connection_state` to `TransportLost`.
struct WalletInterceptor {
    runtime: WalletRuntimeHandle,
}

impl RelayTextInterceptor for WalletInterceptor {
    fn on_relay_text(
        &self,
        kernel: &mut Kernel,
        relay_url: &str,
        text: &str,
    ) -> Vec<OutboundMessage> {
        nmp_nip47::dispatch_nwc_relay_text(&self.runtime, kernel, relay_url, text)
    }

    fn on_idle_tick(&self, kernel: &mut Kernel) -> Vec<OutboundMessage> {
        let now_secs = kernel.now_secs();

        // ── Phase 1: run sweeps inside the lock, collect results ──────────────
        let (failures, heartbeat, ready_frames) = {
            let Ok(mut guard) = self.runtime.lock() else {
                return Vec::new();
            };
            let Some(rt) = guard.as_mut() else {
                return Vec::new();
            };

            // V-64: sweep expired pending_payments.
            let failures = rt.sweep_expired_payments(now_secs, nmp_nip47::PENDING_PAYMENT_TTL_SECS);

            // V-79: heartbeat tick — pure wall-clock gated, Kernel-free.
            let heartbeat = rt.tick_heartbeat(
                now_secs,
                nmp_nip47::HEARTBEAT_CADENCE_SECS,
                nmp_nip47::HEARTBEAT_MAX_FAILURES,
            );
            let ready_frames = heartbeat.ready_frames.clone();
            (failures, heartbeat, ready_frames)
        }; // lock dropped

        // ── Phase 2: Kernel-touching work (lock released) ─────────────────────

        // Record payment timeouts.
        for (cid, reason) in failures {
            kernel.record_action_failure(cid, reason);
        }

        let mut outbound = ready_frames;

        // If connection_state changed, sync the snapshot slot.
        if heartbeat.state_changed {
            let Ok(mut guard) = self.runtime.lock() else {
                return outbound;
            };
            if let Some(rt) = guard.as_mut() {
                rt.sync_connection_state(kernel);
            }
        }

        // Build and enqueue the get_info probe if needed.
        if heartbeat.needs_probe {
            let Ok(mut guard) = self.runtime.lock() else {
                return outbound;
            };
            if let Some(rt) = guard.as_mut() {
                if let Some(msg) = rt.build_get_info_probe(kernel) {
                    outbound.push(msg);
                }
            }
        }

        outbound
    }
}

/// Register the NIP-47 wallet stack on `app`. Called by
/// `nmp_app_chirp_register` when the `wallet` feature is on.
pub(crate) fn register_nip47_wallet(app: &mut NmpApp) {
    // 1. Action modules — exposed under `nmp.wallet.{connect,disconnect,
    //    pay_invoice}` so the existing `nmp_app_wallet_*` FFI shims (which
    //    route through `dispatch_action` post-V-38) reach the runtime.
    app.register_action::<WalletConnectModule>();
    app.register_action::<WalletDisconnectModule>();
    app.register_action::<WalletPayInvoiceModule>();

    // 2. Shared status slot — one `Arc` clone goes to the runtime (sole
    //    writer, D4), the other is captured below by the `"wallet"`
    //    snapshot projection closure.
    let status_slot: WalletStatusSlot = nmp_nip47::new_wallet_status_slot();
    let projection_slot = Arc::clone(&status_slot);

    // 3. Wallet runtime — held inside an `Arc<Mutex<Option<WalletRuntime>>>`
    //    handle the `ProtocolCommand` impls and the interceptor both lock.
    let runtime = WalletRuntime::new(status_slot);
    let handle: WalletRuntimeHandle = new_wallet_runtime_handle();
    if let Ok(mut guard) = handle.lock() {
        *guard = Some(runtime);
    }

    // 4. Install the process-wide active handle so the action-seam executor
    //    (a static `fn`) can fetch it without an `NmpApp` reference. Silent
    //    second-install is OK (e.g. tests).
    let _ = install_wallet_runtime(Arc::clone(&handle));

    // 5. Substrate-generic relay-text interceptor — the actor calls this
    //    for every inbound text frame.
    app.add_relay_text_interceptor(Arc::new(WalletInterceptor {
        runtime: Arc::clone(&handle),
    }));

    // 6. The `"wallet"` snapshot projection — reads `status_slot`, mirrors
    //    the pre-V-38 closure that lived inside `nmp_app_new`.
    app.register_snapshot_projection("wallet", move || match projection_slot.lock() {
        Ok(slot) => slot
            .as_ref()
            .map(|status| serde_json::to_value(status).unwrap_or(serde_json::Value::Null))
            .unwrap_or(serde_json::Value::Null),
        Err(_) => serde_json::Value::Null,
    });
}
