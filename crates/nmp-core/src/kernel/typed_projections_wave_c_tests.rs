//! End-to-end proof for the Wave C identity + views-cluster Tier-2 typed
//! projection sidecars (`accounts` / `active_account` / `profile` /
//! `author_view` / `thread_view`) — the kernel-owned built-in counterpart to
//! the host-registered Tier-1 typed projections (ADR-0037).
//!
//! Split out of `typed_projections_tests.rs` to keep both files under the
//! AGENTS.md 500-LOC hard cap. The bar is identical: each built-in typed
//! projection must appear in the `typed_projections` sidecar of the frame
//! `make_update` actually emits — decoded back to its typed struct — IN ADDITION
//! to its existing generic `Value` entry under the SAME key. The two view
//! built-ins additionally prove D5 optionality: present in the sidecar EXACTLY
//! when their generic JSON key is present (absent on a fresh kernel; present
//! once the corresponding view is opened).

use super::typed_projections::{
    decode_accounts, decode_active_account, decode_author_view, decode_profile, decode_thread_view,
    ACCOUNTS_FILE_IDENTIFIER, ACCOUNTS_SCHEMA_ID, ACCOUNTS_SCHEMA_VERSION,
    ACTIVE_ACCOUNT_FILE_IDENTIFIER, ACTIVE_ACCOUNT_SCHEMA_ID, ACTIVE_ACCOUNT_SCHEMA_VERSION,
    AUTHOR_VIEW_FILE_IDENTIFIER, AUTHOR_VIEW_SCHEMA_ID, AUTHOR_VIEW_SCHEMA_VERSION,
    PROFILE_FILE_IDENTIFIER, PROFILE_SCHEMA_ID, PROFILE_SCHEMA_VERSION,
    THREAD_VIEW_FILE_IDENTIFIER, THREAD_VIEW_SCHEMA_ID, THREAD_VIEW_SCHEMA_VERSION,
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
    assert_eq!(
        decoded_profile.has_profile,
        json_profile
            .get("has_profile")
            .and_then(serde_json::Value::as_bool)
            .expect("profile JSON must carry has_profile"),
        "typed and JSON profile.has_profile must agree"
    );
}

