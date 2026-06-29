//! Tests for the FFI-free dispatch core moved to `nmp-native-runtime`.
//!
//! These cover the typed `dispatch_action_bytes_typed` function — the shared
//! core consumed by both the C-ABI (`nmp-ffi`) and the UniFFI surface
//! (`nmp-uniffi`). They prove correlation_id/error/code semantics so the same
//! assertions need not be duplicated at both ABI layers.

use nmp_core::dispatch_envelope::{
    encode_dispatch_envelope, DISPATCH_ENVELOPE_SCHEMA_VERSION, MAX_DISPATCH_ENVELOPE_BYTES,
};
use nmp_core::substrate::ActionContext;
use nmp_core::substrate::{ActionModule, ActionPayload, ActionRejection};
use nmp_core::actor::ActorCommand;

use super::dispatch_action_bytes_typed;
use crate::new_app;

// ── Test action modules ───────────────────────────────────────────────────────

/// Always-succeeds bytes-capable action module (echo test).
struct EchoModule; // doctrine-allow: action_namespace — test-only namespace inside #[cfg(test)]
impl ActionModule for EchoModule {
    const NAMESPACE: &'static str = "test.dispatch_core.echo"; // doctrine-allow: action_namespace — test fixture
    type Action = serde_json::Value;
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
        Ok(())
    }
    fn execute(
        &self,
        _ctx: &nmp_core::substrate::ActionContext,
        _action: Self::Action,
        _correlation_id: &str,
        _send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        Ok(())
    }
}

/// Coded-rejection action module.
struct CodedRejectModule; // doctrine-allow: action_namespace — test-only namespace inside #[cfg(test)]
impl ActionModule for CodedRejectModule {
    const NAMESPACE: &'static str = "test.dispatch_core.coded_reject"; // doctrine-allow: action_namespace — test fixture
    type Action = serde_json::Value;
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
            code: "dispatch_core_code",
            message: "typed core coded rejection".into(),
        })
    }
    fn execute(
        &self,
        _ctx: &nmp_core::substrate::ActionContext,
        _action: Self::Action,
        _correlation_id: &str,
        _send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        Ok(())
    }
}

/// Plain-reject action module.
struct PlainRejectModule; // doctrine-allow: action_namespace — test-only namespace inside #[cfg(test)]
impl ActionModule for PlainRejectModule {
    const NAMESPACE: &'static str = "test.dispatch_core.plain_reject"; // doctrine-allow: action_namespace — test fixture
    type Action = serde_json::Value;
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
        Err(ActionRejection::Invalid("plain rejection".into()))
    }
    fn execute(
        &self,
        _ctx: &nmp_core::substrate::ActionContext,
        _action: Self::Action,
        _correlation_id: &str,
        _send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        Ok(())
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn make_envelope(correlation_id: &str, namespace: &str) -> Vec<u8> {
    encode_dispatch_envelope(
        correlation_id,
        namespace,
        DISPATCH_ENVELOPE_SCHEMA_VERSION,
        &[0u8; 4],
    )
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// D6: malformed bytes produce an error outcome, never a panic.
#[test]
fn malformed_envelope_produces_error_outcome() {
    let app = new_app();
    let outcome = dispatch_action_bytes_typed(&app, &[0u8; 16]);
    assert!(outcome.error.is_some(), "expected error for malformed bytes");
    assert!(outcome.correlation_id.is_none());
    assert!(outcome.code.is_none());
}

/// D6: oversize bytes produce an error outcome without forming a slice panic.
#[test]
fn oversize_bytes_produce_error_outcome() {
    let app = new_app();
    // Pass a tiny slice but report oversize len via the MAX+1 check (we can't
    // actually allocate 1MiB+1 in a test; instead verify that a slice whose
    // len exceeds MAX is rejected by the inline guard and the decoder alike).
    let oversize = vec![0u8; MAX_DISPATCH_ENVELOPE_BYTES + 1];
    let outcome = dispatch_action_bytes_typed(&app, &oversize);
    assert!(outcome.error.is_some(), "expected error for oversize envelope");
    assert!(outcome.correlation_id.is_none());
}

/// ADR-0064 §4: accepted dispatch echoes the host-supplied correlation_id.
#[test]
fn dispatch_preserves_host_supplied_correlation_id() {
    let mut app = new_app();
    let _ = app.register_action(EchoModule);
    let envelope = make_envelope("corr-typed-core-1", EchoModule::NAMESPACE);
    let outcome = dispatch_action_bytes_typed(&app, &envelope);
    assert_eq!(
        outcome.correlation_id.as_deref(),
        Some("corr-typed-core-1"),
        "typed core must echo the host-supplied correlation_id"
    );
    assert!(outcome.error.is_none());
    assert!(outcome.code.is_none());
}

/// Coded rejection produces both `error` and `code` fields (load-bearing:
/// corresponds to `coded_rejection_tests.rs:122` on the C-ABI side).
#[test]
fn coded_rejection_carries_both_error_and_code() {
    let mut app = new_app();
    let _ = app.register_action(CodedRejectModule);
    let envelope = make_envelope("corr-coded", CodedRejectModule::NAMESPACE);
    let outcome = dispatch_action_bytes_typed(&app, &envelope);
    assert!(
        outcome.error.as_deref().map_or(false, |e| e.contains("typed core coded rejection")),
        "error must carry the human message; got {:?}",
        outcome.error
    );
    assert_eq!(
        outcome.code.as_deref(),
        Some("dispatch_core_code"),
        "code must carry the stable machine token"
    );
    assert!(
        outcome.correlation_id.is_none(),
        "start()-phase rejection must not carry a correlation_id"
    );
}

/// Plain rejection populates `error` but leaves `code` empty.
#[test]
fn plain_rejection_has_error_but_no_code() {
    let mut app = new_app();
    let _ = app.register_action(PlainRejectModule);
    let envelope = make_envelope("corr-plain", PlainRejectModule::NAMESPACE);
    let outcome = dispatch_action_bytes_typed(&app, &envelope);
    assert!(outcome.error.is_some(), "plain rejection must have an error");
    assert!(outcome.code.is_none(), "plain rejection must NOT have a code");
    assert!(outcome.correlation_id.is_none());
}

/// Unknown namespace produces an error outcome (D6 fail-closed).
#[test]
fn unknown_namespace_produces_error_outcome() {
    let app = new_app();
    let envelope = make_envelope("corr-unknown", "test.dispatch_core.no_such_module");
    let outcome = dispatch_action_bytes_typed(&app, &envelope);
    assert!(outcome.error.is_some(), "unknown namespace must produce an error outcome");
    assert!(outcome.correlation_id.is_none());
}
