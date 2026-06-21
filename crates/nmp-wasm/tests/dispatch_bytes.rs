//! ADR-0064 / S2 (#1750) — the wasm binary write doorway (`dispatch_bytes`).
//!
//! Proves the raw-byte inbound channel: a transferable `Uint8Array` of a
//! finished `DispatchEnvelope` crosses, decodes through the ONE core decode
//! path, and routes by `action_namespace`. Fail-closed cases surface a
//! data-shaped `WorkerEvent::Error` — never a panic, never a silent accept.

use nmp_core::dispatch_envelope::{
    encode_dispatch_envelope, DISPATCH_ENVELOPE_SCHEMA_VERSION,
};
use nmp_wasm::{CapabilityFailure, WasmRuntime, WorkerEvent};

fn envelope(namespace: &str, correlation: &str, version: u32, payload: &[u8]) -> Vec<u8> {
    encode_dispatch_envelope(correlation, namespace, version, payload)
}

#[test]
fn binary_doorway_decodes_and_routes_by_namespace() {
    let mut runtime = WasmRuntime::new();
    // Opaque payload — the binary lane carries it verbatim, never interpreted.
    let bytes = envelope("nmp.publish", "corr-1", DISPATCH_ENVELOPE_SCHEMA_VERSION, b"\x01\x02\x03");

    let events = runtime.dispatch_bytes(&bytes);

    // No signer installed → the honest write-path reason, keyed to the decoded
    // namespace + correlation_id. Proves the envelope crossed and decoded.
    assert!(matches!(
        &events[..],
        [WorkerEvent::CapabilityFailure(CapabilityFailure {
            capability,
            correlation_id,
            ..
        })] if capability == "nmp.publish" && correlation_id == "corr-1"
    ));
}

#[test]
fn binary_doorway_rejects_schema_version_mismatch_fail_closed() {
    let mut runtime = WasmRuntime::new();
    let bad = envelope("nmp.publish", "corr-2", DISPATCH_ENVELOPE_SCHEMA_VERSION + 1, b"p");

    let events = runtime.dispatch_bytes(&bad);

    // Fail closed: a data-shaped error, NOT a routed dispatch.
    assert!(matches!(
        &events[..],
        [WorkerEvent::Error { code, .. }] if code == "dispatch_envelope_rejected"
    ));
}

#[test]
fn binary_doorway_rejects_wrong_file_identifier_fail_closed() {
    let mut runtime = WasmRuntime::new();
    let mut bytes = envelope("nmp.publish", "c", DISPATCH_ENVELOPE_SCHEMA_VERSION, b"p");
    bytes[4..8].copy_from_slice(b"NMPU"); // read-direction magic
    let events = runtime.dispatch_bytes(&bytes);
    assert!(matches!(
        &events[..],
        [WorkerEvent::Error { code, .. }] if code == "dispatch_envelope_rejected"
    ));
}

#[test]
fn binary_doorway_rejects_garbage_fail_closed() {
    let mut runtime = WasmRuntime::new();
    let events = runtime.dispatch_bytes(b"not a flatbuffer");
    assert!(matches!(
        &events[..],
        [WorkerEvent::Error { code, .. }] if code == "dispatch_envelope_rejected"
    ));
}
