//! Integration test: prove the compatibility `register_actions` helper wires
//! follow/unfollow plus delegated NIP-25 reaction namespaces against a real
//! `NmpApp` and that each one round-trips through the typed byte doorway
//! `nmp_app_dispatch_action_bytes` (ADR-0064 / S4, #1996).
//!
//! This is the migration-success contract — the same shape the chirp
//! `social_verbs_dispatch_through_action_registry` test enforces, lifted
//! into the substrate crate that now owns the modules.

use std::ffi::CStr;
use std::sync::atomic::{AtomicU64, Ordering};

use nmp_core::dispatch_envelope::{DISPATCH_ENVELOPE_SCHEMA_VERSION, encode_dispatch_envelope};
use nmp_core::substrate::ActionPayload;
use nmp_ffi::{NmpApp, nmp_app_dispatch_action_bytes, nmp_app_free, nmp_app_new, nmp_free_string};

/// Mint a process-local unique host correlation id. On the byte lane the host
/// supplies the id and the doorway echoes it back verbatim (ADR-0064 §4) — it
/// is NOT a kernel-minted 32-hex id.
fn next_correlation_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    format!("nip02-test-{}", COUNTER.fetch_add(1, Ordering::Relaxed))
}

/// Wrap pre-encoded typed `payload` bytes in a `DispatchEnvelope` routed at
/// `namespace`, drive it through `nmp_app_dispatch_action_bytes`, and return the
/// parsed JSON result. The returned C string is freed. The host-supplied
/// correlation id is returned alongside so callers can assert the echo.
fn dispatch_bytes(
    app: *mut NmpApp,
    namespace: &str,
    payload: &[u8],
) -> (String, serde_json::Value) {
    let correlation_id = next_correlation_id();
    let envelope = encode_dispatch_envelope(
        &correlation_id,
        namespace,
        DISPATCH_ENVELOPE_SCHEMA_VERSION,
        payload,
    );
    let ptr = nmp_app_dispatch_action_bytes(app, envelope.as_ptr(), envelope.len());
    assert!(
        !ptr.is_null(),
        "dispatch_action_bytes must never return null"
    );
    // SAFETY: `ptr` is a valid C string from `nmp_app_dispatch_action_bytes`.
    let out = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap().to_owned();
    nmp_free_string(ptr);
    (correlation_id, serde_json::from_str(&out).unwrap())
}

/// After `nmp_nip02::register_actions`, the old public social bundle is
/// reachable through the typed byte doorway. Each accepted dispatch echoes the
/// host-supplied `correlation_id` with no `error`, proving BOTH the
/// shape-validating module (consumed by `ActionRegistry::start_bytes`) AND the
/// `ActorCommand`-enqueuing executor (consumed by `ActionRegistry::execute_bytes`)
/// are wired under each namespace.
#[test]
fn register_actions_wires_compat_social_bundle() {
    let app = nmp_app_new();
    assert!(!app.is_null(), "nmp_app_new must return a valid app");
    // SAFETY: `app` is a valid pointer from `nmp_app_new`; we hold the
    // sole `&mut` for the duration of the registration call and drop it
    // before any other access.
    unsafe {
        nmp_nip02::register_actions(&mut *app);
    }

    let event_id = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    // Build the typed payload for each routing namespace. The envelope's
    // routing namespace is what the registry uses to find the module — it MAY
    // differ from the payload type's `ActionPayload::SCHEMA_ID`; the bytes are
    // that module's typed payload encoded.
    let follow = nmp_nip02::PubkeyAction {
        pubkey: "deadbeef".to_string(),
    }
    .encode();
    let react = nmp_nip25::ReactAction {
        target_event_id: event_id.to_string(),
        reaction: "+".to_string(),
        target_author_pubkey: None,
    }
    .encode();
    let unreact = nmp_nip25::UnreactAction {
        reaction_event_id: event_id.to_string(),
        reason: String::new(),
    }
    .encode();

    for (namespace, payload) in [
        ("nmp.follow", &follow),
        ("nmp.unfollow", &follow),
        ("nmp.nip25.react", &react),
        ("nmp.nip25.unreact", &unreact),
    ] {
        let (sent_id, parsed) = dispatch_bytes(app, namespace, payload);
        assert!(
            parsed.get("error").is_none(),
            "{namespace}: must be accepted, got {parsed}"
        );
        let id = parsed
            .get("correlation_id")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| panic!("{namespace}: expected correlation_id, got {parsed}"));
        assert_eq!(
            id, sent_id,
            "{namespace}: byte doorway must echo the host-supplied correlation id"
        );
    }

    // `nmp.nip25.react` with `reaction: "+"` built directly (the serde JSON
    // default is irrelevant on the typed path) is accepted.
    let default_react = nmp_nip25::ReactAction {
        target_event_id: event_id.to_string(),
        reaction: "+".to_string(),
        target_author_pubkey: None,
    }
    .encode();
    let (_id, parsed) = dispatch_bytes(app, "nmp.nip25.react", &default_react);
    assert!(
        parsed.get("correlation_id").is_some() && parsed.get("error").is_none(),
        "nmp.nip25.react with default '+' reaction should succeed: {parsed}"
    );

    // Malformed payload bytes are rejected by the module's `decode_payload`
    // (the byte-lane equivalent of a JSON decode failure), surfaced as
    // `{"error":...}` (D6 — never a crash). nip02 `FollowModule::start` is a
    // no-op accept, so the rejection comes from the typed decode, not `start`.
    let (_id, parsed) = dispatch_bytes(app, "nmp.follow", b"not a flatbuffer payload");
    assert!(
        parsed.get("error").is_some(),
        "malformed nmp.follow payload bytes must be rejected: {parsed}"
    );

    nmp_app_free(app);
}

