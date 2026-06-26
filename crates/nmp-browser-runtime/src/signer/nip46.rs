//! NIP-46 signer-broker wiring for the native browser runtime (#2068).
//!
//! # Platform split
//!
//! **wasm32 (real browser target)** — NIP-46 handshake and RPC are HOST-DRIVEN.
//! `nmp-signer-broker` depends on `nmp-network/native` (OS threads, sync I/O)
//! and CANNOT compile for wasm32. On wasm32, `dispatch_by_backend` in
//! `signer/completion.rs` returns `false` for `Nip46` backends, causing the
//! runtime to emit `BrowserRuntimeEvent::SignRequest` so the JS host can broker
//! the sign externally and deliver the result via
//! `BrowserRuntimeHandle::deliver_signer_response`. No new wasm code is needed
//! for the host-brokered signing path (#2068).
//!
//! **native** (this module) — `BunkerBroker` drives the nostrconnect handshake
//! and NIP-46 RPC on dedicated OS threads. Dynamic provider registration flows
//! through a bounded channel that `pump()` drains under D4 single-writer.
//! Steady-state RPC completions flow through `signer.ingest_rpc_response()`
//! (called by the `CompletionSink`) which resolves the `SignerOp::Pending`
//! channel created by `dispatch_nip46` → `signer.sign()`. A per-sign thread
//! forwards the resolved `SignedEvent` as a `SignerCompletion` to the runtime's
//! completion channel.
//!
//! # D4 (single-writer)
//!
//! Neither the `BrokerEventHandler` nor the `CompletionSink` touches the
//! `KernelReducer` or `CapabilityProviderRegistry` directly. Both write to
//! channels (`ProviderRegistrationTx`, `SignerCompletionTx`) that `pump()`
//! drains under the sole `&mut KernelReducer` borrow.
//!
//! # D8 (no blocking)
//!
//! `pump()` is never blocked. The per-sign thread owns the blocking `rx.recv()`;
//! `BrokerEventHandler` and `CompletionSink` are non-blocking channel writes.
//!
//! # WakeCell note
//!
//! `WakeCell` is `Rc<RefCell<...>>` (not `Send`) and cannot be fired from the
//! broker's OS threads. On native, the host calls `pump()` directly; no
//! JS-style timer-wake is required. The existing `SignerCompletionTx` (the
//! `mpsc::Sender`) IS `Send` and is used by the per-sign thread to deliver
//! completions, which `pump()` picks up on its next call.

use std::sync::{mpsc, Arc, Mutex};

use nmp_signer_broker::{BrokerEvent, BrokerEventHandler, BunkerBroker, CompletionSink};
use nmp_signers::{Nip46Signer, Signer};
use nmp_signer_iface::{SignerOp, UnsignedEvent};

use crate::relay::WakeCell;
use crate::signer::{SignerCompletion, SignerCompletionTx};

/// A dynamically-registered NIP-46 signer arriving via `BrokerEvent::SignerReady`.
///
/// Enqueued into a bounded channel by the `BrokerEventHandler` (off-thread)
/// and applied to the `CapabilityProviderRegistry` by `pump()` (D4 single-writer).
pub(crate) struct ProviderRegistration {
    /// The fully-connected `Nip46Signer`, cast to `dyn Signer` for insertion
    /// into `CapabilityProviderRegistry` alongside NIP-07 and LocalKey signers.
    pub(crate) signer: Arc<dyn Signer>,
}

/// Sender end of the provider-registration channel (off-thread writes).
pub(crate) type ProviderRegistrationTx = mpsc::SyncSender<ProviderRegistration>;

/// Receiver end of the provider-registration channel (drained by `pump()`).
pub(crate) type ProviderRegistrationRx = mpsc::Receiver<ProviderRegistration>;

/// Bounded capacity for the provider-registration channel.
///
/// MVP supports a single NIP-46 session at a time. A capacity of 8 is generous
/// and prevents unbounded growth if the broker emits multiple `SignerReady`
/// events (e.g. rapid reconnect). `try_send` on a full channel drops the send
/// rather than panicking (D6 — never a silent panic).
pub(crate) const PROVIDER_REG_CHANNEL_CAP: usize = 8;

