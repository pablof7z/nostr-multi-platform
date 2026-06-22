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
use std::sync::{Arc, Mutex};

use super::{dispatch_action_bytes, nmp_app_dispatch_action_bytes};
use crate::free::nmp_free_string;
use crate::{nmp_app_free, nmp_app_new, NmpApp};
use nmp_core::dispatch_envelope::{
    encode_dispatch_envelope, DISPATCH_ENVELOPE_SCHEMA_VERSION, MAX_DISPATCH_ENVELOPE_BYTES,
};
use nmp_core::publish::{PublishAction, PublishTarget};
use nmp_core::substrate::{
    ActionContext, ActionModule, ActionPayload, ActionRejection,
};

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
        signer_pubkey: None,
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
        let out = unsafe { CStr::from_ptr(raw) }.to_string_lossy().into_owned();
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
        let raw = nmp_app_dispatch_action_bytes(
            appp,
            backing.as_ptr(),
            MAX_DISPATCH_ENVELOPE_BYTES + 1,
        );
        assert!(!raw.is_null());
        let out = unsafe { CStr::from_ptr(raw) }.to_string_lossy().into_owned();
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
    assert_eq!(parsed.get("error").and_then(|v| v.as_str()), Some("null app"));
}

#[test]
fn dispatch_bytes_null_ptr_via_c_symbol_returns_error_json() {
    // A null `ptr` is an empty buffer — rejected fail-closed, never deref'd.
    with_app(|app| {
        let ptr = std::ptr::addr_of!(*app).cast_mut();
        let raw = nmp_app_dispatch_action_bytes(ptr, std::ptr::null(), 0);
        assert!(!raw.is_null());
        let out = unsafe { CStr::from_ptr(raw) }.to_string_lossy().into_owned();
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
            signer_pubkey: None,
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

// ─── #1756 / S9 — opaque-passthrough route for app-owned host-op namespaces ──
//
// APP-OWNED host-op namespaces carry an `Action` the kernel does NOT model
// (`serde_json::Value`), so they cannot implement `ActionPayload` and leave
// `decode_payload` defaulted. They opt into the byte doorway via
// `accepts_opaque_payload() -> true`; the registry passes the envelope's opaque
// payload bytes UNDECODED to the app's serde deserialize and runs `start()`.
// This is the Cut-B enabler: app host-ops ride the byte doorway, so deleting the
// JSON doorway no longer strands them.

/// Captures the EXACT bytes `start()` received, so the e2e test can assert
/// byte-for-byte fidelity (not merely JSON-value equality) that `start()`
/// actually ran. `Action` is a `Box<RawValue>`: serde's `RawValue` preserves
/// the source JSON text VERBATIM (whitespace and key order included), so the
/// captured `get()` bytes are precisely the opaque payload bytes the route
/// handed through — a re-canonicalization or whitespace change would be caught.
struct CapturingOpaqueModule {
    seen: Arc<Mutex<Vec<Vec<u8>>>>,
}
impl ActionModule for CapturingOpaqueModule {
    const NAMESPACE: &'static str = "podcast.tasks"; // doctrine-allow: D9 — test-only app-owned namespace inside #[cfg(test)]
    type Action = Box<serde_json::value::RawValue>;

    // DELIBERATE opt-in to the opaque-passthrough byte route (#1756). The kernel
    // models no typed payload for this app-owned namespace.
    fn accepts_opaque_payload() -> bool {
        true
    }

    fn start(
        &self,
        _ctx: &mut ActionContext,
        action: Self::Action,
    ) -> Result<(), ActionRejection> {
        self.seen.lock().unwrap().push(action.get().as_bytes().to_vec());
        Ok(())
    }

    fn execute(
        &self,
        _action: Self::Action,
        _correlation_id: &str,
        _send: &dyn Fn(nmp_core::ActorCommand),
    ) -> Result<(), String> {
        // Fire-and-forget; the e2e assertion is on `start()` receiving the bytes.
        Ok(())
    }
}

/// A typed-only module (`decode_payload` is `Some`) that does NOT opt into
/// opaque-passthrough. Used to prove a typed module is never silently accepted
/// on the opaque lane when handed non-FlatBuffers bytes.
struct TypedOnlyModule;
impl ActionModule for TypedOnlyModule {
    const NAMESPACE: &'static str = "test.typed_only"; // doctrine-allow: D9 — test-only namespace inside #[cfg(test)]
    type Action = PublishAction;

    fn decode_payload(
        bytes: &[u8],
    ) -> Option<Result<Self::Action, nmp_core::substrate::ActionPayloadDecodeError>> {
        Some(<PublishAction as ActionPayload>::decode(bytes))
    }
    // accepts_opaque_payload defaulted to false.

    fn execute(
        &self,
        _action: Self::Action,
        _correlation_id: &str,
        _send: &dyn Fn(nmp_core::ActorCommand),
    ) -> Result<(), String> {
        Ok(())
    }
}

/// A non-opted untyped module: neither `decode_payload` (None) nor
/// `accepts_opaque_payload` (false). Must be REJECTED by the byte doorway — the
/// fail-closed default, not a blanket accept.
struct NonOptedUntypedModule;
impl ActionModule for NonOptedUntypedModule {
    const NAMESPACE: &'static str = "test.non_opted"; // doctrine-allow: D9 — test-only namespace inside #[cfg(test)]
    type Action = serde_json::Value;
    // Both byte-route opt-ins left defaulted.

    fn execute(
        &self,
        _action: Self::Action,
        _correlation_id: &str,
        _send: &dyn Fn(nmp_core::ActorCommand),
    ) -> Result<(), String> {
        Ok(())
    }
}

/// REAL e2e (#1756 / S9): an app-owned `serde_json::Value` module opts into
/// opaque-passthrough, is registered on the app, and a finished
/// `DispatchEnvelope` carrying the namespace + an OPAQUE JSON-bytes payload is
/// dispatched through the ACTUAL `nmp_app_dispatch_action_bytes` C symbol. The
/// module's `start()` must receive the EXACT opaque bytes (after its own JSON
/// deserialize) and the doorway must echo the host-supplied correlation_id.
///
/// Load-bearing: revert the opaque route in `decode_byte_action` (the
/// `accepts_opaque_payload` arm) and this goes red — the app module's namespace
/// fails closed as `NotTypedCapable`, `start()` never runs, and the dispatch
/// returns `{"error":…}` instead of the correlation_id.
#[test]
fn opaque_passthrough_app_module_receives_exact_bytes_via_c_symbol() {
    let seen: Arc<Mutex<Vec<Vec<u8>>>> = Arc::new(Mutex::new(Vec::new()));
    let app = nmp_app_new();
    // SAFETY: `nmp_app_new` never returns null; valid until `nmp_app_free`.
    let app_mut = unsafe { &mut *app };
    app_mut.register_action(CapturingOpaqueModule {
        seen: Arc::clone(&seen),
    });

    // The app owns the payload format: it stamps its OWN JSON-bytes as the
    // envelope's opaque payload. NMP never models this. Use DELIBERATELY
    // non-canonical bytes (extra whitespace + a key order serde_json would
    // reorder on re-emit) so the assertion proves BYTE-FOR-BYTE passthrough, not
    // mere JSON-value equality: any re-canonicalization on the route would change
    // these bytes and fail the test.
    let opaque_payload: &[u8] = b"{ \"task\":\"refresh\" ,  \"feed\": \"podcast-123\" }";
    let envelope = encode_dispatch_envelope(
        "corr-opaque-1",
        "podcast.tasks",
        DISPATCH_ENVELOPE_SCHEMA_VERSION,
        opaque_payload,
    );

    let ptr = std::ptr::addr_of!(*app_mut).cast_mut();
    let raw = nmp_app_dispatch_action_bytes(ptr, envelope.as_ptr(), envelope.len());
    assert!(!raw.is_null(), "non-null app must never return NULL (D6)");
    // SAFETY: `raw` is a freshly minted NUL-terminated string from the call.
    let out = unsafe { CStr::from_ptr(raw) }.to_string_lossy().into_owned();
    nmp_free_string(raw);

    let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(
        parsed.get("correlation_id").and_then(|v| v.as_str()),
        Some("corr-opaque-1"),
        "opaque-passthrough dispatch must accept + echo the host correlation_id: {out}"
    );

    let captured = seen.lock().unwrap();
    assert_eq!(
        captured.len(),
        1,
        "the app module's start() must have run exactly once"
    );
    assert_eq!(
        captured[0].as_slice(),
        opaque_payload,
        "start() must receive the EXACT opaque payload bytes (byte-for-byte) the \
         app stamped — proves the route is genuinely opaque-passthrough, not a \
         re-serialize"
    );
    drop(captured);
    nmp_app_free(app);
}

/// NEGATIVE (b): a typed-only module (decode_payload Some, NOT opaque-opted)
/// handed bytes that are NOT valid typed FlatBuffers must FAIL CLOSED on the
/// typed decode — it is NEVER silently accepted via the opaque lane.
#[test]
fn opaque_passthrough_typed_only_module_is_not_silently_opaque_accepted() {
    let app = nmp_app_new();
    let app_mut = unsafe { &mut *app };
    app_mut.register_action(TypedOnlyModule);

    // Hand it JSON-bytes (what an opaque module would accept) — but this module
    // is typed-only, so its FlatBuffers `decode_payload` must reject these.
    let json_bytes = serde_json::to_vec(&serde_json::json!({ "not": "flatbuffers" })).unwrap();
    let envelope = encode_dispatch_envelope(
        "corr-typed-only",
        "test.typed_only",
        DISPATCH_ENVELOPE_SCHEMA_VERSION,
        &json_bytes,
    );
    let out = dispatch_action_bytes(Some(app_mut), &envelope);
    let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert!(
        parsed.get("error").is_some(),
        "a typed-only module must fail closed on non-FlatBuffers bytes, not opaque-accept: {out}"
    );
    assert!(
        parsed.get("correlation_id").is_none(),
        "no correlation_id must be returned for a rejected dispatch: {out}"
    );
    nmp_app_free(app);
}

/// NEGATIVE (c): an untyped module that opted into NEITHER route is REJECTED
/// (fail-closed default — not a blanket accept of any untyped namespace).
#[test]
fn opaque_passthrough_non_opted_untyped_module_is_rejected() {
    let app = nmp_app_new();
    let app_mut = unsafe { &mut *app };
    app_mut.register_action(NonOptedUntypedModule);

    let payload = serde_json::to_vec(&serde_json::json!({ "anything": true })).unwrap();
    let envelope = encode_dispatch_envelope(
        "corr-non-opted",
        "test.non_opted",
        DISPATCH_ENVELOPE_SCHEMA_VERSION,
        &payload,
    );
    let out = dispatch_action_bytes(Some(app_mut), &envelope);
    let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
    let err = parsed
        .get("error")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("a non-opted untyped module must be rejected: {out}"));
    assert!(
        err.contains("typed FlatBuffers payloads") || err.contains("does not support"),
        "rejection must be the not-typed-capable fail-closed reason: {err}"
    );
    nmp_app_free(app);
}
