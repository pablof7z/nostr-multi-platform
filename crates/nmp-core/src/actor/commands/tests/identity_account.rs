//! Tests for sign-in, create-account, switch-active, and remove-account
//! command handlers.

use super::*;

#[test]
fn sign_in_nsec_adds_active_account_and_projects_it() {
    let (mut id, mut kernel) = fresh();
    sign_in_nsec(&mut id, &mut kernel, TEST_NSEC, false);
    let (accounts, active) = kernel.account_snapshot();
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].status, "active");
    assert_eq!(accounts[0].signer_kind, "local");
    assert!(active.is_some());
    assert_eq!(active, Some(&accounts[0].id));
    assert!(accounts[0].npub.starts_with("npub1"));
}

/// aim.md §2 #4 / §4.5: native cannot derive signer-display labels with a
/// scope a "remote signers" list with a lowercased string comparison, nor
/// compute `isActive` from `status == ..`. The actor pre-classifies the
/// semantic flags on every row; the human-readable signer label is derived by
/// the shell from the raw `signer_kind` token (#1712, D7/D27).
#[test]
fn local_account_projection_carries_preclassified_signer_fields() {
    let (mut id, mut kernel) = fresh();
    sign_in_nsec(&mut id, &mut kernel, TEST_NSEC, false);
    let (accounts, _) = kernel.account_snapshot();
    let row = &accounts[0];
    assert_eq!(row.signer_kind, "local");
    assert!(!row.signer_is_remote);
    assert!(row.is_active);
}

#[test]
fn sign_in_nsec_rejects_garbage_with_toast() {
    let (mut id, mut kernel) = fresh();
    sign_in_nsec(&mut id, &mut kernel, "not-a-key", false);
    assert!(kernel.account_snapshot().0.is_empty());
    assert!(kernel
        .last_error_toast_snapshot()
        .is_some_and(|t| t.contains("invalid secret key")));
    assert_eq!(
        kernel.last_error_category_snapshot().map(String::as_str),
        Some(crate::ui_token::codes::IDENTITY_INVALID_SECRET_KEY)
    );
}

#[test]
fn create_account_generates_fresh_active_key() {
    let (mut id, mut kernel) = fresh();
    let profile = std::collections::HashMap::new();
    let relays: Vec<(String, String)> = vec![];
    create_account(
        &mut id,
        &mut kernel,
        false,
        &profile,
        &relays,
        &[],
        false,
        true,
    );
    assert_eq!(kernel.account_snapshot().0.len(), 1);
    assert!(id.active_pubkey().is_some());
}

#[test]
fn create_account_empty_relays_keeps_preconfigured_relays() {
    // New contract: `nmp-core` no longer owns a hardcoded onboarding default.
    // The app declares its relay set (via `NmpAppBuilder` /
    // `ActorCommand::Lifecycle(LifecycleCommand::Start { initial_relays })`); `create_account` only
    // overwrites `configured_relays` when the caller declares relays. With an
    // empty `relays` arg the kernel's pre-existing relay set is preserved.
    let (mut id, mut kernel) = fresh();

    // Pre-seed relays the way Start (or pre-start `add_relay`) would.
    kernel.set_configured_relays(vec![crate::kernel::AppRelay::new(
        "wss://preseed.test".to_string(),
        "both".to_string(),
    )]);

    let profile = std::collections::HashMap::new();
    let relays: Vec<(String, String)> = vec![];
    create_account(
        &mut id,
        &mut kernel,
        false,
        &profile,
        &relays,
        &[],
        false,
        true,
    );

    // The pre-seeded relay set survives — empty onboarding relays do NOT clobber it.
    let rows = kernel.configured_relays_snapshot();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].url, "wss://preseed.test");
}

#[test]
fn create_account_empty_relays_leaves_unseeded_kernel_empty() {
    // And when nothing was pre-seeded, an empty onboarding relay list leaves
    // `configured_relays` empty — there is NO implicit `nmp-core` fallback.
    let (mut id, mut kernel) = fresh();
    let profile = std::collections::HashMap::new();
    let relays: Vec<(String, String)> = vec![];
    create_account(
        &mut id,
        &mut kernel,
        false,
        &profile,
        &relays,
        &[],
        false,
        true,
    );

    assert!(
        kernel.configured_relays_snapshot().is_empty(),
        "empty onboarding relays + unseeded kernel ⇒ no relays (no hardcoded default)"
    );
}

