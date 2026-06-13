//! Reusable host-side composition for the NIP-47 wallet stack.
//!
//! This module is the canonical, app-neutral wiring for Nostr Wallet Connect.
//! It was lifted out of `nmp-app-chirp::wallet_runtime` (V-95 / issue #619): the
//! wallet runtime install MUST happen during the app's *config* phase, before
//! the kernel starts and reads its wiring slots. Keeping the wiring inside an
//! app crate made it a per-app re-derivation AND left the install-before-start
//! ordering unenforced. Moving it here makes the wiring reusable by any app and
//! lets `nmp-defaults` expose it as a typed `NmpAppBuilder` step
//! (`.with_wallet()`), so a Rust caller cannot reach `start()` without the
//! runtime installed.
//!
//! The function depends only on the substrate-generic [`AppHost`] trait — it
//! names no `NmpApp` and no FFI type, so it is layer-clean (Layer-4 NIP crate
//! wiring against the Layer-3 `nmp-core` host trait). The one host-specific
//! input — the durable storage path — is passed in by the caller (the builder
//! reads it from the un-started app and hands it here).

use std::sync::Arc;

use nmp_core::substrate::{AppHost, RelayTextInterceptor};
use nmp_core::{Kernel, OutboundMessage, TypedProjectionData};

use crate::runtime::{
    install_wallet_runtime, new_wallet_runtime_handle, WalletRuntimeHandle,
};
use crate::status::WalletStatusSlot;
use crate::dispatch_nwc_relay_text;
use crate::{
    encode_wallet_status, new_wallet_status_slot, FsPaymentStore, WalletConnectModule,
    WalletDisconnectModule, WalletPayInvoiceModule, WalletRuntime, PENDING_PAYMENT_TTL_SECS,
    WALLET_STATUS_FILE_IDENTIFIER, WALLET_STATUS_SCHEMA_ID, WALLET_STATUS_SCHEMA_VERSION,
};

