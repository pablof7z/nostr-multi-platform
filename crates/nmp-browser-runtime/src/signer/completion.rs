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
        entry.signer.as_ref(),
        entry.nip46_signer.as_deref(),
        entry.signer.backend(),
        pending,
        correlation_id,
        unsigned,
        tx,
        wake,
    )
}

fn dispatch_by_backend(
    signer: &dyn Signer,
    nip46_signer: Option<&Nip46Signer>,
    backend: SignerBackend,
    pending: &mut PendingSignerCompletions,
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
        SignerBackend::Nip46 => {
            let Some(nip46_signer) = nip46_signer else {
                return false;
            };
            dispatch_nip46(nip46_signer, pending, correlation_id, unsigned, tx);
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
mod tests {
    use std::sync::{mpsc, Arc};

    use nmp_signer_iface::{Nip46Rpc, Nip46Transport};
    use nmp_signers::{LocalKeySigner, Nip46SignerHandle, Signer};

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
        let mut pending = PendingSignerCompletions::new();

        let brokered = broker_sign_request(
            &reg,
            &mut pending,
            "corr-1",
            &pubkey_hex,
            &ujson,
            &tx,
            &noop_wake(),
        );

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
        let mut pending = PendingSignerCompletions::new();

        let brokered = broker_sign_request(
            &reg,
            &mut pending,
            "corr-2",
            "deadbeef",
            "{}",
            &tx,
            &noop_wake(),
        );
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
        let mut pending = PendingSignerCompletions::new();

        let brokered = broker_sign_request(
            &reg,
            &mut pending,
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

    #[derive(Debug, Default)]
    struct StubTransport {
        sent: std::sync::Mutex<Vec<Nip46Rpc>>,
    }

    impl Nip46Transport for StubTransport {
        fn send_rpc(&self, rpc: Nip46Rpc) -> Result<(), SignerError> {
            self.sent.lock().expect("sent lock").push(rpc);
            Ok(())
        }
    }

    #[derive(Debug)]
    struct FailingTransport;

    impl Nip46Transport for FailingTransport {
        fn send_rpc(&self, _rpc: Nip46Rpc) -> Result<(), SignerError> {
            Err(SignerError::Backend("transport closed".to_string()))
        }
    }

    fn nip46_signer_with_transport<T: Nip46Transport + 'static>(
        remote_user: &LocalKeySigner,
        transport: Arc<T>,
    ) -> nmp_signers::Nip46Signer {
        let uri = format!(
            "bunker://{}?relay=wss://relay.example.com",
            remote_user.pubkey().to_hex()
        );
        let handle = Nip46SignerHandle::from_bunker_uri(&uri).expect("valid bunker uri");
        handle.complete(transport, remote_user.pubkey())
    }

    #[test]
    fn nip46_broker_parks_rpc_and_drains_after_response() {
        let remote_user = LocalKeySigner::generate();
        let pubkey_hex = remote_user.pubkey().to_hex();
        let transport = Arc::new(StubTransport::default());
        let signer = Arc::new(nip46_signer_with_transport(
            &remote_user,
            Arc::clone(&transport),
        ));
        let mut reg = CapabilityProviderRegistry::new();
        reg.insert_nip46(Arc::clone(&signer));
        let (tx, rx) = mpsc::channel::<SignerCompletion>();
        let mut pending = PendingSignerCompletions::new();
        let ujson = unsigned_json(&pubkey_hex);

        let brokered = broker_sign_request(
            &reg,
            &mut pending,
            "corr-nip46",
            &pubkey_hex,
            &ujson,
            &tx,
            &noop_wake(),
        );

        assert!(brokered, "NIP-46 signer should be brokered");
        assert!(
            rx.try_recv().is_err(),
            "pending NIP-46 sign must not complete before relay response"
        );
        let sent = transport.sent.lock().expect("sent lock").clone();
        assert_eq!(sent.len(), 1, "one NIP-46 sign_event RPC is queued");

        let unsigned = parse_unsigned_json(&ujson).expect("unsigned parses");
        let signed = match remote_user.sign(unsigned) {
            SignerOp::Ready(Ok(signed)) => signed,
            other => panic!("local fixture sign must complete: {other:?}"),
        };
        let response = serde_json::json!({
            "id": sent[0].id,
            "result": signed.to_nip01_json(),
        })
        .to_string();
        signer.ingest_rpc_response(&response);

        let ready = pending.drain_ready();
        assert_eq!(ready.len(), 1, "response must settle one pending sign");
        assert_eq!(ready[0].correlation_id, "corr-nip46");
        assert!(
            ready[0]
                .result
                .as_ref()
                .is_ok_and(|json| json.contains(&signed.id)),
            "completion must carry signed JSON: {:?}",
            ready[0].result
        );
    }

    #[test]
    fn nip46_transport_error_sends_error_completion() {
        let remote_user = LocalKeySigner::generate();
        let pubkey_hex = remote_user.pubkey().to_hex();
        let signer = Arc::new(nip46_signer_with_transport(
            &remote_user,
            Arc::new(FailingTransport),
        ));
        let mut reg = CapabilityProviderRegistry::new();
        reg.insert_nip46(signer);
        let (tx, rx) = mpsc::channel::<SignerCompletion>();
        let mut pending = PendingSignerCompletions::new();

        let brokered = broker_sign_request(
            &reg,
            &mut pending,
            "corr-nip46-fail",
            &pubkey_hex,
            &unsigned_json(&pubkey_hex),
            &tx,
            &noop_wake(),
        );

        assert!(
            brokered,
            "NIP-46 transport failure still resolves broker path"
        );
        let completion = rx.try_recv().expect("error completion is synchronous");
        assert_eq!(completion.correlation_id, "corr-nip46-fail");
        assert!(completion
            .result
            .expect_err("failure must be reported")
            .contains("transport closed"));
        assert!(
            pending.drain_ready().is_empty(),
            "failed send must not leave a parked op"
        );
    }
}
