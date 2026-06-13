//! Host-side glue for the V-38 NIP-47 wallet stack.
//!
//! `nmp-nip47` owns the runtime + action modules + status type. This module
//! is the Chirp-specific composition root: builds the [`WalletStatusSlot`],
//! the [`WalletRuntime`], registers the three action modules, installs the
//! relay-text interceptor + the runtime handle, and registers the
//! `"wallet"` snapshot projection.

use std::sync::Arc;

use nmp_core::substrate::RelayTextInterceptor;
use nmp_core::{Kernel, OutboundMessage, TypedProjectionData};
use nmp_ffi::NmpApp;

use nmp_nip47::{
    encode_wallet_status, new_wallet_runtime_handle, WalletConnectModule, WalletDisconnectModule,
    WalletPayInvoiceModule, WalletRuntime, WalletRuntimeHandle, WalletStatusSlot,
    WALLET_STATUS_FILE_IDENTIFIER, WALLET_STATUS_SCHEMA_ID, WALLET_STATUS_SCHEMA_VERSION,
};

/// Adapter that wires the wallet runtime's [`nmp_nip47::handle_nwc_text`]
/// (via [`nmp_nip47::dispatch_nwc_relay_text`]) into the substrate-generic
/// [`RelayTextInterceptor`] trait the actor calls.
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
        nmp_nip47::dispatch_nwc_relay_text(&self.runtime, kernel, relay_url, text)
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
            let _swept = rt.sweep_expired_payments(now_secs, nmp_nip47::PENDING_PAYMENT_TTL_SECS);

            // V-79: heartbeat tick — pure wall-clock gated, Kernel-free.
            let heartbeat = rt.tick_heartbeat(
                now_secs,
                nmp_nip47::HEARTBEAT_CADENCE_SECS,
                nmp_nip47::HEARTBEAT_MAX_FAILURES,
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

/// Register the NIP-47 wallet stack on `app`. Called by
/// `nmp_app_chirp_register` when the `wallet` feature is on.
pub(crate) fn register_nip47_wallet(app: &mut NmpApp) {
    // 1. Shared status slot — one `Arc` clone goes to the runtime (sole
    //    writer, D4), the others are captured below by the `"wallet"`
    //    generic + typed snapshot projection closures.
    let status_slot: WalletStatusSlot = nmp_nip47::new_wallet_status_slot();
    let projection_slot = Arc::clone(&status_slot);
    let typed_projection_slot = Arc::clone(&status_slot);

    // 2. Wallet runtime — held inside an `Arc<Mutex<Option<WalletRuntime>>>`
    //    handle the `ProtocolCommand` impls and the interceptor both lock.
    let mut runtime = WalletRuntime::new(status_slot);

    // Install the durable payment store when a persistent storage path is
    // configured (it is set before `nmp_app_start` on every real shell). This
    // activates the double-pay-safe write-before-enqueue + tri-state
    // reconciliation path: in-flight payments survive a process kill and a
    // TTL/disconnect transitions them to `Unknown` (never `Failed`) so a
    // `lookup_invoice` on reconnect settles them to the true outcome.
    if let Some(storage_path) = app.storage_path_for_start() {
        runtime.set_payment_store(nmp_nip47::FsPaymentStore::new(storage_path));
    }

    // 3. The ONE per-app wallet runtime handle (ADR-0052 D1/D2). Created BEFORE
    //    registration so every consumer captures the SAME `Arc` by value — no
    //    process-global. Two `NmpApp`s in one process therefore own two
    //    independent wallet runtimes (the K2 rung 5.2 no-crosstalk invariant).
    let handle: WalletRuntimeHandle = new_wallet_runtime_handle();
    if let Ok(mut guard) = handle.lock() {
        *guard = Some(runtime);
    }

    // 4. Action modules — exposed under `nmp.wallet.{connect,disconnect,
    //    pay_invoice}` so the existing `nmp_app_wallet_*` FFI shims (which
    //    route through `dispatch_action` post-V-38) reach the runtime. Each
    //    module carries its OWN clone of the handle by value.
    app.register_action(WalletConnectModule::new(Arc::clone(&handle)));
    app.register_action(WalletDisconnectModule::new(Arc::clone(&handle)));
    app.register_action(WalletPayInvoiceModule::new(Arc::clone(&handle)));

    // 4b. Re-register the NIP-57 zap module so its LNURL → pay-invoice chain
    //     pays through THIS app's wallet handle (ADR-0052 D2). `nmp-defaults`
    //     registered `ZapAction::default()` (wallet-less) as a yielding default
    //     during `register_defaults`; this app-path registration overrides it
    //     with the NWC-backed payer. Without a wallet composed, the wallet-less
    //     default stands and a zap fails closed with "no wallet connected".
    app.register_action(nmp_nip57::ZapAction::new(nmp_nip57::ZapPayer::Nwc(
        Arc::clone(&handle),
    )));

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

    // 7. The typed `"wallet"` sidecar (Wave A of the typed-snapshot migration,
    //    ADR-0037) — emitted ALONGSIDE the generic `Value` projection above,
    //    never replacing it. A host with an `NWST` decoder prefers this typed
    //    payload; an un-updated host falls back to the generic subtree.
    //    Additive — un-updated hosts are unaffected.
    app.register_typed_snapshot_projection("wallet", move || {
        wallet_typed_projection(&typed_projection_slot)
    });
}

/// Build the typed `"wallet"` sidecar entry from the shared status slot, or
/// `None` when no wallet is connected this session (the slot holds `None`).
///
/// Extracted from the `register_typed_snapshot_projection` closure so the
/// registration's schema identity (`key` / `schema_id` / `file_identifier`) and
/// the encode are unit-testable without spinning the actor (Wave A proof test).
pub(crate) fn wallet_typed_projection(slot: &WalletStatusSlot) -> Option<TypedProjectionData> {
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
#[path = "wallet_runtime_tests.rs"]
mod tests;