/// Create the bounded provider-registration channel.
///
/// The `SyncSender` is `Clone` + `Send`: the broker event handler (OS thread)
/// writes to it, `pump()` drains it. The original sender is kept in
/// `BrowserRuntimeHandle` for test access; the broker gets a clone.
pub(crate) fn provider_registration_channel() -> (ProviderRegistrationTx, ProviderRegistrationRx) {
    mpsc::sync_channel(PROVIDER_REG_CHANNEL_CAP)
}

/// Shared concrete `Nip46Signer` slot: populated by the `BrokerEventHandler`
/// when `SignerReady` arrives; read by the `CompletionSink` to route inbound
/// RPC responses to `ingest_rpc_response`.
///
/// `Arc<Nip46Signer>` (not `dyn Signer`) because `ingest_rpc_response` is a
/// concrete method absent from the abstract `Signer` trait.
type SignerSlot = Arc<Mutex<Option<Arc<Nip46Signer>>>>;

/// Wire a `BunkerBroker` for use in the browser runtime.
///
/// Returns:
/// - The broker — call `broker.start_handshake(uri)` to begin a NIP-46 session
///   and `broker.cancel()` to end it.
/// - The `CompletionSink` — install via `broker.set_completion_sink(sink)` so
///   the broker's transport routes decrypted RPC responses through
///   `ingest_rpc_response` (resolving the `SignerOp::Pending` from `dispatch_nip46`).
///
/// ## Event handler wiring
///
/// - `SignerReady { signer }` → stores the concrete `Nip46Signer` Arc in the
///   shared `SignerSlot` (for `CompletionSink` access); enqueues a
///   `ProviderRegistration` for `pump()` to apply (D4: off-thread enqueue,
///   pump-thread apply). The WakeCell cannot be fired here (Rc not Send) —
///   `pump()` picks up the registration on its next call.
/// - `Progress` / `ConnectionStateChanged` / `RelayIntakeDropped` → informational;
///   future `BrowserRuntimeEvent` forwarding is a follow-up to #2068.
///
/// ## CompletionSink wiring
///
/// Calls `signer.ingest_rpc_response(decrypted_body_json)`, resolving the
/// `Receiver` inside the `SignerOp::Pending` that `dispatch_nip46` → `signer.sign()`
/// produced. The mapper thread inside `nmp_signers` then converts the raw RPC
/// result to a `SignedEvent` and sends it to the per-sign thread's `Receiver`.
/// That thread pushes a `SignerCompletion` to the completion channel for `pump()`.
pub(crate) fn make_nip46_broker(
    provider_reg_tx: ProviderRegistrationTx,
) -> (Arc<BunkerBroker>, CompletionSink) {
    let signer_slot: SignerSlot = Arc::new(Mutex::new(None));
    let signer_slot_for_handler = Arc::clone(&signer_slot);
    let signer_slot_for_sink = Arc::clone(&signer_slot);

    let handler: Arc<BrokerEventHandler> = Arc::new(move |event: BrokerEvent| {
        if let BrokerEvent::SignerReady { signer } = event {
            // Populate the shared slot so the CompletionSink can route RPC
            // responses to the correct signer via ingest_rpc_response.
            if let Ok(mut slot) = signer_slot_for_handler.lock() {
                *slot = Some(Arc::clone(&signer));
            }
            // Enqueue a provider registration for pump() to apply (D4: the
            // CapabilityProviderRegistry is only mutated inside pump()).
            // try_send: drops silently if full (D6, never panics).
            let signer_as_trait: Arc<dyn Signer> = signer;
            let _ = provider_reg_tx.try_send(ProviderRegistration {
                signer: signer_as_trait,
            });
        }
        // Progress / ConnectionStateChanged / RelayIntakeDropped: informational;
        // forwarding to BrowserRuntimeEvent is a follow-up to this PR (#2068).
    });

    let broker = BunkerBroker::new(handler);

    // CompletionSink: called by BrokerTransport::dispatch_inbound with the
    // DECRYPTED NIP-46 RPC response body ({"id":...,"result":...}). Delegates
    // to ingest_rpc_response, which resolves the inner mpsc channel that
    // Nip46Signer::sign() created — eventually waking the per-sign thread in
    // dispatch_nip46 which pushes the SignerCompletion to the runtime channel.
    let completion_sink: CompletionSink = Arc::new(move |response_json: String| {
        if let Ok(slot) = signer_slot_for_sink.lock() {
            if let Some(signer) = slot.as_ref() {
                signer.ingest_rpc_response(&response_json);
            }
        }
    });

    (broker, completion_sink)
}

