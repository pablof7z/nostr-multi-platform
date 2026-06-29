//! ADR-0064 / S4 (#1752) — native byte-doorway (`dispatch_action_bytes`) tests.
//!
//! These exercise the typed-FlatBuffers twin of the JSON doorway: a finished
//! [`nmp_core::dispatch_envelope::DispatchEnvelope`] decodes (S2), routes by
//! `action_namespace` into the registry's typed `start_bytes`/`execute_bytes`
//! (S3), and returns the same `{"correlation_id":…}` / `{"error":…}` shape.
//!
//! The NEGATIVE cases are load-bearing: malformed / oversize / wrong-version /
//! unknown-namespace / null-app must each come back as a data-shaped error
//! (D6) — never a panic across the ABI.

use std::ffi::CStr;

use super::{dispatch_action_bytes, nmp_app_dispatch_action_bytes};
use crate::free::nmp_free_string;
use crate::{nmp_app_free, nmp_app_new, NmpApp};
use nmp_core::dispatch_envelope::{
    encode_dispatch_envelope, DISPATCH_ENVELOPE_SCHEMA_VERSION, MAX_DISPATCH_ENVELOPE_BYTES,
};
use nmp_core::publish::{PublishAction, PublishTarget};
use nmp_core::substrate::ActionPayload;

/// Run `body` against a fresh `NmpApp`, freeing it afterwards.
fn with_app(body: impl FnOnce(&NmpApp)) {
    let app = nmp_app_new();
    // SAFETY: `nmp_app_new` never returns null; valid until `nmp_app_free`.
    body(unsafe { &*app });
    nmp_app_free(app);
}

/// Build a finished `DispatchEnvelope` carrying a typed `nmp.publish`
/// `PublishRaw` payload (needs no signed-event fixture — the actor signs).
fn publish_raw_envelope(correlation_id: &str) -> Vec<u8> {
    let action = PublishAction::PublishRaw {
        kind: 1,
        tags: vec![],
        content: "byte-doorway smoke".to_string(),
        target: PublishTarget::Auto,
        signer: Default::default(),
    };
    let payload = action.encode();
    encode_dispatch_envelope(
        correlation_id,
        "nmp.publish",
        DISPATCH_ENVELOPE_SCHEMA_VERSION,
        &payload,
    )
}

// ─── Happy path ──────────────────────────────────────────────────────────────

#[test]
fn dispatch_bytes_returns_host_supplied_correlation_id() {
    // ADR-0064 §4: on the byte lane the operation identity is the HOST-SUPPLIED
    // envelope `correlation_id`, threaded end-to-end — NOT a kernel-minted id.
    // The host's spinner is keyed on the id it stamped into the envelope, so the
    // accept envelope MUST echo it back (substituting a fresh id would strand
    // the spinner — the identity-substitution class #1748 closed).
    with_app(|app| {
        let envelope = publish_raw_envelope("corr-bytes-1");
        let out = dispatch_action_bytes(Some(app), &envelope);
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        let id = parsed
            .get("correlation_id")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| panic!("expected a correlation_id field, got: {out}"));
        assert_eq!(
            id, "corr-bytes-1",
            "byte doorway echoes the host-supplied envelope correlation_id (ADR-0064 §4)"
        );
    });
}

#[test]
fn dispatch_bytes_drives_through_the_c_symbol() {
    // Exercise the actual `extern "C"` entry, not just the pure core, so the
    // ptr/len → slice path and the heap-string return are covered.
    with_app(|app| {
        let envelope = publish_raw_envelope("corr-c-abi");
        let ptr = std::ptr::addr_of!(*app).cast_mut();
        let raw = nmp_app_dispatch_action_bytes(ptr, envelope.as_ptr(), envelope.len());
        assert!(!raw.is_null(), "non-null app must never return NULL (D6)");
        // SAFETY: `raw` is a freshly minted NUL-terminated string from the call.
        let out = unsafe { CStr::from_ptr(raw) }
            .to_string_lossy()
            .into_owned();
        nmp_free_string(raw);
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(
            parsed.get("correlation_id").and_then(|v| v.as_str()),
            Some("corr-c-abi"),
            "the C symbol echoes the host-supplied envelope correlation_id: {out}"
        );
    });
}

