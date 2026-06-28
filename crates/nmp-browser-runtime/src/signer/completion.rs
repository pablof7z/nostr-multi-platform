//! Sign-round-trip completion channel and `broker_sign_request` helper (#2049).
//!
//! `SignerCompletion` is the typed result pushed through an `mpsc` channel when
//! the registry-brokered sign settles. The pump loop drains this channel on every
//! turn and hands each result to the kernel via `deliver_signed_response_at`.
//!
//! # D4 single-writer
//!
//! The async NIP-07 driver (`spawn_local`) writes to `SignerCompletionTx`.
//! The pump drains `SignerCompletionRx` — from inside `pump()`, the sole
//! `&mut KernelReducer` borrow point. The kernel is **never** touched from
//! inside `spawn_local`. D4 is preserved.
//!
//! # Broker dispatch
//!
//! `broker_sign_request` returns `true` when it found and dispatched a
//! provider, `false` when no resolvable provider exists. The caller emits
//! `BrowserRuntimeEvent::SignRequest` only when `false` (host-brokered path).
//!
//! - **LocalKey** (`SignerOp::Ready`): sign is synchronous — completion sent
//!   on `tx` immediately. No `SignRequest` event emitted.
//! - **NIP-07** on `wasm32 + feature="wasm"`: `sign_event_via_extension` via
//!   `spawn_local`; completion arrives on `tx` when the JS Promise resolves.
//!   No `SignRequest` event emitted.
//! - **NIP-46**: the browser-owned `Nip46Signer` queues the RPC and parks the
//!   returned `SignerOp` in `PendingSignerCompletions`. Relay responses are
//!   delivered to the signer by the NIP-46 bridge; the next pump drains the
//!   ready op and completes the kernel sign round-trip.
//! - **NIP-07 off-wasm**, NIP-55, Custom: unresolvable → `false`.

use std::collections::HashMap;
use std::sync::mpsc;

use nmp_signer_iface::{SignerError, SignerOp, UnsignedEvent};
use nmp_signers::{Nip46Signer, PublicKey, Signer, SignerBackend};

use super::registry::CapabilityProviderRegistry;
use crate::relay::{fire_wake, WakeCell};

/// One settled sign round-trip from the broker.
#[derive(Debug)]
pub(crate) struct SignerCompletion {
    /// Sign round-trip correlation id this settles (matches the parked entry).
    pub(crate) correlation_id: String,
    /// `Ok(flat-NIP-01 signed JSON)` on success; `Err(reason)` on any failure.
    pub(crate) result: Result<String, String>,
}

/// Sender end of the signer-completion channel.
pub(crate) type SignerCompletionTx = mpsc::Sender<SignerCompletion>;
/// Receiver end of the signer-completion channel.
pub(crate) type SignerCompletionRx = mpsc::Receiver<SignerCompletion>;

enum PendingSignerCompletion {
    Nip46Sign {
        op: SignerOp<String>,
        expected_pubkey: PublicKey,
    },
}

impl PendingSignerCompletion {
    fn poll(&mut self) -> Option<Result<String, String>> {
        match self {
            Self::Nip46Sign {
                op,
                expected_pubkey,
            } => match op.poll() {
                Some(Ok(response_json)) => Some(
                    Nip46Signer::parse_sign_event_response(&response_json, *expected_pubkey)
                        .map(|signed| signed.to_nip01_json())
                        .map_err(format_signer_error),
                ),
                Some(Err(error)) => Some(Err(format!("nip46 sign error: {error}"))),
                None => None,
            },
        }
    }
}

/// Pending provider-backed sign operations that resolve from relay/capability
/// re-entry rather than from a host `deliver_signer_response` call.
#[derive(Default)]
pub(crate) struct PendingSignerCompletions {
    pending: HashMap<String, PendingSignerCompletion>,
}

impl PendingSignerCompletions {
    /// Construct an empty pending-op table.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    fn insert_nip46(
        &mut self,
        correlation_id: String,
        op: SignerOp<String>,
        expected_pubkey: PublicKey,
    ) {
        self.pending.insert(
            correlation_id,
            PendingSignerCompletion::Nip46Sign {
                op,
                expected_pubkey,
            },
        );
    }

    /// Poll pending signer operations once and return every settled completion.
    ///
    /// D8: this performs one non-blocking `SignerOp::poll()` per parked op when
    /// `pump()` has already been scheduled by relay/capability re-entry.
    pub(crate) fn drain_ready(&mut self) -> Vec<SignerCompletion> {
        let keys: Vec<String> = self.pending.keys().cloned().collect();
        let mut ready = Vec::new();
        for correlation_id in keys {
            let Some(result) = self
                .pending
                .get_mut(&correlation_id)
                .and_then(PendingSignerCompletion::poll)
            else {
                continue;
            };
            self.pending.remove(&correlation_id);
            ready.push(SignerCompletion {
                correlation_id,
                result,
            });
        }
        ready
    }
}