/// Dispatch a NIP-46 sign request on native (D4-safe, D8-no-block).
///
/// Calls `signer.sign(unsigned)` which sends a NIP-46 RPC through the
/// `BrokerTransport` and returns `SignerOp::Pending(rx)`. A named background
/// thread waits on `rx`; when the RPC round-trip settles (via
/// `CompletionSink` → `ingest_rpc_response` → mapper thread → `rx`), the
/// background thread pushes a `SignerCompletion` to the runtime's completion
/// channel for `pump()` to apply.
///
/// Returns `true` when the sign was dispatched (completion will arrive on `tx`
/// when the RPC settles). `SignerOp::Ready` results are sent inline (unusual
/// for NIP-46 but handled). Thread spawn failure sends an error completion
/// immediately so the parked publish fails fast (D6).
///
/// # Platform
///
/// This function is native-only. On wasm32, `signer/completion.rs`
/// `dispatch_by_backend` returns `false` for the `Nip46` arm, causing the
/// runtime to emit `BrowserRuntimeEvent::SignRequest` for host-brokering.
pub(crate) fn dispatch_nip46(
    signer: &dyn Signer,
    correlation_id: &str,
    unsigned: UnsignedEvent,
    tx: &SignerCompletionTx,
    _wake: &WakeCell,
    // _wake: WakeCell is Rc<RefCell<...>> (not Send) — cannot be moved to the
    // OS thread. Native host calls pump() directly; no JS-style wake needed.
) -> bool {
    match signer.sign(unsigned) {
        SignerOp::Ready(Ok(signed)) => {
            // Immediate result: unusual for NIP-46 but handle defensively.
            let _ = tx.send(SignerCompletion {
                correlation_id: correlation_id.to_string(),
                result: Ok(signed.to_nip01_json()),
            });
            true
        }
        SignerOp::Ready(Err(e)) => {
            let _ = tx.send(SignerCompletion {
                correlation_id: correlation_id.to_string(),
                result: Err(format!("nip46 sign error: {e}")),
            });
            true
        }
        SignerOp::Pending(rx) => {
            let corr = correlation_id.to_string();
            let tx_for_thread = tx.clone();
            let corr_for_spawn_err = corr.clone();
            let tx_for_spawn_err = tx.clone();
            match std::thread::Builder::new()
                .name("nmp-browser-nip46-sign".to_string())
                .spawn(move || {
                    let result = match rx.recv() {
                        Ok(Ok(signed)) => Ok(signed.to_nip01_json()),
                        Ok(Err(e)) => Err(format!("nip46 sign error: {e}")),
                        Err(_) => Err(
                            "nip46 signer channel disconnected (session ended or cancelled)"
                                .to_string(),
                        ),
                    };
                    // Non-blocking: mpsc::Sender::send returns immediately.
                    // pump() picks up the completion on its next call.
                    let _ = tx_for_thread.send(SignerCompletion {
                        correlation_id: corr,
                        result,
                    });
                }) {
                Ok(_join_handle) => true,
                Err(e) => {
                    // OS thread spawn failure: fail the round-trip immediately
                    // so the parked publish doesn't hang forever (D6 — never
                    // a silent drop).
                    let _ = tx_for_spawn_err.send(SignerCompletion {
                        correlation_id: corr_for_spawn_err,
                        result: Err(format!("nip46: thread spawn failed: {e}")),
                    });
                    true
                }
            }
        }
    }
}
