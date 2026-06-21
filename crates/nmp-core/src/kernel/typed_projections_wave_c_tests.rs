//! End-to-end proof for the Wave C identity cluster Tier-2 typed projection
//! sidecars (`accounts` / `active_account` / `profile`) — the kernel-owned
//! built-in counterpart to the host-registered Tier-1 typed projections
//! (ADR-0037).
//!
//! Split out of `typed_projections_tests.rs` to keep both files under the
//! AGENTS.md 500-LOC hard cap. The bar is identical: each built-in typed
//! projection must appear in the `typed_projections` sidecar of the frame
//! `make_update` actually emits — decoded back to its typed struct — IN ADDITION
//! to its existing generic `Value` entry under the SAME key.
//!
//! V-112 (ADR-0042): `author_view` / `thread_view` deleted from typed sidecars.
//! ADR-0063 Lane H: `mention_profiles` / `claimed_profiles` / `resolved_profiles`
//! deleted from typed sidecars (replaced by refs.profile KPRF row-delta sidecar).
//! The `profile_cluster_builtins_emit_typed_sidecars_alongside_json` test is
//! retained for `claimed_events` only; the old profile-cluster assertions are gone.

use super::typed_projections::{
    decode_accounts, decode_active_account, decode_claimed_events, decode_profile,
    ACCOUNTS_FILE_IDENTIFIER, ACCOUNTS_SCHEMA_ID, ACCOUNTS_SCHEMA_VERSION,
    ACTIVE_ACCOUNT_FILE_IDENTIFIER, ACTIVE_ACCOUNT_SCHEMA_ID, ACTIVE_ACCOUNT_SCHEMA_VERSION,
    CLAIMED_EVENTS_FILE_IDENTIFIER, CLAIMED_EVENTS_SCHEMA_ID,
    PROFILE_FILE_IDENTIFIER, PROFILE_SCHEMA_ID, PROFILE_SCHEMA_VERSION,
};
use super::*;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::update_envelope::TypedProjectionData;

/// Local copy of the sidecar lookup helper (the sibling test module's copy is
/// private to that file).
fn typed_entry<'a>(typed: &'a [TypedProjectionData], key: &str) -> &'a TypedProjectionData {
    typed
        .iter()
        .find(|t| t.key == key)
        .unwrap_or_else(|| panic!("typed sidecar must carry a `{key}` entry; got {typed:?}"))
}

