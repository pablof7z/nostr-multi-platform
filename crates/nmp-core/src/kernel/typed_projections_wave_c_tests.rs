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
//! The `view_builtins_emit_only_when_their_views_are_open` test was removed
//! because `Kernel::open_author()` / `Kernel::open_thread()` were deleted
//! as part of the ADR-0042 M2 migration. The D5 optionality property is now
//! covered at the FFI layer via `nmp_app_open_interest` / `nmp_app_open_uri`.

use super::typed_projections::{
    decode_accounts, decode_active_account, decode_claimed_events, decode_claimed_profiles,
    decode_mention_profiles, decode_profile, decode_resolved_profiles,
    ACCOUNTS_FILE_IDENTIFIER, ACCOUNTS_SCHEMA_ID, ACCOUNTS_SCHEMA_VERSION,
    ACTIVE_ACCOUNT_FILE_IDENTIFIER, ACTIVE_ACCOUNT_SCHEMA_ID, ACTIVE_ACCOUNT_SCHEMA_VERSION,
    CLAIMED_EVENTS_FILE_IDENTIFIER, CLAIMED_EVENTS_SCHEMA_ID, CLAIMED_PROFILES_FILE_IDENTIFIER,
    CLAIMED_PROFILES_SCHEMA_ID, MENTION_PROFILES_FILE_IDENTIFIER, MENTION_PROFILES_SCHEMA_ID,
    PROFILE_FILE_IDENTIFIER, PROFILE_SCHEMA_ID, PROFILE_SCHEMA_VERSION,
    RESOLVED_PROFILES_FILE_IDENTIFIER, RESOLVED_PROFILES_SCHEMA_ID,
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

/// Wave C profile/event cluster: all four map-shaped built-ins
/// (`mention_profiles` / `claimed_profiles` / `claimed_events` /
/// `resolved_profiles`) land in the `typed_projections` sidecar of the emitted
/// frame, decode back to their typed structs, AND keep their generic `Value`
/// entries (additivity). All four are unconditional (`{}` when empty), so they
/// appear even on a fresh kernel; this test additionally claims a profile so the
/// `claimed_profiles` / `resolved_profiles` maps are non-empty and the per-entry
/// key + count parity is exercised against the real frame.
#[test]
fn profile_cluster_builtins_emit_typed_sidecars_alongside_json() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    // Claim a profile so `claimed_profiles` (and therefore `resolved_profiles`)
    // carries a placeholder card — exercises the populated map path. No kind:0 is
    // ingested, so every ProfileCard Option is `null`/`None` (placeholder).
    let claimed_pubkey = "ab".repeat(32);
    let _ = kernel.claim_profile(claimed_pubkey.clone(), "view-0".to_string(), true, false);

    let (value, typed) = kernel.make_update_typed_for_test(true);
    let projections = value
        .get("projections")
        .and_then(serde_json::Value::as_object)
        .expect("snapshot must carry a projections object");

    // Helper: assert a map-shaped projection's JSON object survives (additive),
    // its typed sidecar is present with the right ids, and the typed entry count
    // matches the JSON object's key count.
    let json_map_len = |key: &str| -> usize {
        projections
            .get(key)
            .and_then(serde_json::Value::as_object)
            .unwrap_or_else(|| panic!("the generic JSON `{key}` entry must remain (additive)"))
            .len()
    };

    // --- mention_profiles (empty here; no view open) ------------------------
    let mp_json = json_map_len("mention_profiles");
    let mp = typed_entry(&typed, "mention_profiles");
    assert_eq!(mp.schema_id, MENTION_PROFILES_SCHEMA_ID);
    assert_eq!(
        mp.file_identifier.as_bytes(),
        MENTION_PROFILES_FILE_IDENTIFIER
    );
    let mp_decoded =
        decode_mention_profiles(&mp.payload).expect("mention_profiles sidecar must decode");
    assert_eq!(
        mp_decoded.entries.len(),
        mp_json,
        "typed and JSON mention_profiles must carry the same entry count"
    );

    // --- claimed_profiles (non-empty: one claimed placeholder card) ---------
    let cp_json = json_map_len("claimed_profiles");
    assert_eq!(cp_json, 1, "precondition: exactly one claimed profile");
    let cp = typed_entry(&typed, "claimed_profiles");
    assert_eq!(cp.schema_id, CLAIMED_PROFILES_SCHEMA_ID);
    assert_eq!(
        cp.file_identifier.as_bytes(),
        CLAIMED_PROFILES_FILE_IDENTIFIER
    );
    let cp_decoded =
        decode_claimed_profiles(&cp.payload).expect("claimed_profiles sidecar must decode");
    assert_eq!(cp_decoded.entries.len(), cp_json);
    assert_eq!(
        cp_decoded.entries[0].0, claimed_pubkey,
        "typed claimed_profiles key must equal the claimed pubkey"
    );
    // D1 (#606): a placeholder card (no kind:0 ingested) decodes and renders
    // from its raw optional fields alone — there is NO `has_profile` render-gate
    // boolean. Every display Option is `None`; consumers pick their own
    // fallback (abbreviated pubkey etc.) without a "loaded" flag.
    let card = &cp_decoded.entries[0].1;
    assert_eq!(card.pubkey, claimed_pubkey);
    assert_eq!(card.display_name, None);
    assert_eq!(card.picture_url, None);
    assert_eq!(card.lnurl, None);

    // --- claimed_events (empty here) ----------------------------------------
    let ce_json = json_map_len("claimed_events");
    let ce = typed_entry(&typed, "claimed_events");
    assert_eq!(ce.schema_id, CLAIMED_EVENTS_SCHEMA_ID);
    assert_eq!(
        ce.file_identifier.as_bytes(),
        CLAIMED_EVENTS_FILE_IDENTIFIER
    );
    let ce_decoded =
        decode_claimed_events(&ce.payload).expect("claimed_events sidecar must decode");
    assert_eq!(ce_decoded.entries.len(), ce_json);

    // --- resolved_profiles (non-empty: the claimed card, highest precedence) -
    let rp_json = json_map_len("resolved_profiles");
    assert_eq!(rp_json, 1, "resolved_profiles must carry the claimed card");
    let rp = typed_entry(&typed, "resolved_profiles");
    assert_eq!(rp.schema_id, RESOLVED_PROFILES_SCHEMA_ID);
    assert_eq!(
        rp.file_identifier.as_bytes(),
        RESOLVED_PROFILES_FILE_IDENTIFIER
    );
    let rp_decoded =
        decode_resolved_profiles(&rp.payload).expect("resolved_profiles sidecar must decode");
    assert_eq!(rp_decoded.entries.len(), rp_json);
    assert_eq!(
        rp_decoded.entries[0].0, claimed_pubkey,
        "resolved_profiles must carry the claimed pubkey (claimed_profiles precedence)"
    );
}
