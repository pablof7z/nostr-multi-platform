//! Integration test: prove the compatibility `register_actions` helper wires
//! follow/unfollow plus delegated NIP-25 reaction namespaces against a real
//! `NmpApp` and that each one round-trips through `nmp_app_dispatch_action`.
//!
//! This is the migration-success contract — the same shape the chirp
//! `social_verbs_dispatch_through_action_registry` test enforces, lifted
//! into the substrate crate that now owns the modules.

use std::ffi::{CStr, CString};

use nmp_ffi::{nmp_app_dispatch_action, nmp_app_free, nmp_app_new, nmp_free_string};

/// Drive `nmp_app_dispatch_action` for `namespace`/`action_json` and return
/// the parsed JSON result. The returned C string is freed.
fn dispatch(app: *mut nmp_ffi::NmpApp, namespace: &str, action_json: &str) -> serde_json::Value {
    let ns = CString::new(namespace).unwrap();
    let body = CString::new(action_json).unwrap();
    let ptr = nmp_app_dispatch_action(app, ns.as_ptr(), body.as_ptr());
    assert!(!ptr.is_null(), "dispatch_action must never return null");
    // SAFETY: `ptr` is a valid C string from `nmp_app_dispatch_action`.
    let out = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap().to_owned();
    nmp_free_string(ptr);
    serde_json::from_str(&out).unwrap()
}

/// After `nmp_nip02::register_actions`, the old public social bundle is
/// reachable through the generic `dispatch_action` path. Each accepted
/// dispatch returns a 32-hex `correlation_id`, proving BOTH the
/// shape-validating module (consumed by `ActionRegistry::start`) AND the
/// `ActorCommand`-enqueuing executor (consumed by `ActionRegistry::execute`)
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
    for (namespace, body) in [
        ("nmp.follow", r#"{"pubkey":"deadbeef"}"#),
        ("nmp.unfollow", r#"{"pubkey":"deadbeef"}"#),
        (
            "nmp.nip25.react",
            r#"{"target_event_id":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","reaction":"+"}"#,
        ),
        (
            "nmp.nip25.unreact",
            r#"{"reaction_event_id":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#,
        ),
    ] {
        let parsed = dispatch(app, namespace, body);
        let id = parsed
            .get("correlation_id")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| panic!("{namespace}: expected correlation_id, got {parsed}"));
        assert_eq!(
            id.len(),
            32,
            "{namespace}: correlation id must be 32 hex chars"
        );
        assert!(
            id.chars().all(|c| c.is_ascii_hexdigit()),
            "{namespace}: correlation id must be lowercase hex, got {id}"
        );
    }

    // `nmp.nip25.react` accepts a body missing `reaction` (defaults to "+").
    let parsed = dispatch(
        app,
        "nmp.nip25.react",
        &format!(r#"{{"target_event_id":"{event_id}"}}"#),
    );
    assert!(
        parsed.get("correlation_id").is_some(),
        "nmp.nip25.react without `reaction` should default to '+' and succeed: {parsed}"
    );

    // Wrong-shape body is rejected by the module's shape validator (the
    // serde decoder), surfaced as `{"error":...}` (D6 — never a crash).
    let parsed = dispatch(app, "nmp.follow", r#"{"not_pubkey":"x"}"#);
    assert!(
        parsed.get("error").is_some(),
        "wrong-shape nmp.follow must be rejected: {parsed}"
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
    let follow = nmp_nip02::PubkeyAction { pubkey: "a".repeat(64) }.encode();
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
            .start_bytes(&mut ActionContext::default(), 1_700_000_000_000, namespace, payload)
            .unwrap_or_else(|e| panic!("{namespace}: typed start_bytes should accept: {e:?}"));
        assert_eq!(id.len(), 32, "{namespace}: minted 32-hex correlation_id");

        // execute_bytes enqueues exactly one ActorCommand.
        let count = std::cell::Cell::new(0u32);
        registry
            .execute_bytes(namespace, payload, &id, &|_cmd| count.set(count.get() + 1))
            .unwrap_or_else(|e| panic!("{namespace}: execute_bytes should enqueue: {e:?}"));
        assert_eq!(count.get(), 1, "{namespace}: exactly one ActorCommand enqueued");
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
    let parsed = dispatch(app, "nmp.nip02.not_a_real_verb", r#"{}"#);
    assert!(
        parsed.get("error").is_some(),
        "unknown namespace must surface an error: {parsed}"
    );
    nmp_app_free(app);
}
