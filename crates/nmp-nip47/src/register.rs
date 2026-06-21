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
//! The function depends only on the narrow substrate registrar traits it uses
//! (`ActionRegistrar + RelayTextInterceptorRegistrar + SnapshotProjectionRegistrar`),
//! not the full `AppHost` — it names no `NmpApp` and no FFI type, so it is
//! layer-clean (Layer-4 NIP crate wiring against the Layer-3 `nmp-core` host traits). The one host-specific
//! input — the durable storage path — is passed in by the caller (the builder
//! reads it from the un-started app and hands it here).

use std::sync::Arc;

use nmp_core::substrate::{
    ActionRegistrar, RelayTextInterceptor, RelayTextInterceptorRegistrar,
    SnapshotProjectionRegistrar,
};
use nmp_core::{Kernel, OutboundMessage, TypedProjectionData};

use crate::runtime::{new_wallet_runtime_handle, WalletRuntimeHandle};
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
        // ADR-0052 §D5: the runtime helpers name only the narrow
        // `WalletKernelAccess` capability. The interceptor holds a real
        // `&mut Kernel`, so wrap it through `as_wallet_access` — the same
        // surface the `Protocol` dispatch arm installs on the command context.
        dispatch_nwc_relay_text(&self.runtime, &kernel.as_wallet_access(), relay_url, text)
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
                rt.sync_connection_state(&kernel.as_wallet_access());
            }
        }

        // Build and enqueue the get_info probe if needed.
        if heartbeat.needs_probe {
            let Ok(mut guard) = self.runtime.lock() else {
                return outbound;
            };
            if let Some(rt) = guard.as_mut() {
                if let Some(msg) = rt.build_get_info_probe(&kernel.as_wallet_access()) {
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
///    action modules — ADR-0052 rung 5.2: each module VALUE owns a clone of
///    the per-app [`WalletRuntimeHandle`], so dispatch reaches THIS app's
///    runtime with no process-global;
/// 2. constructs the [`WalletRuntime`] behind a shared handle, installing the
///    durable [`FsPaymentStore`] when `storage_path` is `Some` (the
///    double-pay-safe write-before-enqueue path);
/// 3. installs the relay-text interceptor that drives inbound kind:23195
///    decoding + the V-79 heartbeat/TTL sweeps;
/// 4. registers the generic + typed `"wallet"` snapshot projections.
///
/// Returns the per-app [`WalletRuntimeHandle`] so the caller can wrap it in a
/// `PaymentPort` ([`crate::wallet_payment_port`]) and inject it into the NIP-57
/// zap auto-chain (`nmp_nip57::register_zap_with_payment_port`) — the zap
/// override lives at the caller because `nmp-nip47` must not depend on
/// `nmp-nip57` (layer/D0). Two `NmpApp` instances therefore drive fully
/// independent wallet runtimes (no `ACTIVE_WALLET_RUNTIME` global — deleted).
///
/// MUST run during the config phase, before the kernel starts (the actor reads
/// the interceptor + action registry once, at kernel construction). The
/// `NmpAppBuilder::with_wallet` step in `nmp-defaults` enforces this ordering
/// at compile time for Rust callers.
pub fn register_wallet(
    app: &mut (impl ActionRegistrar + RelayTextInterceptorRegistrar + SnapshotProjectionRegistrar),
    storage_path: Option<String>,
) -> WalletRuntimeHandle {
    // 1. Shared status slot — one `Arc` clone goes to the runtime (sole
    //    writer, D4), the other is captured below by the typed snapshot
    //    projection closure.
    let status_slot: WalletStatusSlot = new_wallet_status_slot();
    let typed_projection_slot = Arc::clone(&status_slot);

    // 2. Wallet runtime — held inside an `Arc<Mutex<Option<WalletRuntime>>>`
    //    handle the action modules, the `ProtocolCommand` impls, and the
    //    interceptor all clone and lock.
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

    // 3. Action modules — exposed under `nmp.wallet.{connect,disconnect,
    //    pay_invoice}`. ADR-0052 rung 5.2: each module VALUE owns a clone of
    //    the per-app handle (no process-global install).
    app.register_action(WalletConnectModule::new(Arc::clone(&handle)));
    app.register_action(WalletDisconnectModule::new(Arc::clone(&handle)));
    app.register_action(WalletPayInvoiceModule::new(Arc::clone(&handle)));

    // 4. Substrate-generic relay-text interceptor — the actor calls this for
    //    every inbound text frame.
    app.add_relay_text_interceptor(Arc::new(WalletInterceptor {
        runtime: Arc::clone(&handle),
    }));

    // 5/6. The typed `"wallet"` sidecar (ADR-0037) — emitted ALONGSIDE the
    //    generic `Value` projection above, never replacing it.
    app.register_typed_snapshot_projection("wallet", move || {
        wallet_typed_projection(&typed_projection_slot)
    });

    // Hand the per-app handle back so the caller can thread it into the NIP-57
    // zap auto-chain (ADR-0052 rung 5.2; `nmp-nip47` must not depend on
    // `nmp-nip57`, so the zap override lives at the caller).
    handle
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
        ..Default::default()
    })
}

#[cfg(test)]
#[path = "register_tests.rs"]
mod tests;
