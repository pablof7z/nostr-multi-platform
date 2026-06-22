//! Tests for the `ActionRejection::InvalidCoded` wire shape through the FFI
//! action-dispatch entry points (issue #1734).
//!
//! Covers BOTH doorways:
//! * JSON doorway (`dispatch_action_json`) — `coded_rejection_includes_code_field_in_ffi_json`
//! * Byte doorway (`dispatch_action_bytes`) — `coded_rejection_byte_doorway_includes_code_field`
//!
//! Moved out of `action/tests.rs` to keep that file at its baselined LOC cap.

use super::super::{nmp_app_free, nmp_app_new};
use super::*;

/// Coded-rejection test module under `test.coded_reject` — `start()` returns
/// an `ActionRejection::InvalidCoded` with a stable `code` (issue #1734).
/// Used to verify that the FFI action-result JSON carries `{"error":…,"code":…}`.
struct TestCodedRejectModule; // doctrine-allow: D9 — test-only namespace inside #[cfg(test)]; never on the wire
impl nmp_core::substrate::ActionModule for TestCodedRejectModule {
    const NAMESPACE: &'static str = "test.coded_reject"; // doctrine-allow: D9
    type Action = serde_json::Value;
    fn start(
        &self,
        _ctx: &mut ActionContext,
        _action: Self::Action,
    ) -> Result<(), ActionRejection> {
        Err(ActionRejection::InvalidCoded {
            code: "test_coded_error",
            message: "coded rejection: field required".into(),
        })
    }
    fn execute(
        &self,
        _action: Self::Action,
        _correlation_id: &str,
        _send: &dyn Fn(nmp_core::ActorCommand),
    ) -> Result<(), String> {
        Ok(())
    }
}

/// A typed `ActionModule` whose `start()` returns `Err(ActionRejection::InvalidCoded)`
/// produces `{"error":"…","code":"…"}` — the structured rejection wire shape
/// introduced by issue #1734. The `code` field carries the stable machine token;
/// the `error` field carries the English fallback. Both must be present.
#[test]
fn coded_rejection_includes_code_field_in_ffi_json() {
    let app = nmp_app_new();
    // SAFETY: `nmp_app_new` never returns null; pointer is valid until `nmp_app_free` below.
    let app_mut = unsafe { &mut *app };
    app_mut.register_action(TestCodedRejectModule);

    let out = dispatch_action_json(Some(&*app_mut), "test.coded_reject", "{}");
    let parsed: serde_json::Value =
        serde_json::from_str(&out).unwrap_or_else(|_| panic!("output was not valid JSON: {out}"));

    let err = parsed
        .get("error")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("expected 'error' field in: {out}"));
    assert!(
        err.contains("coded rejection"),
        "English fallback must be in 'error'; got: {err}"
    );

    let code = parsed
        .get("code")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("expected 'code' field in: {out}"));
    assert_eq!(
        code, "test_coded_error",
        "stable machine code must be in 'code'; got: {code}"
    );

    // Sanity: no correlation_id on a start() rejection (action never started).
    assert!(
        parsed.get("correlation_id").is_none(),
        "a start()-phase rejection must not include a correlation_id; got: {out}"
    );

    nmp_app_free(app);
}

/// Bytes-capable coded-rejection test module under `test.coded_reject.bytes`.
///
/// `decode_payload` returns `Some(Ok(...))` for ANY byte slice so the typed
/// bytes doorway (`start_bytes`) proceeds to `start()`, which then returns
/// `ActionRejection::InvalidCoded`. This is a TEST-ONLY bypass of the normal
/// typed-payload decode — production modules decode real FlatBuffers.
struct TestCodedRejectBytesModule; // doctrine-allow: D9 — test-only namespace inside #[cfg(test)]; never on the wire
impl nmp_core::substrate::ActionModule for TestCodedRejectBytesModule {
    const NAMESPACE: &'static str = "test.coded_reject.bytes"; // doctrine-allow: D9
    type Action = serde_json::Value;
    /// Opt into the typed bytes doorway by returning `Some(Ok(...))` for any
    /// payload. The content is irrelevant for this rejection test; only `start()`
    /// matters. Production modules would decode real FlatBuffers here.
    fn decode_payload(
        _bytes: &[u8],
    ) -> Option<Result<Self::Action, nmp_core::substrate::ActionPayloadDecodeError>> {
        Some(Ok(serde_json::Value::Null))
    }
    fn start(
        &self,
        _ctx: &mut ActionContext,
        _action: Self::Action,
    ) -> Result<(), ActionRejection> {
        Err(ActionRejection::InvalidCoded {
            code: "test_coded_error",
            message: "coded rejection: field required".into(),
        })
    }
    fn execute(
        &self,
        _action: Self::Action,
        _correlation_id: &str,
        _send: &dyn Fn(nmp_core::ActorCommand),
    ) -> Result<(), String> {
        Ok(())
    }
}

/// A bytes-capable `ActionModule` whose `start()` returns
/// `Err(ActionRejection::InvalidCoded)` produces `{"error":"…","code":"…"}` through
/// the BYTE doorway (`dispatch_action_bytes`) — the same structured rejection wire
/// shape verified by `coded_rejection_includes_code_field_in_ffi_json` for the JSON
/// doorway.
///
/// LOAD-BEARING: if `bytes.rs`'s rejection path were reverted from `rejection_json`
/// to a plain `error_json(rejection_message(...))` (which omits the `code` field),
/// this test would FAIL — the `"code"` field assertion at the end catches the
/// regression. Both `"error"` and `"code"` must be present.
#[test]
fn coded_rejection_byte_doorway_includes_code_field() {
    use nmp_core::dispatch_envelope::{
        encode_dispatch_envelope, DISPATCH_ENVELOPE_SCHEMA_VERSION,
    };

    let app = nmp_app_new();
    // SAFETY: `nmp_app_new` never returns null; pointer is valid until `nmp_app_free` below.
    let app_mut = unsafe { &mut *app };
    app_mut.register_action(TestCodedRejectBytesModule);

    // Build a well-formed DispatchEnvelope for the bytes namespace. The payload
    // bytes are a minimal non-empty slice — `decode_payload` ignores the content
    // and returns `Some(Ok(...))` unconditionally (test-only bypass).
    let envelope = encode_dispatch_envelope(
        "corr-coded-bytes-1",
        "test.coded_reject.bytes",
        DISPATCH_ENVELOPE_SCHEMA_VERSION,
        &[0u8; 4], // arbitrary payload; decode_payload ignores it
    );

    let out = super::bytes::dispatch_action_bytes(Some(app_mut), &envelope);
    let parsed: serde_json::Value =
        serde_json::from_str(&out).unwrap_or_else(|_| panic!("output was not valid JSON: {out}"));

    let err = parsed
        .get("error")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("expected 'error' field in byte-doorway output: {out}"));
    assert!(
        err.contains("coded rejection"),
        "English fallback must be in 'error'; got: {err}"
    );

    // LOAD-BEARING: this assertion FAILS if bytes.rs reverts to plain error_json
    // (which omits the code field). `rejection_json` must format InvalidCoded as
    // {"error":"…","code":"…"}.
    let code = parsed
        .get("code")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("expected 'code' field in byte-doorway output: {out}"));
    assert_eq!(
        code, "test_coded_error",
        "stable machine code must be in 'code' through the byte doorway; got: {code}"
    );

    // A start()-phase rejection must not carry a correlation_id (action never started).
    assert!(
        parsed.get("correlation_id").is_none(),
        "a start()-phase rejection must not include a correlation_id; got: {out}"
    );

    nmp_app_free(app);
}
