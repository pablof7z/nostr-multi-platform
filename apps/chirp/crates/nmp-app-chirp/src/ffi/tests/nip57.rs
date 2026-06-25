//! NIP-57 zap dispatch proof, plus the Wave A typed-`"nmp.nip57.zaps"`-sidecar
//! proof (ADR-0037): the typed projection closure produces a
//! `TypedProjectionData` whose `payload` decodes back to the same
//! `ZapsAggregateSnapshot` via the generated `NZAP` bindings.

use nmp_ffi::{nmp_app_free, nmp_app_new};

use super::super::nmp_app_chirp_unregister;
use super::super::register::zaps_typed_projection;
use super::helpers::{dispatch, register_app};

/// `nmp.nip57.zap` action — `ZapAction`, an `ActionModule` living in the
/// `nmp-nip57` protocol crate — is reachable through the typed byte doorway
/// (ADR-0064 / Cut-B, #1756). A well-formed `ZapInput` yields an echoed
/// host-supplied `correlation_id` (both the typed module validator AND the
/// executor are wired); a malformed body is rejected with `error`.
///
/// This is the migration proof that ADR-0024's minimum-viable LNURL path
/// (no `HttpCapability` substrate) is live end-to-end: dispatch reaches
/// `ZapAction::execute`, which builds the unsigned kind:9734 and enqueues
/// `ActorCommand::Protocol(FetchLnurlInvoiceCommand{...})` (V-41) for the
/// actor's `Protocol(...)` arm to drive. The protocol command signs on
/// the actor thread and spawns a worker for the HTTP round-trip. The
/// test asserts only the dispatch half (correlation_id minted, executor
/// returned `Ok`); the HTTP round-trip itself requires a live LN provider
/// and is exercised end-to-end through the iOS shell.
#[test]
fn nip57_zap_dispatches_through_action_registry() {
    let app = nmp_app_new();
    let handle = register_app(app);

    let recipient = "bb11223344556677889900aabbccddeeff00112233445566778899aabbccddff";
    let body = format!(
        r#"{{"recipient_pubkey":"{recipient}","amount_msats":21000,"lnurl":"alice@walletofsatoshi.com","relays":["wss://relay.damus.io"]}}"#
    );
    let parsed = dispatch(app, "nmp.nip57.zap", &body);
    let id = parsed
        .get("correlation_id")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("expected correlation_id, got {parsed}"));
    // ADR-0064 / Cut-B (#1756): the byte doorway echoes the host-supplied id.
    assert!(
        !id.is_empty(),
        "byte doorway must echo a non-empty correlation id"
    );

    // A zap to a profile (no target_event_id) is well-formed.
    let body_profile = format!(
        r#"{{"recipient_pubkey":"{recipient}","amount_msats":1000,"lnurl":"https://example.com/.well-known/lnurlp/bob","relays":["wss://relay.damus.io"]}}"#
    );
    let parsed = dispatch(app, "nmp.nip57.zap", &body_profile);
    assert!(
        parsed.get("correlation_id").is_some(),
        "profile-zap (no target) must dispatch cleanly: {parsed}"
    );

    // Zero amount is rejected by the typed validator (D6).
    let bad = format!(
        r#"{{"recipient_pubkey":"{recipient}","amount_msats":0,"lnurl":"alice@walletofsatoshi.com","relays":["wss://relay.damus.io"]}}"#
    );
    let parsed = dispatch(app, "nmp.nip57.zap", &bad);
    assert!(
        parsed.get("error").is_some(),
        "zero-amount zap must be rejected: {parsed}"
    );

    // Empty lnurl is rejected — NIP-57 has no destination without it.
    let no_lnurl = format!(
        r#"{{"recipient_pubkey":"{recipient}","amount_msats":21000,"lnurl":"","relays":["wss://relay.damus.io"]}}"#
    );
    let parsed = dispatch(app, "nmp.nip57.zap", &no_lnurl);
    assert!(
        parsed.get("error").is_some(),
        "empty-lnurl zap must be rejected: {parsed}"
    );

    nmp_app_chirp_unregister(handle);
    nmp_app_free(app);
}

/// Wave A proof: the `"nmp.nip57.zaps"` typed projection produces a
/// typed-sidecar entry (`TypedProjectionData`) whose `payload` decodes back to
/// the same `ZapsAggregateSnapshot` via the generated `NZAP` bindings.
///
/// `zaps_typed_projection` returns exactly the `TypedProjectionData` the
/// kernel's `SnapshotRegistry::run_typed` collects into a frame's
/// `typed_projections` sidecar (proven end-to-end in
/// `nmp-core/src/kernel/snapshot_registry_tests.rs`); driving it directly is the
/// in-crate proof that the `"nmp.nip57.zaps"` closure wires the right schema
/// identity and payload — without spinning the actor.
#[test]
fn zaps_typed_projection_lands_in_the_sidecar_and_round_trips() {
    use nmp_core::substrate::KernelEvent;
    use nmp_core::KernelEventObserver;
    use nmp_nip57::{decode_zaps_snapshot, ZapsAggregateProjection};

    let proj = ZapsAggregateProjection::new();

    // Drive one well-formed kind:9735 receipt through the observer so the
    // snapshot is non-empty (the `bolt11` HRP encodes the msats, matching the
    // crate's own decoder tests).
    let target = "ee".repeat(32);
    proj.on_kernel_event(&KernelEvent {
        id: "rcpt-1".to_string(),
        author: "lnprovider".to_string(),
        kind: 9735,
        created_at: 0,
        tags: vec![
            vec!["p".to_string(), "recipient".to_string()],
            vec!["e".to_string(), target.clone()],
            vec!["bolt11".to_string(), "lnbc210n1pvj...".to_string()],
        ],
        content: String::new(),
        relay_provenance: Vec::new(),
    });

    let entry = zaps_typed_projection(&proj).expect("zaps projection must always emit");

    // Schema identity the host's NZAP decoder keys off.
    assert_eq!(entry.key, "nmp.nip57.zaps");
    assert_eq!(entry.schema_id, nmp_nip57::ZAPS_SCHEMA_ID);
    assert_eq!(entry.schema_id, "nmp.nip57.zaps");
    assert_eq!(entry.schema_version, nmp_nip57::ZAPS_SCHEMA_VERSION);
    assert_eq!(entry.file_identifier, "NZAP");
    assert!(
        !entry.payload.is_empty(),
        "the typed sidecar payload must carry the encoded snapshot bytes"
    );

    // The bytes in the sidecar decode back to the same snapshot the projection
    // reports — not only the generic `payload:Value` tree.
    let decoded =
        decode_zaps_snapshot(&entry.payload).expect("sidecar payload must decode as NZAP");
    assert_eq!(decoded, proj.snapshot());
    assert!(
        decoded.totals.contains_key(&target),
        "the zapped target must survive into the typed sidecar"
    );
}