/// Wave C identity cluster: the three unconditional built-ins (`accounts` /
/// `active_account` / `profile`) land in the `typed_projections` sidecar of the
/// emitted frame, decode back to their typed structs, AND keep their generic
/// `Value` entries (additivity). A fresh kernel has no active account, so this
/// also exercises the `active_account == null` / placeholder-`profile` paths
/// through the real frame.
#[test]
fn identity_builtins_emit_typed_sidecars_alongside_json() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let (value, typed) = kernel.make_update_typed_for_test(true);

    let projections = value
        .get("projections")
        .and_then(serde_json::Value::as_object)
        .expect("snapshot must carry a projections object");

    // --- accounts -----------------------------------------------------------
    let json_accounts = projections
        .get("accounts")
        .and_then(serde_json::Value::as_array)
        .expect("the generic JSON `accounts` entry must remain (additive)");
    let acc = typed_entry(&typed, "accounts");
    assert_eq!(acc.schema_id, ACCOUNTS_SCHEMA_ID);
    assert_eq!(acc.schema_version, ACCOUNTS_SCHEMA_VERSION);
    assert_eq!(acc.file_identifier.as_bytes(), ACCOUNTS_FILE_IDENTIFIER);
    let decoded_accounts = decode_accounts(&acc.payload).expect("accounts sidecar must decode");
    assert_eq!(
        decoded_accounts.accounts.len(),
        json_accounts.len(),
        "typed and JSON accounts must carry the same row count"
    );

    // --- active_account (null on a fresh kernel) ----------------------------
    assert!(
        projections.contains_key("active_account"),
        "the generic JSON `active_account` entry must remain (additive)"
    );
    let json_active = projections.get("active_account").expect("present above");
    let aa = typed_entry(&typed, "active_account");
    assert_eq!(aa.schema_id, ACTIVE_ACCOUNT_SCHEMA_ID);
    assert_eq!(aa.schema_version, ACTIVE_ACCOUNT_SCHEMA_VERSION);
    assert_eq!(
        aa.file_identifier.as_bytes(),
        ACTIVE_ACCOUNT_FILE_IDENTIFIER
    );
    let decoded_active =
        decode_active_account(&aa.payload).expect("active_account sidecar must decode");
    // JSON `null` (no active account) must mirror typed `pubkey == None`.
    assert_eq!(
        decoded_active.pubkey.is_none(),
        json_active.is_null(),
        "typed `has_active_account` must mirror JSON null-ness of active_account"
    );

    // --- profile (placeholder card, all Options null) -----------------------
    let json_profile = projections
        .get("profile")
        .and_then(serde_json::Value::as_object)
        .expect("the generic JSON `profile` entry must remain (additive)");
    let pr = typed_entry(&typed, "profile");
    assert_eq!(pr.schema_id, PROFILE_SCHEMA_ID);
    assert_eq!(pr.schema_version, PROFILE_SCHEMA_VERSION);
    assert_eq!(pr.file_identifier.as_bytes(), PROFILE_FILE_IDENTIFIER);
    let decoded_profile = decode_profile(&pr.payload).expect("profile sidecar must decode");
    // `ProfileCard` has no serde skip — every Option is `null`-when-`None` (key
    // present); the typed `has_*` flag must mirror that null-ness.
    assert_eq!(
        decoded_profile.pubkey.as_str(),
        json_profile
            .get("pubkey")
            .and_then(serde_json::Value::as_str)
            .expect("profile JSON must carry pubkey"),
        "typed and JSON profile.pubkey must agree"
    );
    assert_eq!(
        decoded_profile.display_name.is_none(),
        json_profile
            .get("display_name")
            .map(serde_json::Value::is_null)
            .unwrap_or(true),
        "typed profile.display_name presence must mirror JSON null-ness"
    );
    // D1 (#606): `has_profile` render-gate removed. The card carries no
    // kernel-computed "relay data arrived" boolean on the projection boundary;
    // the JSON snapshot must not carry the key at all.
    assert!(
        json_profile.get("has_profile").is_none(),
        "profile JSON must NOT carry the removed `has_profile` render-gate field"
    );
}

/// Wave C event-cluster: `claimed_events` lands in the `typed_projections`
/// sidecar of the emitted frame, decodes back to its typed struct, AND keeps its
/// generic `Value` entry (additivity). Empty map on a fresh kernel.
///
/// ADR-0063 Lane H: mention_profiles / claimed_profiles / resolved_profiles
/// assertions removed — those projections are deleted. The refs.profile KPRF
/// row-delta sidecar is tested in the refs integration tests instead.
#[test]
fn event_cluster_builtin_emits_typed_sidecar_alongside_json() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let (value, typed) = kernel.make_update_typed_for_test(true);

    let projections = value
        .get("projections")
        .and_then(serde_json::Value::as_object)
        .expect("snapshot must carry a projections object");

    // --- claimed_events (empty on fresh kernel) ---------------------------------
    let ce_json = projections
        .get("claimed_events")
        .and_then(serde_json::Value::as_object)
        .unwrap_or_else(|| panic!("the generic JSON `claimed_events` entry must remain (additive)"))
        .len();
    let ce = typed_entry(&typed, "claimed_events");
    assert_eq!(ce.schema_id, CLAIMED_EVENTS_SCHEMA_ID);
    assert_eq!(ce.file_identifier.as_bytes(), CLAIMED_EVENTS_FILE_IDENTIFIER);
    let ce_decoded =
        decode_claimed_events(&ce.payload).expect("claimed_events sidecar must decode");
    assert_eq!(
        ce_decoded.entries.len(),
        ce_json,
        "typed and JSON claimed_events must carry the same entry count"
    );

    // ADR-0063 Lane H: mention_profiles / claimed_profiles / resolved_profiles
    // are no longer emitted by the kernel. Assert they are absent from the
    // typed sidecar AND from the JSON projections.
    let absent_keys = ["mention_profiles", "claimed_profiles", "resolved_profiles"];
    for key in &absent_keys {
        assert!(
            typed.iter().all(|t| &t.key != key),
            "typed sidecar must NOT carry deleted projection `{key}` (ADR-0063 Lane H)"
        );
        assert!(
            projections.get(*key).is_none(),
            "JSON projections must NOT carry deleted projection `{key}` (ADR-0063 Lane H)"
        );
    }
}