#[test]
fn create_account_launch_override_relay_gets_rust_owned_default_role() {
    let (mut id, mut kernel) = fresh();
    let profile = std::collections::HashMap::new();
    let relays = vec![("wss://maestro.test/".to_string(), String::new())];
    create_account(
        &mut id,
        &mut kernel,
        false,
        &profile,
        &relays,
        &[],
        false,
        true,
    );

    let rows = kernel.configured_relays_snapshot();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].url, "wss://maestro.test");
    assert_eq!(rows[0].role, "both,indexer");
}

#[test]
fn create_account_publishes_bootstrap_events_and_persists_relay_rows() {
    let (mut id, mut kernel, publish_store) = fresh_with_publish_store();
    kernel.install_profile_view_seed_parser_for_test("Signup User");
    let mut profile = std::collections::HashMap::new();
    profile.insert("name".to_string(), "Signup User".to_string());
    let relays = vec![
        ("wss://SIGNUP-WRITE.test/".to_string(), "write".to_string()),
        ("wss://signup-read.test/".to_string(), "read".to_string()),
        (
            "wss://signup-indexer.test/".to_string(),
            "indexer".to_string(),
        ),
    ];
    // App-supplied seed follows (NMP no longer hardcodes them — #1493) so the
    // cold-start kind:3 is published.
    let follows = vec![
        "fa984bd7dbb282f07e16e7ae87b26a2a7b9b90b7246a44771f0cf5ae58018f52".to_string(),
        "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d".to_string(),
    ];
    let outbound = create_account(
        &mut id,
        &mut kernel,
        false,
        &profile,
        &relays,
        &follows,
        false,
        true,
    );

    assert!(
        outbound.iter().any(|msg| msg.text.contains("\"kind\":0")),
        "create_account must return the kind:0 EVENT frame for actor dispatch"
    );
    assert!(
        outbound.iter().any(|msg| msg.text.contains("\"kind\":3")),
        "create_account must return the cold-start kind:3 EVENT frame for actor dispatch"
    );

    let rows = kernel.configured_relays_snapshot();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].url, "wss://signup-write.test");
    assert_eq!(rows[0].role, "write");
    assert_eq!(rows[1].url, "wss://signup-read.test");
    assert_eq!(rows[1].role, "read");
    assert_eq!(rows[2].url, "wss://signup-indexer.test");
    assert_eq!(rows[2].role, "indexer");

    let records = publish_store
        .load_pending()
        .expect("create_account publish records");
    let mut kinds: Vec<u32> = records
        .iter()
        .map(|record| record.event.unsigned.kind)
        .collect();
    kinds.sort();
    assert_eq!(kinds, vec![0, 3]);

    let expected_targets = vec![
        "wss://signup-indexer.test".to_string(),
        "wss://signup-read.test".to_string(),
        "wss://signup-write.test".to_string(),
    ];
    for kind in [0, 3] {
        let record = record_of_kind(&records, kind);
        assert_eq!(
            target_relays(record),
            expected_targets,
            "kind:{kind} must publish to the explicit canonical cold-start relays"
        );
    }

    let metadata = record_of_kind(&records, 0);
    assert!(metadata.event.unsigned.tags.is_empty());
    assert!(metadata.event.unsigned.content.contains("Signup User"));

    let contacts = record_of_kind(&records, 3);
    assert!(
        contacts
            .event
            .unsigned
            .tags
            .iter()
            .any(|tag| tag.first().map(String::as_str) == Some("p")),
        "cold-start kind:3 must carry seed follow p-tags"
    );

    let snap: serde_json::Value =
        serde_json::from_str(&kernel.make_update_json_for_test(true)).expect("snapshot json");
    // D0: the profile card is no longer a typed `KernelSnapshot.profile` field
    // — it is a built-in entry in the `projections` map under `"profile"`.
    assert_eq!(
        snap["projections"]["profile"]["display_name"].as_str(),
        Some("Signup User"),
        "own profile must render from the local kind:0 publish intent before relay echo"
    );
    assert_eq!(
        snap["metrics"]["profile_events"].as_u64(),
        Some(1),
        "local kind:0 publish lands the own profile in the store-first read cache (single mechanism)"
    );
}