#[test]
fn dispatch_bytes_oversize_via_c_symbol_rejects_before_slice() {
    // The ABI gates `len > MAX_DISPATCH_ENVELOPE_BYTES` BEFORE forming a slice,
    // so a hostile length never constructs a `&[u8]` of that span. We pass a
    // dangling (but non-null) pointer with an oversize `len`: a correct
    // implementation rejects on `len` alone and never dereferences `ptr`.
    with_app(|app| {
        let appp = std::ptr::addr_of!(*app).cast_mut();
        // A small real allocation; the oversize `len` is a lie the gate must
        // catch before any read. NonNull, but we assert `ptr` is never read.
        let backing = [0u8; 8];
        let raw =
            nmp_app_dispatch_action_bytes(appp, backing.as_ptr(), MAX_DISPATCH_ENVELOPE_BYTES + 1);
        assert!(!raw.is_null());
        let out = unsafe { CStr::from_ptr(raw) }
            .to_string_lossy()
            .into_owned();
        nmp_free_string(raw);
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        let err = parsed.get("error").and_then(|v| v.as_str()).unwrap();
        assert!(err.contains("oversize"), "got: {err}");
    });
}

// ─── Fail-closed negatives (D6 — data, never a panic) ────────────────────────

#[test]
fn dispatch_bytes_null_app_returns_error_json() {
    let envelope = publish_raw_envelope("corr-x");
    let out = dispatch_action_bytes(None, &envelope);
    let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(
        parsed.get("error").and_then(|v| v.as_str()),
        Some("null app")
    );
}

#[test]
fn dispatch_bytes_null_ptr_via_c_symbol_returns_error_json() {
    // A null `ptr` is an empty buffer — rejected fail-closed, never deref'd.
    with_app(|app| {
        let ptr = std::ptr::addr_of!(*app).cast_mut();
        let raw = nmp_app_dispatch_action_bytes(ptr, std::ptr::null(), 0);
        assert!(!raw.is_null());
        let out = unsafe { CStr::from_ptr(raw) }
            .to_string_lossy()
            .into_owned();
        nmp_free_string(raw);
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert!(parsed.get("error").is_some(), "expected error: {out}");
    });
}

#[test]
fn dispatch_bytes_empty_buffer_returns_error_json() {
    with_app(|app| {
        let out = dispatch_action_bytes(Some(app), &[]);
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert!(parsed.get("error").is_some(), "expected error: {out}");
    });
}

#[test]
fn dispatch_bytes_malformed_buffer_returns_error_json() {
    with_app(|app| {
        // Long enough to pass the < 8 short-circuit but with a wrong file
        // identifier / garbage — the S2 verifier rejects it.
        let garbage = vec![0xABu8; 64];
        let out = dispatch_action_bytes(Some(app), &garbage);
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert!(parsed.get("error").is_some(), "expected error: {out}");
    });
}

#[test]
fn dispatch_bytes_oversize_buffer_returns_error_json() {
    with_app(|app| {
        let oversize = vec![0u8; MAX_DISPATCH_ENVELOPE_BYTES + 1];
        let out = dispatch_action_bytes(Some(app), &oversize);
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        let err = parsed.get("error").and_then(|v| v.as_str()).unwrap();
        assert!(err.contains("oversize"), "got: {err}");
    });
}

#[test]
fn dispatch_bytes_wrong_schema_version_returns_error_json() {
    with_app(|app| {
        let action = PublishAction::PublishRaw {
            kind: 1,
            tags: vec![],
            content: "v-trip".to_string(),
            target: PublishTarget::Auto,
            signer: Default::default(),
        };
        let payload = action.encode();
        // Stamp an unrecognised envelope schema_version — the S2 tripwire
        // rejects it BEFORE any routing.
        let envelope = encode_dispatch_envelope(
            "corr-vtrip",
            "nmp.publish",
            DISPATCH_ENVELOPE_SCHEMA_VERSION + 999,
            &payload,
        );
        let out = dispatch_action_bytes(Some(app), &envelope);
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        let err = parsed.get("error").and_then(|v| v.as_str()).unwrap();
        assert!(err.contains("schema_version"), "got: {err}");
    });
}

#[test]
fn dispatch_bytes_unknown_namespace_returns_error_json() {
    with_app(|app| {
        // A well-formed envelope routed to a namespace no module claims.
        let envelope = encode_dispatch_envelope(
            "corr-unknown",
            "nmp.does.not.exist",
            DISPATCH_ENVELOPE_SCHEMA_VERSION,
            &[1u8, 2, 3, 4],
        );
        let out = dispatch_action_bytes(Some(app), &envelope);
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        let err = parsed.get("error").and_then(|v| v.as_str()).unwrap();
        assert!(err.contains("unknown action namespace"), "got: {err}");
    });
}
