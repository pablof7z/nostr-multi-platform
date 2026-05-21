//! Tests for `Nip46Transport` trait-object dispatch and `Nip46Rpc` shape.
//!
//! The trait surface is intentionally tiny — `send_rpc(&self, rpc) -> Result`.
//! The kernel holds an `Arc<dyn Nip46Transport>` (doctrine D0 — `nmp-core`
//! must not depend on `nmp-signers`), so the trait must be object-safe and
//! round-trip an `Nip46Rpc` through a `Box<dyn Nip46Transport>` without
//! requiring the concrete type at the call site.

use std::sync::{Arc, Mutex};

use nmp_signer_iface::{Nip46Rpc, Nip46Transport, SignerError};

/// Stub transport that records every RPC it receives. Mirrors the shape the
/// test infrastructure described in `nip46_transport.rs` ("tests can implement
/// it with `Vec<Nip46Rpc>` + an inject-response helper").
#[derive(Debug, Default)]
struct StubTransport {
    sent: Mutex<Vec<Nip46Rpc>>,
    fail_next: Mutex<Option<SignerError>>,
}

impl StubTransport {
    fn sent(&self) -> Vec<Nip46Rpc> {
        self.sent.lock().unwrap().clone()
    }

    fn arm_failure(&self, err: SignerError) {
        *self.fail_next.lock().unwrap() = Some(err);
    }
}

impl Nip46Transport for StubTransport {
    fn send_rpc(&self, rpc: Nip46Rpc) -> Result<(), SignerError> {
        if let Some(err) = self.fail_next.lock().unwrap().take() {
            return Err(err);
        }
        self.sent.lock().unwrap().push(rpc);
        Ok(())
    }
}

fn sample_rpc() -> Nip46Rpc {
    Nip46Rpc {
        id: "rpc-1".into(),
        body_json: r#"{"id":"rpc-1","method":"sign_event","params":["..."]}"#.into(),
        encrypted_payload: "base64-blob==".into(),
        relays: vec!["wss://relay.example".into(), "wss://relay.test".into()],
        remote_pubkey_hex: "ff".repeat(32),
    }
}

#[test]
fn box_dyn_transport_round_trips_rpc() {
    // Construct via `Box<dyn Nip46Transport>` to exercise the dyn-trait
    // dispatch path. The test inspects the stub via a side-channel `Arc`
    // because the trait object intentionally hides the concrete type.
    let stub = Arc::new(StubTransport::default());
    let dispatch: &dyn Nip46Transport = stub.as_ref();

    let rpc = sample_rpc();
    dispatch
        .send_rpc(rpc.clone())
        .expect("&dyn Nip46Transport must accept send_rpc");

    let captured = stub.sent();
    assert_eq!(captured.len(), 1, "stub must record exactly one RPC");
    let got = &captured[0];
    assert_eq!(got.id, rpc.id);
    assert_eq!(got.body_json, rpc.body_json);
    assert_eq!(got.encrypted_payload, rpc.encrypted_payload);
    assert_eq!(got.relays, rpc.relays);
    assert_eq!(got.remote_pubkey_hex, rpc.remote_pubkey_hex);
}

#[test]
fn arc_dyn_transport_round_trips_rpc() {
    // The production seam is `Arc<dyn Nip46Transport>` (one transport shared
    // across the kernel and any signer modules). Confirm that path works too.
    let stub = Arc::new(StubTransport::default());
    let dyn_arc: Arc<dyn Nip46Transport> = stub.clone();

    let a = sample_rpc();
    let mut b = sample_rpc();
    b.id = "rpc-2".into();

    dyn_arc.send_rpc(a.clone()).expect("a must send");
    dyn_arc.send_rpc(b.clone()).expect("b must send");

    let captured = stub.sent();
    assert_eq!(captured.len(), 2);
    assert_eq!(captured[0].id, "rpc-1");
    assert_eq!(captured[1].id, "rpc-2");
}

#[test]
fn transport_error_propagates_via_trait_object() {
    // A transport-side failure must reach the caller as a typed `SignerError`
    // — the kernel maps this into a `toast` payload at the FFI boundary.
    let stub = Arc::new(StubTransport::default());
    stub.arm_failure(SignerError::Backend("relay down".into()));

    let dyn_arc: Arc<dyn Nip46Transport> = stub.clone();
    let err = dyn_arc
        .send_rpc(sample_rpc())
        .expect_err("transport must surface the armed failure");

    assert!(matches!(err, SignerError::Backend(_)));

    // After the armed failure, the next send must succeed and be recorded —
    // confirms the failure didn't poison the transport.
    dyn_arc
        .send_rpc(sample_rpc())
        .expect("subsequent send must succeed");
    assert_eq!(stub.sent().len(), 1, "only the post-failure RPC is recorded");
}

#[test]
fn nip46_rpc_clones_independently() {
    // `Nip46Rpc` is `Clone` (per `#[derive(Clone, Debug)]`). The kernel often
    // clones an RPC into a log/observer before handing it to the transport;
    // mutations on the clone must not affect the original.
    let original = sample_rpc();
    let mut clone = original.clone();
    clone.id = "rpc-other".into();
    clone.relays.push("wss://injected".into());

    assert_eq!(original.id, "rpc-1");
    assert_eq!(
        original.relays.len(),
        2,
        "original relays must not see the clone's push"
    );
}

#[test]
fn nip46_rpc_debug_does_not_panic_on_empty_fields() {
    // Defensive: `Debug` is auto-derived but the type is held in user-facing
    // diagnostics and tests; an empty-field instance must format without panic.
    let empty = Nip46Rpc {
        id: String::new(),
        body_json: String::new(),
        encrypted_payload: String::new(),
        relays: Vec::new(),
        remote_pubkey_hex: String::new(),
    };
    let s = format!("{empty:?}");
    assert!(s.contains("Nip46Rpc"));
}