/// Enqueue a settled completion and fire the wake so a pump is scheduled.
///
/// Used by the paths that enqueue **outside** `pump()` — the async NIP-07
/// driver (`spawn_local`) and the host-brokered
/// `BrowserRuntimeHandle::deliver_signer_response`. Firing the wake (the SAME
/// indirection relay inbound uses) is what guarantees the queued completion is
/// drained on a subsequent pump instead of sitting forever (D8: no polling;
/// D4: the reducer is NOT touched here — only the channel + wake).
///
/// The synchronous LocalKey path does NOT use this: its completion is sent
/// during `drain_inbox` and drained in the same pump turn (step 1.5), so no
/// wake is needed.
pub(crate) fn enqueue_completion(
    tx: &SignerCompletionTx,
    wake: &WakeCell,
    completion: SignerCompletion,
) {
    let _ = tx.send(completion);
    fire_wake(wake);
}

/// Parse a flat-NIP-01 or nested `UnsignedEvent` JSON into an [`UnsignedEvent`].
///
/// Total (D6): returns `Err(reason)` on any shape mismatch — never panics.
fn parse_unsigned_json(unsigned_json: &str) -> Result<UnsignedEvent, String> {
    // Try the nested `UnsignedEvent` derive shape first (produced by
    // `serde_json::to_string(&unsigned_event)`).
    if let Ok(u) = serde_json::from_str::<UnsignedEvent>(unsigned_json) {
        return Ok(u);
    }
    // Fall back to the flat wire shape (`{pubkey, kind, tags, content, created_at}`).
    #[derive(serde::Deserialize)]
    struct Flat {
        pubkey: String,
        kind: u32,
        tags: Vec<Vec<String>>,
        content: String,
        created_at: u64,
    }
    let flat: Flat = serde_json::from_str(unsigned_json)
        .map_err(|e| format!("unsigned event JSON did not parse: {e}"))?;
    Ok(UnsignedEvent {
        pubkey: flat.pubkey,
        kind: flat.kind,
        tags: flat.tags,
        content: flat.content,
        created_at: flat.created_at,
    })
}

/// Try to broker a sign request using a registered capability provider.
///
/// Returns `true` when the sign was dispatched (the completion will arrive on
/// `tx`). Returns `false` when no resolvable provider is registered for
/// `account_pubkey` — the caller MUST emit `BrowserRuntimeEvent::SignRequest`
/// so the host can broker externally (never a silent drop, D6).
///
/// See module-level doc for backend dispatch rules.
pub(crate) fn broker_sign_request(
    registry: &CapabilityProviderRegistry,
    pending: &mut PendingSignerCompletions,
    correlation_id: &str,
    account_pubkey: &str,
    unsigned_json: &str,
    tx: &SignerCompletionTx,
    wake: &WakeCell,
) -> bool {
    let Some(entry) = registry.resolve(account_pubkey) else {
        return false;
    };

    let unsigned = match parse_unsigned_json(unsigned_json) {
        Ok(u) => u,
        Err(e) => {
            // Parse failure is terminal; fail the round-trip immediately.
            // Sent during `drain_inbox` (inside pump) — drained the same turn,
            // so no wake is needed here.
            let _ = tx.send(SignerCompletion {
                correlation_id: correlation_id.to_string(),
                result: Err(format!("broker: unsigned-event parse error: {e}")),
            });
            return true;
        }
    };

    dispatch_by_backend(
        BackendDispatch {
            signer: entry.signer.as_ref(),
            nip46_signer: entry.nip46_signer.as_deref(),
            backend: entry.signer.backend(),
            pending,
            tx,
            wake,
        },
        correlation_id,
        unsigned,
    )
}

struct BackendDispatch<'a> {
    signer: &'a dyn Signer,
    nip46_signer: Option<&'a Nip46Signer>,
    backend: SignerBackend,
    pending: &'a mut PendingSignerCompletions,
    tx: &'a SignerCompletionTx,
    wake: &'a WakeCell,
}

fn dispatch_by_backend(
    dispatch: BackendDispatch<'_>,
    correlation_id: &str,
    unsigned: UnsignedEvent,
) -> bool {
    match dispatch.backend {
        SignerBackend::LocalKey => {
            dispatch_local_key(dispatch.signer, correlation_id, unsigned, dispatch.tx);
            true
        }
        SignerBackend::Nip07 => dispatch_nip07(
            dispatch.signer,
            correlation_id,
            unsigned,
            dispatch.tx,
            dispatch.wake,
        ),
        SignerBackend::Nip46 => {
            let Some(nip46_signer) = dispatch.nip46_signer else {
                return false;
            };
            dispatch_nip46(
                nip46_signer,
                dispatch.pending,
                correlation_id,
                unsigned,
                dispatch.tx,
            );
            true
        }
        // NIP-55 and Custom providers are not wired in the browser runtime.
        _ => false,
    }
}