/// Wave C views cluster (D5 optionality): the `author_view` / `thread_view`
/// typed sidecars are emitted EXACTLY when their generic JSON keys are present —
/// absent on a fresh kernel, present once the corresponding view is opened — and
/// the typed payload agrees with the JSON when present.
#[test]
fn view_builtins_emit_only_when_their_views_are_open() {
    // --- fresh kernel: both views ABSENT in JSON AND in the typed sidecar ----
    {
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        let (value, typed) = kernel.make_update_typed_for_test(true);
        let projections = value
            .get("projections")
            .and_then(serde_json::Value::as_object)
            .expect("snapshot must carry a projections object");

        assert!(
            !projections.contains_key("author_view"),
            "JSON must omit author_view when no author view is open (D5)"
        );
        assert!(
            !typed.iter().any(|t| t.key == "author_view"),
            "typed sidecar must omit author_view when JSON does (no placeholder)"
        );
        assert!(
            !projections.contains_key("thread_view"),
            "JSON must omit thread_view when no thread view is open (D5)"
        );
        assert!(
            !typed.iter().any(|t| t.key == "thread_view"),
            "typed sidecar must omit thread_view when JSON does (no placeholder)"
        );
    }

    // --- author view OPEN: author_view present in BOTH, agrees --------------
    {
        let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
        let author = "a".repeat(64);
        // Seed one kind:1 note by this author so the author view carries a
        // non-empty `items` array — this exercises the shared `timeline_item_model`
        // DTO→Model mapper with real values end-to-end (not the trivial 0 == 0
        // empty case), mirroring the seeded-relay bar in the relay/settings proof.
        let note_id = "e".repeat(64);
        kernel.events.insert(
            note_id.clone(),
            StoredEvent {
                id: note_id.clone(),
                author: author.clone(),
                kind: 1,
                created_at: 1_700_000_000,
                tags: Vec::new(),
                content: "gm from the typed sidecar".to_string(),
                relay_count: 2,
            },
        );
        let _ = kernel.open_author(
            author.clone(),
            std::collections::BTreeSet::from([1u32, 6u32]),
            true,
        );
        let (value, typed) = kernel.make_update_typed_for_test(true);
        let projections = value
            .get("projections")
            .and_then(serde_json::Value::as_object)
            .expect("snapshot must carry a projections object");

        let json_author = projections
            .get("author_view")
            .and_then(serde_json::Value::as_object)
            .expect("JSON author_view must be present once the view is open (D5)");
        let av = typed_entry(&typed, "author_view");
        assert_eq!(av.schema_id, AUTHOR_VIEW_SCHEMA_ID);
        assert_eq!(av.schema_version, AUTHOR_VIEW_SCHEMA_VERSION);
        assert_eq!(av.file_identifier.as_bytes(), AUTHOR_VIEW_FILE_IDENTIFIER);
        let decoded = decode_author_view(&av.payload).expect("author_view sidecar must decode");
        assert_eq!(
            decoded.pubkey,
            json_author
                .get("pubkey")
                .and_then(serde_json::Value::as_str)
                .expect("author_view JSON must carry pubkey"),
            "typed and JSON author_view.pubkey must agree"
        );
        assert_eq!(
            decoded.pubkey, author,
            "the opened author's pubkey survives"
        );

        // The seeded note flows through the shared `timeline_item_model` mapper;
        // assert the typed row agrees field-for-field with the JSON row (value
        // coverage for the DTO→Model mapping, not just presence).
        let json_items = json_author
            .get("items")
            .and_then(serde_json::Value::as_array)
            .expect("author_view JSON must carry an items array");
        assert_eq!(
            decoded.items.len(),
            json_items.len(),
            "typed and JSON author_view items must carry the same count"
        );
        assert_eq!(decoded.items.len(), 1, "the seeded note must appear");
        assert_eq!(
            decoded.items[0].id, note_id,
            "the typed item id is the seeded event id"
        );
        assert_eq!(
            Some(decoded.items[0].id.as_str()),
            json_items[0].get("id").and_then(serde_json::Value::as_str),
            "typed and JSON author_view.items[0].id must agree"
        );
        assert_eq!(
            decoded.items[0].content, "gm from the typed sidecar",
            "the typed item content survives the DTO→Model→bytes→Model round-trip"
        );
        assert_eq!(
            Some(decoded.items[0].content.as_str()),
            json_items[0]
                .get("content")
                .and_then(serde_json::Value::as_str),
            "typed and JSON author_view.items[0].content must agree"
        );
        // The nested `profile` card (shared `profile_card_model` mapper) carries
        // the opened author's pubkey in both forms.
        assert_eq!(
            decoded.profile.pubkey, author,
            "the nested author profile pubkey survives"
        );
        assert_eq!(
            Some(decoded.profile.pubkey.as_str()),
            json_author
                .get("profile")
                .and_then(serde_json::Value::as_object)
                .and_then(|p| p.get("pubkey"))
                .and_then(serde_json::Value::as_str),
            "typed and JSON author_view.profile.pubkey must agree"
        );

        // thread_view must still be absent (only the author view is open).
        assert!(
            !projections.contains_key("thread_view"),
            "thread_view must remain absent when only the author view is open"
        );
        assert!(!typed.iter().any(|t| t.key == "thread_view"));
    }

    // --- thread view OPEN: thread_view present in BOTH, agrees --------------
    {
        let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
        let focused = "e".repeat(64);
        let _ = kernel.open_thread(
            focused.clone(),
            std::collections::BTreeSet::from([1u32, 6u32]),
            true,
        );
        let (value, typed) = kernel.make_update_typed_for_test(true);
        let projections = value
            .get("projections")
            .and_then(serde_json::Value::as_object)
            .expect("snapshot must carry a projections object");

        let json_thread = projections
            .get("thread_view")
            .and_then(serde_json::Value::as_object)
            .expect("JSON thread_view must be present once the view is open (D5)");
        let tv = typed_entry(&typed, "thread_view");
        assert_eq!(tv.schema_id, THREAD_VIEW_SCHEMA_ID);
        assert_eq!(tv.schema_version, THREAD_VIEW_SCHEMA_VERSION);
        assert_eq!(tv.file_identifier.as_bytes(), THREAD_VIEW_FILE_IDENTIFIER);
        let decoded = decode_thread_view(&tv.payload).expect("thread_view sidecar must decode");
        assert_eq!(
            decoded.focused_event_id,
            json_thread
                .get("focused_event_id")
                .and_then(serde_json::Value::as_str)
                .expect("thread_view JSON must carry focused_event_id"),
            "typed and JSON thread_view.focused_event_id must agree"
        );
        assert_eq!(decoded.focused_event_id, focused);
        // author_view must still be absent (only the thread view is open).
        assert!(
            !projections.contains_key("author_view"),
            "author_view must remain absent when only the thread view is open"
        );
        assert!(!typed.iter().any(|t| t.key == "author_view"));
    }
}