#[test]
fn create_account_next_note_routes_via_local_relay_rows_before_relay_echo() {
    let (mut id, mut kernel, publish_store) = fresh_with_publish_store();
    let mut profile = std::collections::HashMap::new();
    profile.insert("name".to_string(), "Signup User".to_string());
    let relays = vec![("wss://signup-write.test".to_string(), "write".to_string())];
    create_account(
        &mut id,
        &mut kernel,
        false,
        &profile,
        &relays,
        &[],
        false,
        true,
    );

    let unsigned = nmp_signer_iface::UnsignedEvent {
        pubkey: String::new(), // ignored by signer; filled from active account
        kind: 1,
        tags: Vec::new(),
        content: "first note after signup".to_string(),
        created_at: 0,
    };
    let outbound = publish_unsigned_event(
        &id,
        &mut kernel,
        unsigned,
        None,
        None,
        None,
        &mut crate::actor::pending_sign::ParkedSignerOps::new(),
    );
    assert!(
        outbound
            .iter()
            .any(|msg| msg.relay_url == "wss://signup-write.test"),
        "next note must route through the active account's local write rows before kind:10002 echo"
    );
    assert!(
        kernel
            .last_error_toast_snapshot()
            .map(|toast| !toast.contains("no write-relays"))
            .unwrap_or(true),
        "publish before relay-list echo must not show the no write-relays toast"
    );

    let records = publish_store
        .load_pending()
        .expect("pending publish records after next note");
    let note = record_of_kind(&records, 1);
    assert_eq!(
        target_relays(note),
        vec!["wss://signup-write.test".to_string()],
        "kind:1 publish intent must persist with the local write relay target"
    );
}

#[test]
fn switch_active_flips_status_synchronously() {
    let (mut id, mut kernel) = fresh();
    sign_in_nsec(&mut id, &mut kernel, TEST_NSEC, false);
    let profile = std::collections::HashMap::new();
    let relays: Vec<(String, String)> = vec![];
    create_account(
        &mut id,
        &mut kernel,
        false,
        &profile,
        &relays,
        &[],
        false,
        true,
    );
    let first_id = kernel.account_snapshot().0[0].id.clone();
    let second_active = id.active_pubkey().unwrap();
    assert_ne!(first_id, second_active);

    switch_active(&mut id, &mut kernel, &first_id, false);
    let (accounts, active) = kernel.account_snapshot();
    assert_eq!(active, Some(&first_id));
    let first = accounts.iter().find(|a| a.id == first_id).unwrap();
    assert_eq!(first.status, "active");
    let second = accounts.iter().find(|a| a.id == second_active).unwrap();
    assert_eq!(second.status, "idle");
}

#[test]
fn switch_to_unknown_account_toasts_and_no_op() {
    let (mut id, mut kernel) = fresh();
    sign_in_nsec(&mut id, &mut kernel, TEST_NSEC, false);
    let before = id.active_pubkey();
    switch_active(&mut id, &mut kernel, SECOND_HEX, false);
    assert_eq!(id.active_pubkey(), before);
    assert!(kernel
        .last_error_toast_snapshot()
        .is_some_and(|t| t.contains("account not found")));
    assert_eq!(
        kernel.last_error_category_snapshot().map(String::as_str),
        Some(crate::ui_token::codes::IDENTITY_ACCOUNT_NOT_FOUND)
    );
}

#[test]
fn remove_active_account_clears_active_slot() {
    let (mut id, mut kernel) = fresh();
    sign_in_nsec(&mut id, &mut kernel, TEST_NSEC, false);
    let only = kernel.account_snapshot().0[0].id.clone();
    remove_account(&mut id, &mut kernel, &only);
    let (accounts, active) = kernel.account_snapshot();
    assert!(accounts.is_empty());
    assert!(active.is_none());
}
