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
//! - **NIP-07 off-wasm**, NIP-46, NIP-55, Custom: unresolvable → `false`.
//!   NIP-46 is #2068 (follow-up PR).

use std::sync::mpsc;

use nmp_signer_iface::{SignerOp, UnsignedEvent};
use nmp_signers::{Signer, SignerBackend};

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
        entry.signer.as_ref(),
        entry.signer.backend(),
        correlation_id,
        unsigned,
        tx,
        wake,
    )
}

fn dispatch_by_backend(
    signer: &dyn Signer,
    backend: SignerBackend,
    correlation_id: &str,
    unsigned: UnsignedEvent,
    tx: &SignerCompletionTx,
    wake: &WakeCell,
) -> bool {
    match backend {
        SignerBackend::LocalKey => {
            dispatch_local_key(signer, correlation_id, unsigned, tx);
            true
        }
        SignerBackend::Nip07 => dispatch_nip07(signer, correlation_id, unsigned, tx, wake),
        // NIP-46 (#2068 follow-up), NIP-55, Custom: not wired in this track.
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
mod tests {
    use std::sync::{mpsc, Arc};

    use nmp_signers::LocalKeySigner;
    use nmp_signers::Signer;

    use super::*;
    use crate::signer::registry::CapabilityProviderRegistry;

    /// A no-op wake cell for broker tests that don't assert on wake firing.
    fn noop_wake() -> WakeCell {
        use std::cell::RefCell;
        use std::rc::Rc;
        Rc::new(RefCell::new(Rc::new(|| {}) as Rc<dyn Fn()>))
    }

    fn make_registry_with_local_key(secret_hex: &str) -> (CapabilityProviderRegistry, String) {
        let signer = LocalKeySigner::from_secret_hex(secret_hex).expect("valid secret");
        let pubkey_hex = signer.pubkey().to_hex();
        let mut reg = CapabilityProviderRegistry::new();
        reg.insert(Arc::new(signer) as Arc<dyn Signer>);
        (reg, pubkey_hex)
    }

    /// A minimal unsigned event JSON in the flat wire shape.
    fn unsigned_json(pubkey: &str) -> String {
        serde_json::json!({
            "pubkey": pubkey,
            "kind": 1,
            "tags": [],
            "content": "test",
            "created_at": 1_700_000_000u64,
        })
        .to_string()
    }

    #[test]
    fn local_key_broker_sends_completion() {
        let secret = "bb".repeat(32);
        let (reg, pubkey_hex) = make_registry_with_local_key(&secret);
        let (tx, rx) = mpsc::channel::<SignerCompletion>();
        let ujson = unsigned_json(&pubkey_hex);

        let brokered = broker_sign_request(&reg, "corr-1", &pubkey_hex, &ujson, &tx, &noop_wake());

        assert!(brokered, "LocalKey should be brokered");
        let completion = rx.try_recv().expect("completion must arrive synchronously");
        assert_eq!(completion.correlation_id, "corr-1");
        assert!(
            completion.result.is_ok(),
            "LocalKey sign must succeed: {:?}",
            completion.result
        );
    }

    #[test]
    fn unknown_pubkey_returns_false() {
        let (reg, _) = make_registry_with_local_key(&"cc".repeat(32));
        let (tx, _rx) = mpsc::channel::<SignerCompletion>();

        let brokered = broker_sign_request(&reg, "corr-2", "deadbeef", "{}", &tx, &noop_wake());
        assert!(!brokered, "unknown pubkey must not be brokered");
    }

    #[test]
    fn enqueue_completion_fires_wake_and_queues() {
        use std::cell::{Cell, RefCell};
        use std::rc::Rc;

        // Build a wake cell with a counting closure (what set_wake installs).
        let count = Rc::new(Cell::new(0u32));
        let count_clone = Rc::clone(&count);
        let wake: WakeCell = Rc::new(RefCell::new(Rc::new(move || {
            count_clone.set(count_clone.get() + 1);
        }) as Rc<dyn Fn()>));

        let (tx, rx) = mpsc::channel::<SignerCompletion>();
        enqueue_completion(
            &tx,
            &wake,
            SignerCompletion {
                correlation_id: "corr-wake".to_string(),
                result: Ok("{}".to_string()),
            },
        );

        assert_eq!(count.get(), 1, "enqueue_completion must fire the wake once");
        let completion = rx.try_recv().expect("completion must be queued");
        assert_eq!(completion.correlation_id, "corr-wake");
    }

    #[test]
    fn malformed_unsigned_json_sends_error_completion() {
        let secret = "dd".repeat(32);
        let (reg, pubkey_hex) = make_registry_with_local_key(&secret);
        let (tx, rx) = mpsc::channel::<SignerCompletion>();

        let brokered = broker_sign_request(
            &reg,
            "corr-3",
            &pubkey_hex,
            "not-valid-json",
            &tx,
            &noop_wake(),
        );
        assert!(
            brokered,
            "malformed json still triggers broker (error path)"
        );
        let completion = rx.try_recv().expect("error completion must arrive");
        assert!(
            completion.result.is_err(),
            "malformed JSON must produce error completion"
        );
    }
}