/// ADR-0064 / S3 (#1751) — the TYPED FlatBuffers payload doorway end-to-end:
/// `DispatchEnvelope` payload bytes → `ActionRegistry::start_bytes` → typed
/// decode → `start()`, then `execute_bytes` enqueues the `ActorCommand`. Drives
/// the nip25 + nip02 trio members this crate registers, plus the NEGATIVE
/// (bad schema_version → rejected before start).
#[test]
fn typed_bytes_dispatch_round_trips_trio_through_registry() {
    use nmp_core::__ffi_internal::ActionRegistry;
    use nmp_core::substrate::{ActionContext, ActionPayload};

    let mut registry = ActionRegistry::new();
    nmp_nip02::register_actions(&mut registry);

    // Build typed payloads with each crate's `ActionPayload::encode`, then drive
    // them through the registry's typed-bytes doorway exactly as the byte
    // transport (S2) would after decoding the envelope.
    let follow = nmp_nip02::PubkeyAction {
        pubkey: "a".repeat(64),
    }
    .encode();
    let follow_many = nmp_nip02::FollowManyAction {
        pubkeys: vec!["b".repeat(64), "c".repeat(64)],
    }
    .encode();
    let react = nmp_nip25::ReactAction {
        target_event_id: "d".repeat(64),
        reaction: "+".to_string(),
        target_author_pubkey: None,
    }
    .encode();
    let unreact = nmp_nip25::UnreactAction {
        reaction_event_id: "e".repeat(64),
        reason: String::new(),
    }
    .encode();

    for (namespace, payload) in [
        ("nmp.follow", &follow),
        ("nmp.unfollow", &follow),
        ("nmp.follow_many", &follow_many),
        ("nmp.nip25.react", &react),
        ("nmp.nip25.unreact", &unreact),
    ] {
        let id = registry
            .start_bytes(
                &mut ActionContext::default(),
                1_700_000_000_000,
                namespace,
                payload,
            )
            .unwrap_or_else(|e| panic!("{namespace}: typed start_bytes should accept: {e:?}"));
        assert_eq!(id.len(), 32, "{namespace}: minted 32-hex correlation_id");

        // execute_bytes enqueues exactly one ActorCommand.
        let count = std::cell::Cell::new(0u32);
        registry
            .execute_bytes(
                &nmp_core::substrate::ActionContext::default(),
                namespace,
                payload,
                &id,
                &|_cmd| count.set(count.get() + 1),
            )
            .unwrap_or_else(|e| panic!("{namespace}: execute_bytes should enqueue: {e:?}"));
        assert_eq!(
            count.get(),
            1,
            "{namespace}: exactly one ActorCommand enqueued"
        );
    }

    // NEGATIVE 1: a payload with a BAD schema_version is REJECTED before start().
    let bad_version = build_bad_version_follow_payload();
    let err = registry
        .start_bytes(
            &mut ActionContext::default(),
            1_700_000_000_000,
            "nmp.follow",
            &bad_version,
        )
        .expect_err("a wrong schema_version must be rejected before start (fail closed)");
    match err {
        nmp_core::substrate::ActionRejection::Invalid(msg) => assert!(
            msg.contains("schema_version mismatch"),
            "reject should name the version trip: {msg}"
        ),
        other => panic!("expected Invalid rejection, got {other:?}"),
    }

    // NEGATIVE 2: a malformed (non-FlatBuffers) payload is also rejected.
    let err = registry
        .start_bytes(
            &mut ActionContext::default(),
            1_700_000_000_000,
            "nmp.follow",
            b"not a flatbuffer payload",
        )
        .expect_err("malformed typed payload must be rejected (fail closed)");
    assert!(matches!(
        err,
        nmp_core::substrate::ActionRejection::Invalid(_)
    ));
}

