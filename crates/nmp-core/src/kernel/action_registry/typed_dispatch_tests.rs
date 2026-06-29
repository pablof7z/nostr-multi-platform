//! End-to-end tests for the typed-bytes (ADR-0064 / S3 #1751) registry doorway:
//! a `DispatchEnvelope` payload's bytes → `start_bytes`/`execute_bytes` → typed
//! decode → `start()`/`execute()`. Every fail-closed gate asserts the NEGATIVE
//! (bad schema_version / not-typed-capable / unknown namespace → REJECTED).

use super::*;
use crate::publish::{PublishAction, PublishTarget};
use crate::substrate::{ActionContext, ActionPayload};
fn ctx() -> ActionContext {
    ActionContext::default()
}

// ---- Happy path: typed bytes → start_bytes → minted correlation_id ----------

// ---- True S2 → S3 seam: DispatchEnvelope bytes → decode → start_bytes -------

#[test]
fn dispatch_envelope_bytes_decode_and_route_into_start_bytes_end_to_end() {
    use crate::transport::dispatch_envelope::{
        decode_dispatch_envelope, encode_dispatch_envelope, DISPATCH_ENVELOPE_SCHEMA_VERSION,
    };

    let registry = default_registry();
    // The app-facing typed builder would stamp the namespace + the typed payload
    // into the envelope; here we drive the same primitives directly.
    let action = PublishAction::PublishRaw {
        kind: 1,
        tags: vec![],
        content: "typed-bytes round trip".to_string(),
        target: PublishTarget::Auto,
        signer: Default::default(),
    };
    let payload = action.encode();
    let envelope = encode_dispatch_envelope(
        "corr-e2e",
        "nmp.publish",
        DISPATCH_ENVELOPE_SCHEMA_VERSION,
        &payload,
    );

    // S2: decode the open envelope (transport never peeks the opaque payload).
    let decoded = decode_dispatch_envelope(&envelope).expect("S2 envelope decodes");
    assert_eq!(decoded.action_namespace, "nmp.publish");
    assert_eq!(decoded.payload, payload, "opaque payload carried verbatim");

    // S3: route the opaque payload by namespace into the registry's typed
    // doorway — typed decode (incl. the schema_version gate) → start() →
    // execute() → ActorCommand.
    let id = registry
        .start_bytes(
            &mut ctx(),
            1_700_000_000_000,
            &decoded.action_namespace,
            &decoded.payload,
        )
        .expect("S3 typed start_bytes accepts the routed payload");
    assert_eq!(id.len(), 32);

    use crate::actor::ActorCommand;
    use std::cell::RefCell;
    let sent: RefCell<Vec<ActorCommand>> = RefCell::new(Vec::new());
    registry
        .execute_bytes(
            &ctx(),
            &decoded.action_namespace,
            &decoded.payload,
            &id,
            &|cmd| sent.borrow_mut().push(cmd),
        )
        .expect("S3 execute_bytes enqueues");
    assert_eq!(
        sent.into_inner().len(),
        1,
        "exactly one ActorCommand enqueued"
    );
}

