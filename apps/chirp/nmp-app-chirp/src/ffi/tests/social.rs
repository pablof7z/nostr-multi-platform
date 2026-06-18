//! Social-verb migration proof: react / unreact / follow / unfollow are reachable
//! through the generic `dispatch_action` path after `nmp_app_chirp_register`.
//!
//! Plus the Wave A typed-`"nmp.follow_list"`-sidecar proof (ADR-0037): the
//! typed projection closure produces a `TypedProjectionData` whose `payload`
//! decodes back to the same follow-list snapshot via the generated `NF02`
//! bindings — under the `"nmp.follow_list"` key but the distinct
//! `"nmp.nip02.follow_list"` schema_id.

use nmp_ffi::{nmp_app_free, nmp_app_new};

use super::super::nmp_app_chirp_unregister;
use super::super::register::follow_list_typed_projection;
use super::helpers::{dispatch, register_app};

/// THE MIGRATION PROOF: after `nmp_app_chirp_register`, the public social
/// verbs are reachable through the generic `dispatch_action` path — each
/// returns a 32-hex `correlation_id`, proving BOTH the host-registered
/// module (consumed by `start()`) AND executor (consumed by `execute()`)
/// are wired. This replaces the deleted per-verb `nmp_app_react` /
/// `nmp_app_follow` / `nmp_app_unfollow` C symbols (D0).
#[test]
fn social_verbs_dispatch_through_action_registry() {
    let app = nmp_app_new();
    let handle = register_app(app);
    let event_id = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    for (namespace, body) in [
        (
            "nmp.nip25.react",
            r#"{"target_event_id":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","reaction":"+"}"#,
        ),
        (
            "nmp.nip25.unreact",
            r#"{"reaction_event_id":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#,
        ),
        ("nmp.follow", r#"{"pubkey":"deadbeef"}"#),
        ("nmp.unfollow", r#"{"pubkey":"deadbeef"}"#),
    ] {
        let parsed = dispatch(app, namespace, body);
        let id = parsed
            .get("correlation_id")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| panic!("{namespace}: expected correlation_id, got {parsed}"));
        assert_eq!(id.len(), 32, "{namespace}: correlation id should be 32 hex");
    }

    // `nmp.nip25.react` defaults `reaction` to `"+"` when absent.
    let parsed = dispatch(
        app,
        "nmp.nip25.react",
        &format!(r#"{{"target_event_id":"{event_id}"}}"#),
    );
    assert!(
        parsed.get("correlation_id").is_some(),
        "nmp.nip25.react without reaction should default and succeed: {parsed}"
    );

    // Malformed JSON shape is rejected by the host module validator (D6).
    let parsed = dispatch(app, "nmp.follow", r#"{"not_pubkey":"x"}"#);
    assert!(
        parsed.get("error").is_some(),
        "wrong-shape nmp.follow must be rejected: {parsed}"
    );

    nmp_app_chirp_unregister(handle);
    nmp_app_free(app);
}

/// Wave A proof: the `"nmp.follow_list"` typed projection produces a
/// typed-sidecar entry (`TypedProjectionData`) whose `payload` decodes back to
/// the same follow-list snapshot via the generated `NF02` bindings.
///
/// `follow_list_typed_projection` returns exactly the `TypedProjectionData` the
/// kernel's `SnapshotRegistry::run_typed` collects into a frame's
/// `typed_projections` sidecar (proven end-to-end in
/// `nmp-core/src/kernel/snapshot_registry_tests.rs`); driving it directly is the
/// in-crate proof that the closure wires the right schema identity (note the
/// key/schema_id split) and payload — without spinning the actor.
#[test]
fn follow_list_typed_projection_lands_in_the_sidecar_and_round_trips() {
    use std::sync::{Arc, Mutex};

    use nmp_core::kinds::KIND_CONTACT_LIST;
    use nmp_core::substrate::KernelEvent;
    use nmp_core::KernelEventObserver;
    use nmp_nip02::{decode_follow_list, FollowListProjection};

    let me = "aa".repeat(32);
    let followed_a = "11".repeat(32);
    let followed_b = "22".repeat(32);

    let active = Arc::new(Mutex::new(Some(me.clone())));
    let proj = FollowListProjection::new(Arc::clone(&active));

    // Drive the active account's kind:3 contact list through the observer.
    proj.on_kernel_event(&KernelEvent {
        id: "contacts-1".to_string(),
        author: me.clone(),
        kind: KIND_CONTACT_LIST,
        created_at: 0,
        tags: vec![
            vec!["p".to_string(), followed_a.clone()],
            vec!["p".to_string(), followed_b.clone()],
        ],
        content: String::new(),
        relay_provenance: Vec::new(),
    });

    let entry = follow_list_typed_projection(&proj).expect("follow-list projection always emits");

    // Schema identity — note the deliberate key/schema_id split.
    assert_eq!(entry.key, "nmp.follow_list");
    assert_eq!(entry.schema_id, nmp_nip02::FOLLOW_LIST_SCHEMA_ID);
    assert_eq!(entry.schema_id, "nmp.nip02.follow_list");
    assert_eq!(entry.schema_version, nmp_nip02::FOLLOW_LIST_SCHEMA_VERSION);
    assert_eq!(entry.file_identifier, "NF02");
    assert!(
        !entry.payload.is_empty(),
        "the typed sidecar payload must carry the encoded follow list"
    );

    // The bytes in the sidecar decode back to the same snapshot the projection
    // reports — not only the generic `payload:Value` tree.
    let decoded = decode_follow_list(&entry.payload).expect("sidecar payload must decode as NF02");
    assert_eq!(decoded, proj.snapshot());
    let pubkeys: Vec<&str> = decoded.follows.iter().map(|f| f.pubkey.as_str()).collect();
    assert_eq!(pubkeys, vec![followed_a.as_str(), followed_b.as_str()]);
}