/// A finished `nmp.follow` (`FollowActionPayload`) buffer whose `schema_version`
/// is 999 — the fail-closed tripwire must reject it before `start` runs.
fn build_bad_version_follow_payload() -> Vec<u8> {
    use nmp_nip02::wire::action_payload::follow_action_generated::nmp::nip_02 as fb;
    let mut fbb = flatbuffers::FlatBufferBuilder::new();
    let pubkey = fbb.create_string(&"a".repeat(64));
    let payload = fb::FollowActionPayload::create(
        &mut fbb,
        &fb::FollowActionPayloadArgs {
            schema_version: 999,
            pubkey: Some(pubkey),
        },
    );
    fb::finish_follow_action_payload_buffer(&mut fbb, payload);
    fbb.finished_data().to_vec()
}

// ---- S3 gap tests: bad-version trip for every migrated nip02 namespace -------

/// ADR-0064 / S3 (#1751) — `nmp.unfollow` uses the same `FollowActionPayload`
/// wire shape as `nmp.follow`. A bad `schema_version` MUST be rejected BEFORE
/// `start()` runs, proving the fail-closed gate covers the unfollow namespace.
#[test]
fn start_bytes_rejects_wrong_schema_version_for_unfollow() {
    use nmp_core::__ffi_internal::ActionRegistry;
    use nmp_core::substrate::{ActionContext, ActionRejection};

    let mut registry = ActionRegistry::new();
    nmp_nip02::register_actions(&mut registry);

    let bad_version = build_bad_version_unfollow_payload();
    let err = registry
        .start_bytes(
            &mut ActionContext::default(),
            1_700_000_000_000,
            "nmp.unfollow",
            &bad_version,
        )
        .expect_err("a wrong schema_version must be rejected before start() (fail closed)");
    match err {
        ActionRejection::Invalid(msg) => assert!(
            msg.contains("schema_version mismatch"),
            "rejection must name the version trip: {msg}"
        ),
        other => panic!("expected Invalid rejection, got {other:?}"),
    }
}

/// ADR-0064 / S3 (#1751) — `nmp.follow_many` uses `FollowManyActionPayload`.
/// A bad `schema_version` MUST be rejected BEFORE `start()` runs.
#[test]
fn start_bytes_rejects_wrong_schema_version_for_follow_many() {
    use nmp_core::__ffi_internal::ActionRegistry;
    use nmp_core::substrate::{ActionContext, ActionRejection};

    let mut registry = ActionRegistry::new();
    nmp_nip02::register_actions(&mut registry);

    let bad_version = build_bad_version_follow_many_payload();
    let err = registry
        .start_bytes(
            &mut ActionContext::default(),
            1_700_000_000_000,
            "nmp.follow_many",
            &bad_version,
        )
        .expect_err("a wrong schema_version must be rejected before start() (fail closed)");
    match err {
        ActionRejection::Invalid(msg) => assert!(
            msg.contains("schema_version mismatch"),
            "rejection must name the version trip: {msg}"
        ),
        other => panic!("expected Invalid rejection, got {other:?}"),
    }
}

/// `nmp.unfollow` shares the same `FollowActionPayload` wire type as `nmp.follow`.
/// Build one with `schema_version = 999` to trip the fail-closed gate.
fn build_bad_version_unfollow_payload() -> Vec<u8> {
    // Same wire type as nmp.follow — PubkeyAction encodes as FollowActionPayload.
    build_bad_version_follow_payload()
}

/// `nmp.follow_many` (`FollowManyActionPayload`) with `schema_version = 999`.
fn build_bad_version_follow_many_payload() -> Vec<u8> {
    use nmp_nip02::wire::action_payload::follow_many_action_generated::nmp::nip_02 as fb;
    let mut fbb = flatbuffers::FlatBufferBuilder::new();
    let pk = fbb.create_string(&"b".repeat(64));
    let pubkeys = fbb.create_vector(&[pk]);
    let payload = fb::FollowManyActionPayload::create(
        &mut fbb,
        &fb::FollowManyActionPayloadArgs {
            schema_version: 999,
            pubkeys: Some(pubkeys),
        },
    );
    fb::finish_follow_many_action_payload_buffer(&mut fbb, payload);
    fbb.finished_data().to_vec()
}

/// Unknown namespace is rejected by the registry — this proves the
/// registration is namespace-scoped (a host that calls `register_actions`
/// only gets the three social verbs, not a wildcard).
#[test]
fn unregistered_namespace_is_rejected_even_after_register_actions() {
    let app = nmp_app_new();
    // SAFETY: same as `register_actions_wires_all_three_social_verbs`.
    unsafe {
        nmp_nip02::register_actions(&mut *app);
    }
    let payload = nmp_nip02::PubkeyAction {
        pubkey: "deadbeef".to_string(),
    }
    .encode();
    let (_id, parsed) = dispatch_bytes(app, "nmp.nip02.not_a_real_verb", &payload);
    assert!(
        parsed.get("error").is_some(),
        "unknown namespace must surface an error: {parsed}"
    );
    nmp_app_free(app);
}