/// Synchronous LocalKey path: `SignerOp::Ready` — sign inline, send immediately.
fn dispatch_local_key(
    signer: &dyn Signer,
    correlation_id: &str,
    unsigned: UnsignedEvent,
    tx: &SignerCompletionTx,
) {
    let result = match signer.sign(unsigned) {
        SignerOp::Ready(Ok(signed)) => Ok(signed.to_nip01_json()),
        SignerOp::Ready(Err(e)) => Err(format!("local-key sign error: {e}")),
        // `LocalKeySigner::sign` always returns `Ready`; guard against
        // a misbehaving custom implementation (D6 — never panic across seam).
        SignerOp::Pending(_) => Err("local-key signer returned Pending unexpectedly".to_string()),
    };
    let _ = tx.send(SignerCompletion {
        correlation_id: correlation_id.to_string(),
        result,
    });
}

fn format_signer_error(error: SignerError) -> String {
    error.to_string()
}

fn dispatch_nip46(
    signer: &Nip46Signer,
    pending: &mut PendingSignerCompletions,
    correlation_id: &str,
    unsigned: UnsignedEvent,
    tx: &SignerCompletionTx,
) {
    let expected_pubkey = signer.pubkey();
    let mut op = signer.sign_event_response_json(&unsigned);
    match op.poll() {
        Some(Ok(response_json)) => {
            let result = Nip46Signer::parse_sign_event_response(&response_json, expected_pubkey)
                .map(|signed| signed.to_nip01_json())
                .map_err(format_signer_error);
            let _ = tx.send(SignerCompletion {
                correlation_id: correlation_id.to_string(),
                result,
            });
        }
        Some(Err(error)) => {
            let _ = tx.send(SignerCompletion {
                correlation_id: correlation_id.to_string(),
                result: Err(format!("nip46 sign error: {error}")),
            });
        }
        None => {
            pending.insert_nip46(correlation_id.to_string(), op, expected_pubkey);
        }
    }
}

/// NIP-07 async dispatch (wasm32 + `feature = "wasm"` path).
///
/// On the wasm path: drives `nmp_signers::sign_event_via_extension` via
/// `wasm_bindgen_futures::spawn_local`; the `SignerCompletion` is sent when
/// the JS Promise resolves. The kernel is NOT touched inside `spawn_local`
/// (D4 single-writer preserved — only the channel sender is used).
///
/// On native / no-wasm-feature: unresolvable — returns `false` so the caller
/// emits `SignRequest` for host-brokering.
#[cfg(all(target_arch = "wasm32", feature = "wasm"))]
fn dispatch_nip07(
    signer: &dyn Signer,
    correlation_id: &str,
    unsigned: UnsignedEvent,
    tx: &SignerCompletionTx,
    wake: &WakeCell,
) -> bool {
    let pubkey = signer.pubkey();
    let corr = correlation_id.to_string();
    let tx = tx.clone();
    let wake = wake.clone();
    wasm_bindgen_futures::spawn_local(async move {
        let result = nmp_signers::sign_event_via_extension(pubkey, unsigned)
            .await
            .map(|signed| signed.to_nip01_json())
            .map_err(|e| format!("nip07 extension sign error: {e}"));
        // Resolves in a FUTURE JS task — pump() has long returned. Enqueue AND
        // fire the wake so the queued completion is drained next pump instead
        // of sitting forever (D8: no polling; D4: reducer untouched here).
        enqueue_completion(
            &tx,
            &wake,
            SignerCompletion {
                correlation_id: corr,
                result,
            },
        );
    });
    true
}

/// NIP-07 off-wasm stub: unresolvable; host must broker via `SignRequest`.
#[cfg(not(all(target_arch = "wasm32", feature = "wasm")))]
fn dispatch_nip07(
    _signer: &dyn Signer,
    _correlation_id: &str,
    _unsigned: UnsignedEvent,
    _tx: &SignerCompletionTx,
    wake: &WakeCell,
) -> bool {
    // NIP-07 extension signing requires wasm32 + browser context.
    // On native builds the provider is unresolvable; the runtime falls back to
    // emitting `BrowserRuntimeEvent::SignRequest` for host-brokering.
    let _ = wake;
    false
}

#[cfg(test)]
mod tests;