#[test]
fn start_bytes_publish_raw_returns_minted_correlation_id() {
    let registry = default_registry();
    let action = PublishAction::PublishRaw {
        kind: 1,
        tags: vec![],
        content: "hello typed".to_string(),
        target: PublishTarget::Auto,
        signer: Default::default(),
    };
    let payload = action.encode();
    let id = registry
        .start_bytes(&mut ctx(), 1_700_000_000_000, "nmp.publish", &payload)
        .expect("typed publish raw should be accepted");
    assert_eq!(id.len(), 32, "minted correlation_id is 32 hex");
    assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn start_bytes_presigned_publish_payload_is_rejected() {
    let registry = default_registry();
    let action = PublishAction::Publish {
        handle: "h".to_string(),
        event: nmp_signer_iface::SignedEvent {
            id: "a".repeat(64),
            sig: "b".repeat(128),
            unsigned: nmp_signer_iface::UnsignedEvent {
                pubkey: "c".repeat(64),
                kind: 1,
                tags: vec![],
                content: "typed-bytes presigned reject".to_string(),
                created_at: 1_700_000_000,
            },
        },
        target: PublishTarget::explicit(
            vec!["wss://relay.example".to_string()],
            crate::publish::PublishRouteClass::ImportedOrPresigned,
        ),
    };
    let err = registry
        .start_bytes(
            &mut ctx(),
            1_700_000_000_000,
            "nmp.publish",
            &action.encode(),
        )
        .expect_err("typed pre-signed publish payload must be rejected");
    assert!(
        matches!(err, ActionRejection::Invalid(ref msg) if msg.contains("missing NPUB")),
        "empty pre-signed payload should fail at typed decode; got: {err:?}"
    );
}

#[test]
fn execute_bytes_presigned_publish_payload_is_rejected() {
    let registry = default_registry();
    let action = PublishAction::Publish {
        handle: "h".to_string(),
        event: nmp_signer_iface::SignedEvent {
            id: "a".repeat(64),
            sig: "b".repeat(128),
            unsigned: nmp_signer_iface::UnsignedEvent {
                pubkey: "c".repeat(64),
                kind: 1,
                tags: vec![],
                content: "typed-bytes presigned reject".to_string(),
                created_at: 1_700_000_000,
            },
        },
        target: PublishTarget::explicit(
            vec!["wss://relay.example".to_string()],
            crate::publish::PublishRouteClass::ImportedOrPresigned,
        ),
    };
    let err = registry
        .execute_bytes(
            &ctx(),
            "nmp.publish",
            &action.encode(),
            "corr-typed-1",
            &|_| {},
        )
        .expect_err("typed pre-signed publish payload must be rejected");
    assert!(
        err.message.contains("missing NPUB"),
        "empty pre-signed payload should fail at typed decode; got: {err:?}"
    );
}

// ---- Fail CLOSED: schema_version trip rejected BEFORE start() ----------------

#[test]
fn start_bytes_rejects_wrong_schema_version_before_start() {
    let registry = default_registry();
    // Build a publish payload buffer carrying a bogus schema_version.
    let bytes = bad_version_publish_payload();
    let err = registry
        .start_bytes(&mut ctx(), 1_700_000_000_000, "nmp.publish", &bytes)
        .expect_err("a wrong schema_version must be rejected before start()");
    match err {
        ActionRejection::Invalid(msg) => assert!(
            msg.contains("schema_version mismatch"),
            "rejection should name the version trip: {msg}"
        ),
        other => panic!("expected Invalid, got {other:?}"),
    }
}

/// A finished `nmp.publish` payload buffer whose `schema_version` is 999 — the
/// fail-closed tripwire must reject it before the body is decoded / `start` runs.
fn bad_version_publish_payload() -> Vec<u8> {
    crate::publish::wire::encode_with_schema_version_for_test(999)
}

#[test]
fn start_bytes_rejects_malformed_payload() {
    let registry = default_registry();
    let err = registry
        .start_bytes(
            &mut ctx(),
            1_700_000_000_000,
            "nmp.publish",
            b"not a flatbuffer",
        )
        .expect_err("malformed payload must be rejected");
    assert!(matches!(err, ActionRejection::Invalid(_)));
}

#[test]
fn start_bytes_unknown_namespace_is_rejected() {
    let registry = default_registry();
    let err = registry
        .start_bytes(
            &mut ctx(),
            1_700_000_000_000,
            "nmp.nope",
            b"\x00\x00\x00\x00",
        )
        .expect_err("unknown namespace rejected");
    match err {
        ActionRejection::Invalid(msg) => assert!(msg.contains("unknown action namespace")),
        other => panic!("expected Invalid, got {other:?}"),
    }
}

// ---- Fail CLOSED: a non-migrated module is not typed-capable -----------------

struct JsonOnlyModule;
impl ActionModule for JsonOnlyModule {
    const NAMESPACE: &'static str = "nmp.test.json_only";
    type Action = serde_json::Value;
    fn execute(
        &self,
        _ctx: &ActionContext,
        _action: Self::Action,
        _correlation_id: &str,
        _send: &dyn Fn(crate::actor::ActorCommand),
    ) -> Result<(), String> {
        Ok(())
    }
}

#[test]
fn start_bytes_rejects_not_typed_capable_module() {
    // A module that left `decode_payload` defaulted (serde_json::Value Action,
    // cannot implement ActionPayload) is rejected by the typed doorway — it does
    // NOT silently fall back to JSON.
    let mut registry = ActionRegistry::new();
    let _ = registry.register(JsonOnlyModule);
    let err = registry
        .start_bytes(
            &mut ctx(),
            1_700_000_000_000,
            "nmp.test.json_only",
            b"anything",
        )
        .expect_err("a non-typed-capable module must reject typed bytes");
    match err {
        ActionRejection::Invalid(msg) => assert!(
            msg.contains("does not support typed FlatBuffers"),
            "got: {msg}"
        ),
        other => panic!("expected Invalid, got {other:?}"),
    }
}

#[test]
fn execute_bytes_unknown_namespace_reports_no_executor() {
    let registry = ActionRegistry::new();
    let failure = registry
        .execute_bytes(&ctx(), "nmp.nope", b"x", "c", &|_| {})
        .expect_err("unknown namespace has no executor");
    assert_eq!(failure.kind, ActionFailureKind::NoExecutor);
    assert!(!failure.enqueued);
}

// ─── Typed-only byte-doorway gate (ADR-0064 / #1756) ─────────────────────────
//
// Beyond the per-dispatch fail-closed negatives above, the registry exposes the
// invariant intrinsically: `untyped_namespaces()` lists every registered module
// that left `decode_payload` defaulted (rejected `NotTypedCapable` by the byte
// doorway). The kernel-slice gate asserts the kernel's `default_registry` has
// ZERO untyped modules; the full production composition is gated in
// `nmp-defaults`. This prevents re-introducing the reverted opaque-passthrough /
// JSON-compat shim (#1828).

/// THE production gate (kernel slice): the registry the kernel ships with must
/// have ZERO untyped namespaces — every module reachable through the byte
/// doorway decodes a typed FlatBuffers payload. If a future change registers a
/// JSON-only module under `default_registry`, this goes red.
#[test]
fn default_registry_has_no_untyped_namespaces() {
    let untyped = default_registry().untyped_namespaces();
    assert!(
        untyped.is_empty(),
        "every module reachable via the byte doorway must be typed (override \
         `decode_payload`); untyped (JSON-only) namespaces are a doctrine \
         violation (ADR-0064 / #1756 — no JSON-compat shim): {untyped:?}"
    );
}

/// LOAD-BEARING negative: register the same JSON-only module the fail-closed
/// `start_bytes` test above uses (`nmp.test.json_only`, no `decode_payload`) and
/// prove the gate FLAGS it. If `untyped_namespaces()` (or the underlying
/// `is_typed_capable` probe) stopped distinguishing typed from JSON-only
/// modules, this goes green-when-it-should-be-red and the production gate above
/// would be vacuous.
#[test]
fn untyped_namespaces_flags_a_json_only_module() {
    let mut registry = ActionRegistry::new();
    let _ = registry.register(crate::publish::PublishModule); // typed — must NOT be flagged
    let _ = registry.register(JsonOnlyModule); // JSON-only — MUST be flagged
    assert_eq!(
        registry.untyped_namespaces(),
        vec!["nmp.test.json_only".to_string()],
        "the gate must flag exactly the JSON-only module's namespace and leave \
         the typed `nmp.publish` module unflagged"
    );
}

/// The canonical typed protocol module is NOT flagged — the probe does not
/// false-positive on a real typed module.
#[test]
fn untyped_namespaces_does_not_flag_the_typed_publish_module() {
    let mut registry = ActionRegistry::new();
    let _ = registry.register(crate::publish::PublishModule);
    assert!(
        registry.untyped_namespaces().is_empty(),
        "the typed `nmp.publish` module must be recognised as typed-capable"
    );
}