/// Adapter that wires the wallet runtime's [`dispatch_nwc_relay_text`] into the
/// substrate-generic [`RelayTextInterceptor`] trait the actor calls.
///
/// `on_idle_tick` implements two wall-clock-gated sweeps (D8 — no sleep/loop):
///
/// 1. **TTL sweep (double-pay-safe)** — transitions `pending_payments` entries
///    older than `PENDING_PAYMENT_TTL_SECS` to the durable `Unknown` state for
///    `lookup_invoice` reconciliation on reconnect, instead of recording a
///    failure (a TTL elapsing never means the HTLC failed). Fires even when the
///    NWC relay is completely silent.
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
        dispatch_nwc_relay_text(&self.runtime, kernel, relay_url, text)
    }

    fn on_idle_tick(&self, kernel: &mut Kernel) -> Vec<OutboundMessage> {
        let now_secs = kernel.now_secs();

        // ── Phase 1: run sweeps inside the lock, collect results ──────────────
        let (heartbeat, ready_frames) = {
            let Ok(mut guard) = self.runtime.lock() else {
                return Vec::new();
            };
            let Some(rt) = guard.as_mut() else {
                return Vec::new();
            };

            // Double-pay-safe TTL sweep: transitions expired pending_payments to
            // the durable `Unknown` state (for `lookup_invoice` reconciliation on
            // reconnect) instead of recording failures. A TTL elapsing never
            // means the HTLC failed — so the returned outcomes are observational
            // only and we deliberately do NOT call record_action_failure on them.
            let _swept = rt.sweep_expired_payments(now_secs, PENDING_PAYMENT_TTL_SECS);

            // V-79: heartbeat tick — pure wall-clock gated, Kernel-free.
            let heartbeat = rt.tick_heartbeat(
                now_secs,
                crate::HEARTBEAT_CADENCE_SECS,
                crate::HEARTBEAT_MAX_FAILURES,
            );
            let ready_frames = heartbeat.ready_frames.clone();
            (heartbeat, ready_frames)
        }; // lock dropped

        // ── Phase 2: Kernel-touching work (lock released) ─────────────────────

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

/// Register the NIP-47 wallet stack on `app`.
///
/// This is the reusable composition root for Nostr Wallet Connect: it
///
/// 1. registers the three `nmp.wallet.{connect,disconnect,pay_invoice}`
///    action modules so the dispatch seam reaches the runtime;
/// 2. constructs the [`WalletRuntime`] behind a shared handle, installing the
///    durable [`FsPaymentStore`] when `storage_path` is `Some` (the
///    double-pay-safe write-before-enqueue path);
/// 3. installs the process-wide active runtime handle via
///    [`install_wallet_runtime`] — the action-seam executor (`execute` is a
///    static `fn`) fetches it through `active_wallet_runtime` without an
///    `NmpApp` reference;
/// 4. installs the relay-text interceptor that drives inbound kind:23195
///    decoding + the V-79 heartbeat/TTL sweeps;
/// 5. registers the generic + typed `"wallet"` snapshot projections.
///
/// MUST run during the config phase, before the kernel starts (the actor reads
/// the interceptor + action registry once, at kernel construction). The
/// `NmpAppBuilder::with_wallet` step in `nmp-defaults` enforces this ordering
/// at compile time for Rust callers.
pub fn register_wallet(app: &mut impl AppHost, storage_path: Option<String>) {
    // 1. Action modules — exposed under `nmp.wallet.{connect,disconnect,
    //    pay_invoice}` so dispatch reaches the runtime.
    app.register_action::<WalletConnectModule>();
    app.register_action::<WalletDisconnectModule>();
    app.register_action::<WalletPayInvoiceModule>();

    // 2. Shared status slot — one `Arc` clone goes to the runtime (sole
    //    writer, D4), the others are captured below by the `"wallet"`
    //    generic + typed snapshot projection closures.
    let status_slot: WalletStatusSlot = new_wallet_status_slot();
    let projection_slot = Arc::clone(&status_slot);
    let typed_projection_slot = Arc::clone(&status_slot);

    // 3. Wallet runtime — held inside an `Arc<Mutex<Option<WalletRuntime>>>`
    //    handle the `ProtocolCommand` impls and the interceptor both lock.
    let mut runtime = WalletRuntime::new(status_slot);

    // Install the durable payment store when a persistent storage path is
    // configured. This activates the double-pay-safe write-before-enqueue +
    // tri-state reconciliation path: in-flight payments survive a process kill
    // and a TTL/disconnect transitions them to `Unknown` (never `Failed`) so a
    // `lookup_invoice` on reconnect settles them to the true outcome.
    if let Some(storage_path) = storage_path.filter(|p| !p.trim().is_empty()) {
        runtime.set_payment_store(FsPaymentStore::new(storage_path));
    }

    let handle: WalletRuntimeHandle = new_wallet_runtime_handle();
    if let Ok(mut guard) = handle.lock() {
        *guard = Some(runtime);
    }

    // 4. Install the process-wide active handle so the action-seam executor
    //    (a static `fn`) can fetch it without an `NmpApp` reference. Silent
    //    second-install is OK (e.g. tests) — the first handle wins.
    let _ = install_wallet_runtime(Arc::clone(&handle));

    // 5. Substrate-generic relay-text interceptor — the actor calls this for
    //    every inbound text frame.
    app.add_relay_text_interceptor(Arc::new(WalletInterceptor {
        runtime: Arc::clone(&handle),
    }));

    // 6. The `"wallet"` snapshot projection — reads `status_slot`.
    app.register_snapshot_projection("wallet", move || match projection_slot.lock() {
        Ok(slot) => slot
            .as_ref()
            .map(|status| serde_json::to_value(status).unwrap_or(serde_json::Value::Null))
            .unwrap_or(serde_json::Value::Null),
        Err(_) => serde_json::Value::Null,
    });

    // 7. The typed `"wallet"` sidecar (ADR-0037) — emitted ALONGSIDE the
    //    generic `Value` projection above, never replacing it.
    app.register_typed_snapshot_projection("wallet", move || {
        wallet_typed_projection(&typed_projection_slot)
    });
}

/// Build the typed `"wallet"` sidecar entry from the shared status slot, or
/// `None` when no wallet is connected this session (the slot holds `None`).
///
/// Extracted from the `register_typed_snapshot_projection` closure so the
/// registration's schema identity (`key` / `schema_id` / `file_identifier`) and
/// the encode are unit-testable without spinning the actor.
pub fn wallet_typed_projection(slot: &WalletStatusSlot) -> Option<TypedProjectionData> {
    let status = slot.lock().ok()?.clone()?;
    Some(TypedProjectionData {
        key: "wallet".to_string(),
        schema_id: WALLET_STATUS_SCHEMA_ID.to_string(),
        schema_version: WALLET_STATUS_SCHEMA_VERSION,
        file_identifier: String::from_utf8_lossy(WALLET_STATUS_FILE_IDENTIFIER).into_owned(),
        payload: encode_wallet_status(&status),
    })
}

#[cfg(test)]
#[path = "register_tests.rs"]
mod tests;
