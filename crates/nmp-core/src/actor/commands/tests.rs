//! T66a command-path unit tests.
//!
//! Each test drives the public command handlers against a real `Kernel` +
//! `IdentityRuntime` (no mocks) and asserts on the snapshot projections the
//! FFI surfaces — exactly what the SwiftUI screens read.

use super::*;
use crate::kernel::Kernel;
use crate::publish::{InMemoryPublishStore, PublishRecord, PublishStore, PublishTarget};
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use std::sync::Arc;

const TEST_NSEC: &str = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";
const SECOND_HEX: &str = "0000000000000000000000000000000000000000000000000000000000000abc";

/// Write relays injected via kind:10002 for tests that exercise the publish path.
///
/// T-publish-resolver-indexer (codex f81f735): `Nip65OutboxResolver` is now
/// fail-closed — an author with no kind:10002 resolves to an empty relay set
/// (`NoTargets`). Tests that assert non-empty outbound frames MUST seed a
/// kind:10002 for the active account before publishing.
const TEST_WRITE_RELAYS: &[&str] = &["wss://test-write-r1.test", "wss://test-write-r2.test"];

/// Test shim preserving the pre-`AddSigner` `sign_in_nsec(id, kernel, secret,
/// relays_ready)` call shape used throughout this file. Delegates to the
/// unified `add_signer` reducer with `make_active: true` (the old `sign_in_nsec`
/// always activated the imported key).
fn sign_in_nsec(
    identity: &mut IdentityRuntime,
    kernel: &mut Kernel,
    secret: &str,
    relays_ready: bool,
) -> Vec<crate::relay::OutboundMessage> {
    add_signer(
        identity,
        kernel,
        crate::actor::SignerSource::LocalNsec(zeroize::Zeroizing::new(secret.to_string())),
        true,
        relays_ready,
    )
}

/// Test shim preserving the pre-`AddSigner` `sign_in_bunker(id, kernel, uri)`
/// call shape. Delegates to the unified `add_signer` reducer's `BunkerUri`
/// branch with `make_active: true` (the old bunker sign-in always activated the
/// resolved account). Needs `&mut` because the reducer stashes the
/// `make_active` flag for the async handshake round-trip.
fn sign_in_bunker(identity: &mut IdentityRuntime, kernel: &mut Kernel, uri: &str) {
    add_signer(
        identity,
        kernel,
        crate::actor::SignerSource::BunkerUri(uri.to_string()),
        true,
        false,
    );
}

fn fresh() -> (IdentityRuntime, Kernel) {
    (
        IdentityRuntime::new(
            new_bunker_handshake_slot(),
            crate::actor::new_signer_state_slot(),
        ),
        Kernel::new(DEFAULT_VISIBLE_LIMIT),
    )
}

fn fresh_with_publish_store() -> (IdentityRuntime, Kernel, Arc<InMemoryPublishStore>) {
    let publish_store = Arc::new(InMemoryPublishStore::new());
    let kernel = Kernel::with_publish_store(
        DEFAULT_VISIBLE_LIMIT,
        Arc::clone(&publish_store) as Arc<dyn PublishStore>,
    );
    (
        IdentityRuntime::new(
            new_bunker_handshake_slot(),
            crate::actor::new_signer_state_slot(),
        ),
        kernel,
        publish_store,
    )
}

/// Sign in with TEST_NSEC and seed kind:10002 write relays for the active
/// account so the `Nip65OutboxResolver` has NIP-65 data and publish commands
/// produce non-empty outbound frames.
fn sign_in_with_nip65(id: &mut IdentityRuntime, kernel: &mut Kernel) {
    sign_in_nsec(id, kernel, TEST_NSEC, false);
    let pubkey = id
        .active_pubkey()
        .expect("active account after sign_in_nsec");
    kernel.seed_kind10002_for_test(&pubkey, TEST_WRITE_RELAYS);
}

fn record_of_kind(records: &[PublishRecord], kind: u32) -> &PublishRecord {
    records
        .iter()
        .find(|record| record.event.unsigned.kind == kind)
        .unwrap_or_else(|| panic!("expected pending publish record for kind:{kind}"))
}

fn target_relays(record: &PublishRecord) -> Vec<String> {
    let mut relays: Vec<String> = record
        .per_relay
        .iter()
        .map(|(relay, _state)| relay.clone())
        .collect();
    relays.sort();
    relays
}

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

/// aim.md §4.4 / §4.5: native cannot derive signer-display labels with a
/// `switch` on a wire token, nor scope a "remote signers" list with a
/// lowercased string comparison, nor compute `isActive` from `status == ..`.
/// The actor pre-classifies all three on every row.
#[test]
fn local_account_projection_carries_preclassified_signer_fields() {
    let (mut id, mut kernel) = fresh();
    sign_in_nsec(&mut id, &mut kernel, TEST_NSEC, false);
    let (accounts, _) = kernel.account_snapshot();
    let row = &accounts[0];
    assert_eq!(row.signer_kind, "local");
    assert_eq!(row.signer_label, "Local key");
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
}

#[test]
fn create_account_generates_fresh_active_key() {
    let (mut id, mut kernel) = fresh();
    let profile = std::collections::HashMap::new();
    let relays: Vec<(String, String)> = vec![];
    create_account(&mut id, &mut kernel, false, &profile, &relays, false, true);
    assert_eq!(kernel.account_snapshot().0.len(), 1);
    assert!(id.active_pubkey().is_some());
}

#[test]
fn create_account_empty_relays_keeps_preconfigured_relays() {
    // New contract: `nmp-core` no longer owns a hardcoded onboarding default.
    // The app declares its relay set (via `NmpAppBuilder` /
    // `ActorCommand::Start { initial_relays }`); `create_account` only
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
    create_account(&mut id, &mut kernel, false, &profile, &relays, false, true);

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
    create_account(&mut id, &mut kernel, false, &profile, &relays, false, true);

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
    create_account(&mut id, &mut kernel, false, &profile, &relays, false, true);

    let rows = kernel.configured_relays_snapshot();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].url, "wss://maestro.test");
    assert_eq!(rows[0].role, "both,indexer");
}

#[test]
fn create_account_publishes_bootstrap_events_and_persists_relay_rows() {
    let (mut id, mut kernel, publish_store) = fresh_with_publish_store();
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
    let outbound = create_account(&mut id, &mut kernel, false, &profile, &relays, false, true);
    assert!(
        outbound.iter().any(|msg| msg.text.contains("\"kind\":0")),
        "create_account must return the kind:0 EVENT frame for actor dispatch"
    );
    assert!(
        outbound
            .iter()
            .any(|msg| msg.text.contains("\"kind\":10002")),
        "create_account must return the kind:10002 EVENT frame for actor dispatch"
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
    assert_eq!(kinds, vec![0, 3, 10002]);

    let expected_targets = vec![
        "wss://signup-indexer.test".to_string(),
        "wss://signup-read.test".to_string(),
        "wss://signup-write.test".to_string(),
    ];
    for kind in [0, 3, 10002] {
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

    let relay_list = record_of_kind(&records, 10002);
    assert!(relay_list.event.unsigned.tags.contains(&vec![
        "r".to_string(),
        "wss://signup-write.test".to_string(),
        "write".to_string(),
    ]));
    assert!(relay_list.event.unsigned.tags.contains(&vec![
        "r".to_string(),
        "wss://signup-read.test".to_string(),
        "read".to_string(),
    ]));
    assert!(
        !relay_list.event.unsigned.tags.iter().any(|tag| tag
            .get(1)
            .is_some_and(|url| url == "wss://signup-indexer.test")),
        "indexer rows are app relay config, not NIP-65 account relay tags"
    );

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
    create_account(&mut id, &mut kernel, false, &profile, &relays, false, true);

    let unsigned = crate::substrate::UnsignedEvent {
        pubkey: String::new(), // ignored by signer; filled from active account
        kind: 1,
        tags: Vec::new(),
        content: "first note after signup".to_string(),
        created_at: 0,
    };
    let outbound = publish_unsigned_event(&id, &mut kernel, unsigned, None, None, &mut Vec::new());
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
    create_account(&mut id, &mut kernel, false, &profile, &relays, false, true);
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

#[test]
fn publish_unsigned_event_without_account_toasts_and_no_outbound() {
    let (id, mut kernel) = fresh();
    let unsigned = crate::substrate::UnsignedEvent {
        pubkey: String::new(), // ignored by signer; irrelevant when no account
        kind: 30023,
        tags: vec![vec!["d".into(), "x".into()]],
        content: "body".into(),
        created_at: 0,
    };
    let outbound = publish_unsigned_event(&id, &mut kernel, unsigned, None, None, &mut Vec::new());
    assert!(outbound.is_empty());
    assert!(kernel
        .last_error_toast_snapshot()
        .is_some_and(|t| t.contains("no active account")));
}

#[test]
fn publish_unsigned_event_signs_and_publishes_arbitrary_kind() {
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);
    let active_pubkey = id.active_pubkey().unwrap();
    // Construct a generic kind:30023 (NIP-23 article) UnsignedEvent inline —
    // no per-kind kernel logic; the kernel just signs + publishes.
    let unsigned = crate::substrate::UnsignedEvent {
        pubkey: "ignored-by-signer".into(),
        kind: 30023,
        tags: vec![
            vec!["d".into(), "test-article".into()],
            vec!["title".into(), "Hello".into()],
        ],
        content: "# body".into(),
        created_at: 1_700_000_000,
    };
    let outbound = publish_unsigned_event(&id, &mut kernel, unsigned, None, None, &mut Vec::new());
    assert!(!outbound.is_empty());
    assert!(outbound[0].text.contains("\"kind\":30023"));
    assert!(outbound[0]
        .text
        .contains(&format!("\"pubkey\":\"{active_pubkey}\"")));
    assert!(!outbound[0].text.contains("ignored-by-signer"));
    assert!(outbound[0].text.contains("\"d\""));
    assert!(outbound[0].text.contains("test-article"));
    let q = kernel.publish_queue_snapshot();
    assert_eq!(q.last().unwrap().kind, 30023);
    assert_eq!(q.last().unwrap().status, "accepted_locally");
}

// ── Findings 1 + 2 (codex batch review e895c09) ────────────────────────────
//
// Finding 1 (HIGH): `unsigned.kind as u16` silently truncates out-of-range
// kinds (e.g. 65559 → 23). Fix: validate range in `sign_with` and return
// `Err` so the caller surfaces a D6 toast. No publish must happen.
//
// Finding 2 (MEDIUM): `filter_map(|t| Tag::parse(t).ok())` silently drops
// malformed tags. Fix: count failures and hard-fail with a D6 toast listing
// the count. Valid tags must still pass through unchanged.

#[test]
fn publish_unsigned_event_rejects_oversized_kind_with_toast() {
    let (mut id, mut kernel) = fresh();
    sign_in_nsec(&mut id, &mut kernel, TEST_NSEC, false);
    // kind 100_000 is above u16::MAX (65_535) — previously it would silently
    // truncate to kind:34_464 (100_000 mod 65_536); now it must be rejected.
    let unsigned = crate::substrate::UnsignedEvent {
        pubkey: String::new(),
        kind: 100_000,
        tags: vec![],
        content: "should not publish".into(),
        created_at: 1_700_000_000,
    };
    let outbound = publish_unsigned_event(&id, &mut kernel, unsigned, None, None, &mut Vec::new());
    assert!(
        outbound.is_empty(),
        "oversized kind must produce no outbound frames"
    );
    assert!(
        kernel
            .last_error_toast_snapshot()
            .is_some_and(|t| t.contains("invalid kind") && t.contains("100000")),
        "expected toast about invalid kind, got: {:?}",
        kernel.last_error_toast_snapshot()
    );
    assert!(
        kernel.publish_queue_snapshot().is_empty(),
        "oversized kind must not appear in the publish queue"
    );
}

#[test]
fn publish_unsigned_event_valid_kind_publishes_normally() {
    // Regression for Finding 1: a valid u32 kind within [0, 65535] must still
    // publish exactly as before.
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);
    let unsigned = crate::substrate::UnsignedEvent {
        pubkey: String::new(),
        kind: 1,
        tags: vec![],
        content: "valid kind".into(),
        created_at: 1_700_000_000,
    };
    let outbound = publish_unsigned_event(&id, &mut kernel, unsigned, None, None, &mut Vec::new());
    assert!(
        !outbound.is_empty(),
        "valid kind:1 must produce outbound frames"
    );
    assert_eq!(kernel.last_error_toast_snapshot(), None);
    let q = kernel.publish_queue_snapshot();
    assert_eq!(q.len(), 1);
    assert_eq!(q[0].kind, 1);
}

#[test]
fn publish_unsigned_event_rejects_malformed_tag_with_toast() {
    let (mut id, mut kernel) = fresh();
    sign_in_nsec(&mut id, &mut kernel, TEST_NSEC, false);
    // An empty vec[] is rejected by Tag::parse (tag slice must be non-empty).
    let unsigned = crate::substrate::UnsignedEvent {
        pubkey: String::new(),
        kind: 1,
        tags: vec![vec![]], // malformed: empty tag row
        content: "tag test".into(),
        created_at: 1_700_000_000,
    };
    let outbound = publish_unsigned_event(&id, &mut kernel, unsigned, None, None, &mut Vec::new());
    assert!(
        outbound.is_empty(),
        "malformed tag must produce no outbound frames"
    );
    assert!(
        kernel
            .last_error_toast_snapshot()
            .is_some_and(|t| t.contains("malformed tag")),
        "expected toast about malformed tag, got: {:?}",
        kernel.last_error_toast_snapshot()
    );
    assert!(
        kernel.publish_queue_snapshot().is_empty(),
        "malformed tag must not appear in the publish queue"
    );
}

#[test]
fn publish_unsigned_event_valid_tags_pass_through() {
    // Regression for Finding 2: all-valid tags must still appear in the
    // signed event unchanged.
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);
    let unsigned = crate::substrate::UnsignedEvent {
        pubkey: String::new(),
        kind: 30023,
        tags: vec![
            vec!["d".into(), "test-slug".into()],
            vec!["title".into(), "Hello".into()],
        ],
        content: "body".into(),
        created_at: 1_700_000_000,
    };
    let outbound = publish_unsigned_event(&id, &mut kernel, unsigned, None, None, &mut Vec::new());
    assert!(!outbound.is_empty());
    assert_eq!(kernel.last_error_toast_snapshot(), None);
    assert!(outbound[0].text.contains("\"d\""));
    assert!(outbound[0].text.contains("test-slug"));
    assert!(outbound[0].text.contains("\"title\""));
}

// ── publish_signed_event — already-signed verbatim relay-publish path ───────
//
// Sibling to the unsigned tests above. The decisive difference: the signer is
// NEVER consulted. We produce a genuine signed event via
// `sign_active_nonblocking` (real Schnorr sig over TEST_NSEC's keys), serialize
// it to flat NIP-01 JSON, and feed it through the signed path. Assertions
// mirror the unsigned sibling.

/// Produce a genuine flat NIP-01 JSON for a real signed event over `id`'s
/// active keys (kind:30023 article — generic, kind-agnostic).
fn signed_nip01_json(id: &IdentityRuntime, content: &str) -> (String, String, String) {
    let unsigned = crate::substrate::UnsignedEvent {
        pubkey: String::new(), // ignored by signer
        kind: 30023,
        tags: vec![
            vec!["d".into(), "signed-test".into()],
            vec!["title".into(), "Signed".into()],
        ],
        content: content.into(),
        created_at: 1_700_000_000,
    };
    let signed = crate::actor::commands::identity::sign_active_nonblocking(id, &unsigned)
        .expect("sign_active_nonblocking ok")
        .poll()
        .expect("local sign resolves Ready immediately")
        .expect("sign produces a real signed event");
    let raw = crate::store::RawEvent {
        id: signed.id.clone(),
        pubkey: signed.unsigned.pubkey.clone(),
        created_at: signed.unsigned.created_at,
        kind: signed.unsigned.kind,
        tags: signed.unsigned.tags.clone(),
        content: signed.unsigned.content.clone(),
        sig: signed.sig.clone(),
    };
    let json = serde_json::to_string(&raw).expect("serialize flat NIP-01");
    (json, signed.id, signed.sig)
}

#[test]
fn flat_nip01_json_round_trips_into_raw_event() {
    // Lock in the RawEvent serde shape == the flat NIP-01 event object the
    // FFI contract advertises (field-name based, not order based).
    let literal = r#"{"id":"aa","pubkey":"bb","created_at":1700000000,
        "kind":30023,"tags":[["d","x"]],"content":"hi","sig":"cc"}"#;
    let raw: crate::store::RawEvent =
        serde_json::from_str(literal).expect("flat NIP-01 → RawEvent");
    assert_eq!(raw.id, "aa");
    assert_eq!(raw.pubkey, "bb");
    assert_eq!(raw.created_at, 1_700_000_000);
    assert_eq!(raw.kind, 30023);
    assert_eq!(raw.content, "hi");
    assert_eq!(raw.sig, "cc");
}

#[test]
fn publish_signed_event_routes_and_dispatches_verbatim() {
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);
    let active_pubkey = id.active_pubkey().unwrap();
    let (json, ev_id, ev_sig) = signed_nip01_json(&id, "# signed body");

    let raw: crate::store::RawEvent = serde_json::from_str(&json).unwrap();
    let outbound = publish_signed_event(&mut kernel, raw, PublishTarget::Auto, None);

    assert!(!outbound.is_empty(), "valid signed event must route");
    assert_eq!(kernel.last_error_toast_snapshot(), None);
    // Verbatim: the exact id + sig bytes from the input appear on the wire
    // frame unchanged (no re-signing).
    assert!(
        outbound[0].text.contains(&format!("\"id\":\"{ev_id}\"")),
        "event id must be carried through verbatim"
    );
    assert!(
        outbound[0].text.contains(&format!("\"sig\":\"{ev_sig}\"")),
        "signature must be carried through verbatim — never re-signed"
    );
    assert!(outbound[0]
        .text
        .contains(&format!("\"pubkey\":\"{active_pubkey}\"")));
    assert!(outbound[0].text.contains("\"kind\":30023"));
    let q = kernel.publish_queue_snapshot();
    assert_eq!(q.last().unwrap().kind, 30023);
    assert_eq!(q.last().unwrap().status, "accepted_locally");
}

#[test]
fn publish_signed_event_publishes_without_active_account() {
    // Behavioral asymmetry vs. the unsigned sibling: the signature already
    // exists, routing keys off the event's own pubkey (its kind:10002), so
    // NO active account is required. Sign the event under a throwaway
    // identity, seed THAT pubkey's kind:10002, then publish on a kernel with
    // no active account.
    let (mut signer_id, mut signer_kernel) = fresh();
    sign_in_with_nip65(&mut signer_id, &mut signer_kernel);
    let author = signer_id.active_pubkey().unwrap();
    let (json, ev_id, _sig) = signed_nip01_json(&signer_id, "no-account body");

    // Fresh kernel: NO account signed in, but the author's kind:10002 seeded.
    let (no_acct_id, mut kernel) = fresh();
    assert!(no_acct_id.active_pubkey().is_none());
    kernel.seed_kind10002_for_test(&author, TEST_WRITE_RELAYS);

    let raw: crate::store::RawEvent = serde_json::from_str(&json).unwrap();
    let outbound = publish_signed_event(&mut kernel, raw, PublishTarget::Auto, None);

    assert!(
        !outbound.is_empty(),
        "signed event must publish even with no active account"
    );
    assert_eq!(kernel.last_error_toast_snapshot(), None);
    assert!(outbound[0].text.contains(&format!("\"id\":\"{ev_id}\"")));
}

#[test]
fn publish_signed_event_rejects_tampered_signature_with_toast() {
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);
    let (json, _ev_id, sig) = signed_nip01_json(&id, "tamper me");

    // Flip one hex char of the signature — id stays valid, sig is now forged.
    let flipped = if sig.starts_with('a') { 'b' } else { 'a' };
    let bad_json = json.replacen(&sig, &format!("{flipped}{}", &sig[1..]), 1);
    assert_ne!(bad_json, json, "signature must actually have changed");

    let raw: crate::store::RawEvent = serde_json::from_str(&bad_json).unwrap();
    let outbound = publish_signed_event(&mut kernel, raw, PublishTarget::Auto, None);

    assert!(
        outbound.is_empty(),
        "forged-signature event must produce no outbound frames"
    );
    assert!(
        kernel
            .last_error_toast_snapshot()
            .is_some_and(|t| t.contains("signed event rejected")),
        "expected rejection toast, got: {:?}",
        kernel.last_error_toast_snapshot()
    );
    assert!(
        kernel.publish_queue_snapshot().is_empty(),
        "forged event must never enter the publish queue"
    );
}

#[test]
fn publish_signed_event_rejects_id_mismatch_with_toast() {
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);
    let (json, _ev_id, _sig) = signed_nip01_json(&id, "id mismatch");

    // Mutate content without re-deriving the id → id-hash check must fail.
    let mut raw: crate::store::RawEvent = serde_json::from_str(&json).unwrap();
    raw.content = "tampered-after-signing".into();
    let outbound = publish_signed_event(&mut kernel, raw, PublishTarget::Auto, None);

    assert!(outbound.is_empty(), "id-mismatch event must not publish");
    assert!(kernel
        .last_error_toast_snapshot()
        .is_some_and(|t| t.contains("signed event rejected")));
    assert!(kernel.publish_queue_snapshot().is_empty());
}

// ── publish_signed_event_to — EXPLICIT relay targeting (Marmot D3 opt-out) ──
//
// kind:445 group messages must go to the pinned GROUP relay, kind:1059
// gift-wraps to recipient inbox relays — relays the author's kind:10002
// outbox does NOT cover. The explicit-target path routes the verbatim signed
// event to EXACTLY the named relays, bypassing the NIP-65 resolver, while
// still gating Schnorr+id and never invoking the signer.

/// Relays distinct from `TEST_WRITE_RELAYS` so the assertion discriminates an
/// honest Explicit route from a silent Auto/outbox fallback.
const TEST_GROUP_RELAYS: &[&str] = &["wss://group-relay-a.test", "wss://group-relay-b.test"];

#[test]
fn publish_signed_event_to_explicit_relays_routes_verbatim_to_exactly_those() {
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);
    let active_pubkey = id.active_pubkey().unwrap();
    let (json, ev_id, ev_sig) = signed_nip01_json(&id, "group message body");

    let relays: Vec<String> = TEST_GROUP_RELAYS.iter().map(|s| s.to_string()).collect();
    let raw: crate::store::RawEvent = serde_json::from_str(&json).unwrap();
    let outbound = publish_signed_event(
        &mut kernel,
        raw,
        PublishTarget::Explicit {
            relays: relays.clone(),
        },
        None,
    );

    assert!(!outbound.is_empty(), "explicit-target publish must route");
    assert_eq!(kernel.last_error_toast_snapshot(), None);

    // The relay set is EXACTLY the explicit targets — and contains none of
    // the author's kind:10002 outbox. This single assertion is what
    // distinguishes Explicit from a silent Auto fallback.
    let mut got: Vec<String> = outbound.iter().map(|m| m.relay_url.clone()).collect();
    got.sort();
    let mut want = relays.clone();
    want.sort();
    assert_eq!(got, want, "must dispatch to exactly the explicit relays");
    for url in TEST_WRITE_RELAYS {
        assert!(
            !got.iter().any(|g| g == url),
            "explicit target must NOT leak to the kind:10002 outbox relay {url}"
        );
    }

    // Verbatim id/sig/pubkey — the signer was never consulted.
    assert!(outbound[0].text.contains(&format!("\"id\":\"{ev_id}\"")));
    assert!(outbound[0].text.contains(&format!("\"sig\":\"{ev_sig}\"")));
    assert!(outbound[0]
        .text
        .contains(&format!("\"pubkey\":\"{active_pubkey}\"")));
}

#[test]
fn publish_signed_event_to_empty_explicit_relays_fails_closed() {
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);
    let (json, _ev_id, _sig) = signed_nip01_json(&id, "empty explicit body");

    let raw: crate::store::RawEvent = serde_json::from_str(&json).unwrap();
    let outbound = publish_signed_event(
        &mut kernel,
        raw,
        PublishTarget::Explicit { relays: Vec::new() },
        None,
    );

    assert!(
        outbound.is_empty(),
        "empty explicit relays must not publish"
    );
    assert!(
        kernel
            .last_error_toast_snapshot()
            .is_some_and(|t| t.contains("explicit publish target rejected")),
        "expected explicit-target rejection toast, got: {:?}",
        kernel.last_error_toast_snapshot()
    );
    assert!(kernel.publish_queue_snapshot().is_empty());
}

#[test]
fn publish_signed_event_to_explicit_relays_works_with_no_active_account() {
    // The realistic Marmot case: a kind:445 group message / kind:1059
    // gift-wrap was signed elsewhere (MDK group signer) and must go to a
    // pinned relay while the user is signed-out. The explicit path keys off
    // the verbatim relays — NOT the author's kind:10002 — so no active
    // account is required AND no kind:10002 seed is needed.
    let (mut signer_id, mut signer_kernel) = fresh();
    sign_in_with_nip65(&mut signer_id, &mut signer_kernel);
    let (json, ev_id, ev_sig) = signed_nip01_json(&signer_id, "signed-out group msg");

    // Fresh kernel: NO account signed in, NO kind:10002 seeded for anyone.
    let (no_acct_id, mut kernel) = fresh();
    assert!(no_acct_id.active_pubkey().is_none());

    let relays: Vec<String> = TEST_GROUP_RELAYS.iter().map(|s| s.to_string()).collect();
    let raw: crate::store::RawEvent = serde_json::from_str(&json).unwrap();
    let outbound = publish_signed_event(
        &mut kernel,
        raw,
        PublishTarget::Explicit {
            relays: relays.clone(),
        },
        None,
    );

    assert!(
        !outbound.is_empty(),
        "explicit-target publish must work with no active account and no kind:10002"
    );
    assert_eq!(kernel.last_error_toast_snapshot(), None);
    let mut got: Vec<String> = outbound.iter().map(|m| m.relay_url.clone()).collect();
    got.sort();
    let mut want = relays.clone();
    want.sort();
    assert_eq!(got, want, "must dispatch to exactly the explicit relays");
    assert!(outbound[0].text.contains(&format!("\"id\":\"{ev_id}\"")));
    assert!(outbound[0].text.contains(&format!("\"sig\":\"{ev_sig}\"")));
}

// ── D10 defensive guard — kind:1059 + empty relays NEVER Auto-routes ────────
//
// A `dispatch_action("nmp.publish", PublishAction::Publish { target: Auto })`
// for a kind:1059 envelope lands in `actor::dispatch::PublishSignedEvent` with
// `relays: vec![]`, which calls `publish_signed_event(kernel, raw, &[], cid)`
// and falls through the `relays.is_empty()` branch → `publish_signed_with_correlation`
// → `PublishTarget::Auto` → leak. The same hole exists at the
// `NmpApp::publish_signed_explicit` workspace-internal seam.
//
// The D10 defensive guard at the top of `publish_signed_event` refuses any
// kind:1059 publish whose `relays` slice is empty — the encrypted envelope is
// dropped, a D6 toast names the refusal, and no outbound frames / publish
// queue entries are produced. These tests pin the guard's shape from every
// entry point the kernel can be reached through.

/// Produce a genuine signed kind:1059 (NIP-59 gift-wrap shape) RawEvent.
///
/// The body is a placeholder ciphertext — the gift-wrap construction's
/// authenticity gate is the outer Schnorr signature, and
/// `sign_active_nonblocking` mints a real Schnorr over the active keys.
/// `VerifiedEvent::try_from_raw` (the gate that runs first inside
/// `publish_signed_event`) accepts this as a well-formed signed event; only
/// the kernel-level D10 guard rejects it.
fn signed_kind_1059_raw(id: &IdentityRuntime) -> crate::store::RawEvent {
    let unsigned = crate::substrate::UnsignedEvent {
        pubkey: String::new(), // ignored by signer
        kind: 1059,
        tags: vec![vec![
            "p".into(),
            "0000000000000000000000000000000000000000000000000000000000000abc".into(),
        ]],
        content: "AAAA-placeholder-ciphertext".into(),
        created_at: 1_700_000_000,
    };
    let signed = crate::actor::commands::identity::sign_active_nonblocking(id, &unsigned)
        .expect("sign_active_nonblocking ok")
        .poll()
        .expect("local sign resolves Ready immediately")
        .expect("sign produces a real signed kind:1059 envelope");
    crate::store::RawEvent {
        id: signed.id.clone(),
        pubkey: signed.unsigned.pubkey.clone(),
        created_at: signed.unsigned.created_at,
        kind: signed.unsigned.kind,
        tags: signed.unsigned.tags.clone(),
        content: signed.unsigned.content.clone(),
        sig: signed.sig.clone(),
    }
}

/// Direct-call shape — the same call the actor's
/// `ActorCommand::PublishSignedEvent` arm performs when the dispatch path
/// routes a kind:1059 envelope with `target: PublishTarget::Auto`. The guard
/// must fire BEFORE the `relays.is_empty()` → Auto branch can reach the
/// outbox resolver.
#[test]
fn publish_signed_event_refuses_kind_1059_with_empty_relays() {
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);
    // Belt-and-suspenders: even with the kernel's `configured_relays` truly
    // empty (no cfg(test) fallback Content relay), the guard must still
    // refuse — proving the refusal happens upstream of the outbox resolver.
    kernel.clear_configured_relays_for_test();
    let raw = signed_kind_1059_raw(&id);

    let outbound = publish_signed_event(&mut kernel, raw, PublishTarget::Auto, None);

    assert!(
        outbound.is_empty(),
        "kind:1059 with PublishTarget::Auto MUST produce no outbound frames \
         (D10: envelope existence would leak through the NIP-65 outbox)"
    );
    assert!(
        kernel
            .last_error_toast_snapshot()
            .is_some_and(|t| t.contains("kind:1059") && t.contains("D10")),
        "guard must surface a D6 toast naming kind:1059 and D10; got: {:?}",
        kernel.last_error_toast_snapshot()
    );
    assert!(
        kernel.publish_queue_snapshot().is_empty(),
        "refused kind:1059 envelope must NEVER enter the publish queue"
    );
}

/// The same dispatch shape `kernel::action_registry::default_registry()`
/// produces for `PublishAction::Publish { target: Auto }` — `relays_for_target(&Auto)`
/// returns `Vec::new()`. The defensive guard MUST fire for the empty-Vec
/// shape too, not just `&[]` slice literals.
#[test]
fn publish_signed_event_refuses_kind_1059_with_empty_vec_relays() {
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);
    kernel.clear_configured_relays_for_test();
    let raw = signed_kind_1059_raw(&id);

    // Exact shape `actor::dispatch::PublishSignedEvent` calls with when the
    // dispatch path routes `PublishAction::Publish { target: Auto }`. The
    // guard must fire on the Auto variant regardless of how the target was
    // constructed.
    let outbound = publish_signed_event(&mut kernel, raw, PublishTarget::Auto, None);

    assert!(
        outbound.is_empty(),
        "PublishTarget::Auto must trigger the guard"
    );
    assert!(
        kernel.last_error_toast_snapshot().is_some(),
        "the guard must set a toast for the empty Vec case too"
    );
    assert!(kernel.publish_queue_snapshot().is_empty());
}

/// Sanity bound — the guard is targeted at kind:1059 ONLY. A non-1059
/// signed event with empty relays must STILL Auto-route (the pre-existing
/// behaviour that the rest of the codebase relies on: kind:1 react,
/// kind:30023 article, etc., all use this fallback intentionally).
#[test]
fn publish_signed_event_does_not_refuse_other_kinds_with_empty_relays() {
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);
    let (json, _ev_id, _sig) = signed_nip01_json(&id, "kind 30023 still routes");

    let raw: crate::store::RawEvent = serde_json::from_str(&json).unwrap();
    let outbound = publish_signed_event(&mut kernel, raw, PublishTarget::Auto, None);

    assert!(
        !outbound.is_empty(),
        "non-1059 kinds must continue to route under PublishTarget::Auto — the \
         D10 guard is targeted strictly at kind:1059"
    );
    assert_eq!(
        kernel.last_error_toast_snapshot(),
        None,
        "non-1059 Auto-route must not surface a guard toast"
    );
}

/// Broken-promise contract — when the dispatch path supplied a
/// `correlation_id`, the guard's refusal must reach `action_results` as a
/// terminal `failed` verdict so the host's spinner clears. This mirrors the
/// pattern in `publish_profile` for its sign-step early-
/// exits (see `kernel::action_failure_tests`). Without this, a dispatched
/// kind:1059 publish with `target: Auto` would hang the host spinner forever.
#[test]
fn publish_signed_event_kind_1059_guard_records_action_failure_for_correlation() {
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);
    kernel.clear_configured_relays_for_test();
    let raw = signed_kind_1059_raw(&id);

    let outbound = publish_signed_event(
        &mut kernel,
        raw,
        PublishTarget::Auto,
        Some("corr-1059-leak".to_string()),
    );
    assert!(outbound.is_empty());

    // The guard must surface a terminal `failed` verdict under the dispatch
    // correlation_id so the host's spinner can be cleared.
    let snapshot_json = kernel.make_update_json_for_test(true);
    let parsed: serde_json::Value = serde_json::from_str(&snapshot_json).unwrap();
    let results = parsed
        .get("projections")
        .and_then(|v| v.get("action_results"))
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let arr = results.as_array().unwrap_or_else(|| {
        panic!(
            "guard must surface a terminal verdict under correlation_id; got: {}",
            results
        )
    });
    assert_eq!(arr.len(), 1, "exactly one terminal verdict from the guard");
    let entry = &arr[0];
    assert_eq!(
        entry.get("correlation_id").and_then(|v| v.as_str()),
        Some("corr-1059-leak"),
        "the dispatch correlation_id is carried through"
    );
    assert_eq!(
        entry.get("status").and_then(|v| v.as_str()),
        Some("failed"),
        "guard refusal reports the terminal `failed` status"
    );
}

/// The corresponding HAPPY path — a kind:1059 publish with an EXPLICIT pin
/// must succeed unchanged. The guard targets the empty-relays branch only;
/// any non-empty `relays` slice carries the envelope on the
/// `PublishTarget::Explicit` path (the correct shape for NIP-17 / Marmot).
#[test]
fn publish_signed_event_publishes_kind_1059_with_explicit_pin() {
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);
    let raw = signed_kind_1059_raw(&id);

    let pin: Vec<String> = TEST_GROUP_RELAYS.iter().map(|s| s.to_string()).collect();
    let outbound = publish_signed_event(
        &mut kernel,
        raw,
        PublishTarget::Explicit {
            relays: pin.clone(),
        },
        None,
    );

    assert!(
        !outbound.is_empty(),
        "kind:1059 + explicit pin must publish (guard is PublishTarget::Auto only)"
    );
    assert_eq!(
        kernel.last_error_toast_snapshot(),
        None,
        "the happy path must not surface a guard toast"
    );
    // The envelope MUST go to exactly the pinned relays — NOT the author's
    // kind:10002 outbox. This is what NIP-17 / Marmot rely on.
    let mut got: Vec<String> = outbound.iter().map(|m| m.relay_url.clone()).collect();
    got.sort();
    let mut want = pin.clone();
    want.sort();
    assert_eq!(
        got, want,
        "kind:1059 with explicit pin must route to EXACTLY the pin, never to the kind:10002 outbox"
    );
}

#[test]
fn publish_signed_event_to_explicit_relays_still_rejects_tampered_sig() {
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);
    let (json, _ev_id, sig) = signed_nip01_json(&id, "explicit tamper");

    let flipped = if sig.starts_with('a') { 'b' } else { 'a' };
    let bad_json = json.replacen(&sig, &format!("{flipped}{}", &sig[1..]), 1);
    assert_ne!(bad_json, json);

    let relays: Vec<String> = TEST_GROUP_RELAYS.iter().map(|s| s.to_string()).collect();
    let raw: crate::store::RawEvent = serde_json::from_str(&bad_json).unwrap();
    let outbound = publish_signed_event(&mut kernel, raw, PublishTarget::Explicit { relays }, None);

    assert!(
        outbound.is_empty(),
        "forged-signature event must not publish even with explicit relays"
    );
    assert!(
        kernel
            .last_error_toast_snapshot()
            .is_some_and(|t| t.contains("signed event rejected")),
        "expected the same rejection toast contract as the Auto path"
    );
    assert!(kernel.publish_queue_snapshot().is_empty());
}

// ── publish_unsigned_event_to_relays — sign + EXPLICIT relay pin ────────────
//
// The host-pinned twin of `publish_unsigned_event`: it SIGNS with the active
// account (unlike `publish_signed_event` which carries an already-signed
// event) and ROUTES to an explicit relay set (unlike `publish_unsigned_event`
// which routes via the NIP-65 outbox). This is the path a NIP-29 group action
// needs — a join request must reach the group's host relay, not the author's
// kind:10002 outbox.

#[test]
fn publish_unsigned_event_to_relays_signs_and_routes_to_exactly_those() {
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);
    let active_pubkey = id.active_pubkey().unwrap();

    // A kind:9021 NIP-29 join-request-shaped unsigned event. `pubkey` is a
    // placeholder — the signer derives it from the active identity.
    let unsigned = crate::substrate::UnsignedEvent {
        pubkey: String::new(),
        kind: 9021,
        tags: vec![vec!["h".into(), "rust-nostr".into()]],
        content: "hello".into(),
        created_at: 1_700_000_000,
    };
    let relays: Vec<String> = TEST_GROUP_RELAYS.iter().map(|s| s.to_string()).collect();
    let outbound = publish_unsigned_event_to_relays(
        &id,
        &mut kernel,
        unsigned,
        relays.clone(),
        None,
        None,
        &mut Vec::new(),
    );

    assert!(!outbound.is_empty(), "host-pinned publish must route");
    assert_eq!(kernel.last_error_toast_snapshot(), None);

    // The relay set is EXACTLY the explicit pin — and contains none of the
    // author's kind:10002 outbox. This distinguishes the Explicit route from
    // a silent fall-through to the NIP-65 outbox resolver.
    let mut got: Vec<String> = outbound.iter().map(|m| m.relay_url.clone()).collect();
    got.sort();
    let mut want = relays.clone();
    want.sort();
    assert_eq!(got, want, "must dispatch to exactly the pinned relays");
    for url in TEST_WRITE_RELAYS {
        assert!(
            !got.iter().any(|g| g == url),
            "host-pinned publish must NOT leak to the kind:10002 outbox relay {url}"
        );
    }

    // The event was signed by the active account: its pubkey is on the wire
    // frame even though the caller passed an empty `pubkey`.
    assert!(outbound[0]
        .text
        .contains(&format!("\"pubkey\":\"{active_pubkey}\"")));
    assert!(outbound[0].text.contains("\"kind\":9021"));
    assert_eq!(kernel.publish_queue_snapshot().last().unwrap().kind, 9021);
}

#[test]
fn publish_unsigned_event_to_relays_without_account_toasts() {
    // Unlike `publish_signed_event` (signature already exists, no account
    // needed), this path SIGNS — so a missing active account is surfaced as a
    // toast (D6), never a panic, and produces no outbound frames.
    let (id, mut kernel) = fresh();
    assert!(id.active_pubkey().is_none());

    let unsigned = crate::substrate::UnsignedEvent {
        pubkey: String::new(),
        kind: 9021,
        tags: vec![vec!["h".into(), "rust-nostr".into()]],
        content: String::new(),
        created_at: 1_700_000_000,
    };
    let relays: Vec<String> = TEST_GROUP_RELAYS.iter().map(|s| s.to_string()).collect();
    let outbound =
        publish_unsigned_event_to_relays(
            &id,
            &mut kernel,
            unsigned,
            relays,
            None,
            None,
            &mut Vec::new(),
        );

    assert!(
        outbound.is_empty(),
        "no active account must produce no outbound frames"
    );
    assert!(
        kernel
            .last_error_toast_snapshot()
            .is_some_and(|t| t.contains("no active account")),
        "expected a no-account toast, got: {:?}",
        kernel.last_error_toast_snapshot()
    );
}

#[test]
fn publish_unsigned_event_to_relays_empty_relays_fails_closed() {
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);

    let unsigned = crate::substrate::UnsignedEvent {
        pubkey: String::new(),
        kind: 9021,
        tags: vec![vec!["h".into(), "rust-nostr".into()]],
        content: String::new(),
        created_at: 1_700_000_000,
    };
    let outbound = publish_unsigned_event_to_relays(
        &id,
        &mut kernel,
        unsigned,
        Vec::new(),
        None,
        None,
        &mut Vec::new(),
    );

    assert!(
        outbound.is_empty(),
        "empty explicit relays must not publish"
    );
    assert!(
        kernel
            .last_error_toast_snapshot()
            .is_some_and(|t| t.contains("explicit publish target rejected")),
        "expected explicit-target rejection toast, got: {:?}",
        kernel.last_error_toast_snapshot()
    );
    assert!(kernel.publish_queue_snapshot().is_empty());
}

#[test]
fn publish_unsigned_event_to_relays_invalid_relay_fails_closed() {
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);

    let unsigned = crate::substrate::UnsignedEvent {
        pubkey: String::new(),
        kind: 9021,
        tags: vec![vec!["h".into(), "rust-nostr".into()]],
        content: String::new(),
        created_at: 1_700_000_000,
    };
    let outbound = publish_unsigned_event_to_relays(
        &id,
        &mut kernel,
        unsigned,
        vec!["https://not-a-nostr-relay.example".to_string()],
        None,
        None,
        &mut Vec::new(),
    );

    assert!(
        outbound.is_empty(),
        "invalid explicit relay must not publish"
    );
    assert!(
        kernel
            .last_error_toast_snapshot()
            .is_some_and(|t| t.contains("ws:// or wss://")),
        "expected malformed relay rejection toast, got: {:?}",
        kernel.last_error_toast_snapshot()
    );
}

/// Pull out the most recent published event JSON the kernel emitted on the
/// wire so a test can assert on its tag shape.
fn last_published_event_json(outbound: &[crate::relay::OutboundMessage]) -> serde_json::Value {
    let frame = outbound
        .iter()
        .rev()
        .find(|m| m.text.starts_with("[\"EVENT\""))
        .expect("at least one EVENT frame");
    let parsed: serde_json::Value = serde_json::from_str(&frame.text).expect("EVENT frame is JSON");
    parsed
        .as_array()
        .and_then(|arr| arr.get(1).cloned())
        .expect("EVENT frame is [\"EVENT\", <event>]")
}

fn tags_of(event_json: &serde_json::Value) -> Vec<Vec<String>> {
    event_json["tags"]
        .as_array()
        .expect("tags array")
        .iter()
        .map(|t| {
            t.as_array()
                .expect("tag is array")
                .iter()
                .map(|c| c.as_str().expect("tag column is string").to_string())
                .collect()
        })
        .collect()
}

#[test]
fn react_builds_kind7_with_e_and_p_tags() {
    // NIP-25 §1: a kind:7 reaction carries an `e` tag (the reacted-to event)
    // AND a `p` tag (that event's author) so the author's relays route the
    // reaction to their notification inbox. The target is seeded into the
    // kernel read-cache with a known author distinct from the signer, so the
    // emitted `p` tag's pubkey is unambiguous.
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);
    let target = "a".repeat(64);
    let target_author = "cccc000000000000000000000000000000000000000000000000000000000000";
    kernel.seed_kind1_for_reply_test(&target, target_author, 100, vec![], "reacted-to note");

    let outbound = react(&id, &mut kernel, &target, "❤", None, &mut Vec::new());
    assert!(!outbound.is_empty());
    assert!(outbound[0].text.contains("\"kind\":7"));
    assert!(outbound[0].text.contains(&target));
    assert_eq!(kernel.publish_queue_snapshot().last().unwrap().kind, 7);

    let event = last_published_event_json(&outbound);
    let tags = tags_of(&event);
    assert_eq!(
        tags,
        vec![
            vec!["e".to_string(), target.clone()],
            vec!["p".to_string(), target_author.to_string()],
        ],
        "reaction must carry an `e` tag for the target and a `p` tag for its author"
    );
}

// Issue #1246 kind:3 full-edit follow tests live in the sibling child module
// `tests_follow_kind3_fulledit` (declared at the bottom of this file) to keep
// this file under the file-size hard cap.

// ── react: account / id-validation / default-content gaps ──────────────────
//
// `react_builds_kind7_with_e_tag` above covers only the custom-emoji happy
// path. These pin the three remaining branches in `publish::react`:
// the no-account D6 toast, the malformed-id D6 toast, and the empty-reaction
// → `"+"` default-content fallback (publish.rs:257-261).

#[test]
fn react_without_account_toasts_and_no_outbound() {
    // D6: a missing active account is surfaced as a toast across FFI, never
    // an exception. No EVENT frame, no publish-queue entry.
    let (id, mut kernel) = fresh();
    let target = "a".repeat(64);
    let outbound = react(&id, &mut kernel, &target, "+", None, &mut Vec::new());
    assert!(
        outbound.is_empty(),
        "react with no active account must produce no outbound frames"
    );
    assert!(kernel
        .last_error_toast_snapshot()
        .is_some_and(|t| t.contains("react") && t.contains("no active account")));
    assert!(
        kernel.publish_queue_snapshot().is_empty(),
        "react with no active account must not enqueue a publish"
    );
}

#[test]
fn react_to_malformed_event_id_toasts_and_refuses() {
    // The target must be a 64-hex event id. A malformed id is a user-visible
    // error (D6 toast), not a silent no-op — and must not panic.
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);
    let outbound = react(
        &id,
        &mut kernel,
        "not-a-real-event-id",
        "+",
        None,
        &mut Vec::new(),
    );
    assert!(
        outbound.is_empty(),
        "react to a malformed event id must produce no outbound frames"
    );
    assert!(kernel
        .last_error_toast_snapshot()
        .is_some_and(|t| t.contains("react") && t.contains("malformed")));
    assert!(
        kernel.publish_queue_snapshot().is_empty(),
        "react to a malformed event id must not enqueue a publish"
    );
}

#[test]
fn react_with_empty_reaction_defaults_to_plus() {
    // An empty/whitespace reaction string falls back to the NIP-25 default
    // `"+"` (a "like"). The emitted kind:7 must carry `"content":"+"`, not an
    // empty string. The target is seeded so the NIP-25 `p` tag is also exercised.
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);
    let target = "a".repeat(64);
    let target_author = "cccc000000000000000000000000000000000000000000000000000000000000";
    kernel.seed_kind1_for_reply_test(&target, target_author, 100, vec![], "reacted-to note");

    let outbound = react(&id, &mut kernel, &target, "   ", None, &mut Vec::new());
    assert!(!outbound.is_empty(), "react must produce an EVENT frame");
    let event = last_published_event_json(&outbound);
    assert_eq!(event["kind"], 7, "reaction must be kind:7");
    assert_eq!(
        event["content"], "+",
        "empty/whitespace reaction must default to the NIP-25 `+` like"
    );
    // NIP-25 §1: the reaction carries both an `e` tag for the target and a
    // `p` tag naming the reacted-to event's author (notification routing).
    let tags = tags_of(&event);
    assert_eq!(
        tags,
        vec![
            vec!["e".to_string(), target.clone()],
            vec!["p".to_string(), target_author.to_string()],
        ],
        "react must emit an `e` tag for the target and a `p` tag for its author"
    );
}

#[test]
fn react_to_uncached_event_omits_p_tag_gracefully() {
    // D6: when the reacted-to event is not in the kernel read-cache its author
    // is unknown, so the `p` tag cannot be built. The reaction must still
    // publish — degraded but valid NIP-25, with just the `e` tag — and must
    // never panic. (The target id is a well-formed 64-hex id that is simply
    // never seeded.)
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);
    let target = "d".repeat(64);

    let outbound = react(&id, &mut kernel, &target, "❤", None, &mut Vec::new());
    assert!(
        !outbound.is_empty(),
        "react to an uncached event must still publish a kind:7"
    );
    let event = last_published_event_json(&outbound);
    assert_eq!(event["kind"], 7, "reaction must be kind:7");
    let tags = tags_of(&event);
    assert_eq!(
        tags,
        vec![vec!["e".to_string(), target.clone()]],
        "uncached target → reaction carries only the `e` tag, no `p` tag"
    );
}

#[test]
fn react_routes_to_reacted_to_author_inbox_relay() {
    // NIP-25 §1 + NIP-65 inbox routing: a kind:7 reaction must not only *label*
    // the reacted-to author with a `p` tag — it must *reach* that author. The
    // publish engine derives `#p` recipients from the event's own tags
    // (`engine::helpers::collect_p_tags`) and the `Nip65OutboxResolver` unions
    // every recipient's kind:10002 READ relays (their inbox) into the publish
    // target set. So a reaction whose author has a known kind:10002 must emit an
    // outbound frame addressed to that author's inbox relay.
    //
    // The reactor's WRITE relays and the reacted-to author's READ (inbox)
    // relay are deliberately disjoint URLs: an inbox-routed frame can only
    // appear if the resolver actually consulted the recipient's kind:10002, so
    // the assertion proves inbox routing rather than incidental outbox overlap.
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel); // reactor → TEST_WRITE_RELAYS (write-marked)

    let target = "a".repeat(64);
    let target_author = "cccc000000000000000000000000000000000000000000000000000000000000";
    kernel.seed_kind1_for_reply_test(&target, target_author, 100, vec![], "reacted-to note");

    // Seed the reacted-to author's NIP-65 list with a READ-marked inbox relay.
    // `seed_kind10002_for_test` only emits write-marked tags, so the kind:10002
    // is injected directly with an explicit `"read"` marker — that is the relay
    // the resolver routes the inbox copy to.
    const AUTHOR_INBOX_RELAY: &str = "wss://reacted-author-inbox.test";
    // Use the target_author pubkey as the event id — guaranteed valid hex (64
    // hex chars).  The old string "cccck10002inbox" contained 'k', 'i', 'n'
    // which are not valid hex characters; V-70 strengthened
    // `is_structurally_valid()` to check hex chars, so that synthetic event
    // was rejected as Malformed and never entered the store.
    let k10002_id = target_author.to_string();
    kernel.inject_replaceable_event(
        &k10002_id,
        target_author,
        1_700_000_000,
        10002,
        vec![vec![
            "r".to_string(),
            AUTHOR_INBOX_RELAY.to_string(),
            "read".to_string(),
        ]],
        "wss://seed",
        1_700_000_000_000,
    );

    let outbound = react(&id, &mut kernel, &target, "❤", None, &mut Vec::new());

    // The reaction must carry the `p` tag (NIP-25 §1) so the engine has a
    // recipient to resolve at all.
    let event = last_published_event_json(&outbound);
    assert_eq!(
        tags_of(&event),
        vec![
            vec!["e".to_string(), target.clone()],
            vec!["p".to_string(), target_author.to_string()],
        ],
        "reaction must carry a `p` tag naming the reacted-to author for inbox routing"
    );

    // The decisive assertion: an EVENT frame went to the author's READ/inbox
    // relay. This only happens if the NIP-65 resolver consulted the recipient's
    // kind:10002 — the reactor's own write relays do not include this URL.
    let routed_to_inbox = outbound
        .iter()
        .any(|m| m.relay_url == AUTHOR_INBOX_RELAY && m.text.starts_with("[\"EVENT\""));
    assert!(
        routed_to_inbox,
        "reaction must be routed to the reacted-to author's NIP-65 inbox relay \
         ({AUTHOR_INBOX_RELAY}); outbound relays: {:?}",
        outbound.iter().map(|m| &m.relay_url).collect::<Vec<_>>()
    );

    // Sanity: the reactor's own outbox relays are still in the target set —
    // inbox routing augments, never replaces, the author's NIP-65 write fanout.
    for write_url in TEST_WRITE_RELAYS {
        assert!(
            outbound.iter().any(|m| &m.relay_url == write_url),
            "reaction must still fan out to the reactor's NIP-65 write relay {write_url}"
        );
    }
}

#[test]
fn react_to_uncached_author_skips_inbox_routing_gracefully() {
    // D6: when the reacted-to event is uncached, `react` cannot build the `p`
    // tag, so there is no recipient for the resolver to route an inbox copy
    // to. The reaction must still publish to the reactor's own outbox relays —
    // degraded but valid — and must not panic. This is the negative companion
    // to `react_routes_to_reacted_to_author_inbox_relay`.
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);
    let target = "d".repeat(64); // well-formed id, never seeded → author uncached

    let outbound = react(&id, &mut kernel, &target, "❤", None, &mut Vec::new());

    assert!(
        !outbound.is_empty(),
        "react to an uncached event must still publish to the reactor's outbox"
    );
    for write_url in TEST_WRITE_RELAYS {
        assert!(
            outbound.iter().any(|m| &m.relay_url == write_url),
            "uncached target → reaction still fans out to the reactor's write relay {write_url}"
        );
    }
}

// ── follow: unfollow / idempotency / account / pubkey-validation gaps ───────
//
// `follow_publishes_kind3_with_p_tag` above covers only the first add against
// an empty contact list. These pin the rest of `publish::follow`: removal
// from an existing kind:3, idempotent re-add (no duplicate `p` tag), the
// no-account D6 toast for both add and remove, and the malformed-pubkey toast.

/// Seed an existing kind:3 contact list for `author` containing `follows`,
/// using the kernel's verification-free replaceable-event injector so
/// `current_follows` reads it back. `created_at` is well in the past so a
/// subsequent `follow` command (stamped `now_secs()`) supersedes it.
fn seed_contact_list(kernel: &mut Kernel, author: &str, follows: &[&str]) {
    let p_tags: Vec<Vec<String>> = follows
        .iter()
        .map(|p| vec!["p".to_string(), (*p).to_string()])
        .collect();
    kernel.inject_replaceable_event(
        &"3".repeat(64),
        author,
        1_700_000_000,
        3,
        p_tags,
        "wss://seed-relay.test",
        1,
    );
}

#[test]
fn unfollow_removes_pubkey_from_contact_list() {
    // Seed a kind:3 that already follows two pubkeys, then unfollow one.
    // The re-published kind:3 must drop exactly that pubkey and keep the other.
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);
    let author = id.active_pubkey().unwrap();
    let keep = "c".repeat(64);
    let drop = "d".repeat(64);
    seed_contact_list(&mut kernel, &author, &[&keep, &drop]);

    let outbound = follow(&id, &mut kernel, &drop, false, None, &mut Vec::new());
    assert!(!outbound.is_empty(), "unfollow must re-publish the kind:3");
    let event = last_published_event_json(&outbound);
    assert_eq!(event["kind"], 3);
    let p_pubkeys: Vec<String> = tags_of(&event)
        .into_iter()
        .filter(|t| t.first().map(String::as_str) == Some("p"))
        .filter_map(|t| t.get(1).cloned())
        .collect();
    assert!(
        p_pubkeys.contains(&keep),
        "unfollowed list must still contain the kept pubkey"
    );
    assert!(
        !p_pubkeys.contains(&drop),
        "unfollowed pubkey must be gone from the contact list"
    );
    assert_eq!(p_pubkeys.len(), 1, "exactly one follow must remain");
}

#[test]
fn follow_already_followed_is_idempotent_no_duplicate() {
    // Re-following a pubkey already in the kind:3 must not append a duplicate
    // `p` tag (publish.rs:308-311 — the `!any(|p| p == pubkey)` guard).
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);
    let author = id.active_pubkey().unwrap();
    let already = "e".repeat(64);
    seed_contact_list(&mut kernel, &author, &[&already]);

    let outbound = follow(&id, &mut kernel, &already, true, None, &mut Vec::new());
    assert!(!outbound.is_empty(), "follow must re-publish the kind:3");
    let event = last_published_event_json(&outbound);
    let p_pubkeys: Vec<String> = tags_of(&event)
        .into_iter()
        .filter(|t| t.first().map(String::as_str) == Some("p"))
        .filter_map(|t| t.get(1).cloned())
        .collect();
    assert_eq!(
        p_pubkeys,
        vec![already],
        "re-following an existing pubkey must not duplicate the `p` tag"
    );
}

#[test]
fn follow_without_account_toasts_and_no_outbound() {
    // D6: follow with no active account → toast naming the `follow` action.
    let (id, mut kernel) = fresh();
    let target = "f".repeat(64);
    let outbound = follow(&id, &mut kernel, &target, true, None, &mut Vec::new());
    assert!(
        outbound.is_empty(),
        "follow with no active account must produce no outbound frames"
    );
    assert!(kernel
        .last_error_toast_snapshot()
        .is_some_and(|t| t.contains("follow") && t.contains("no active account")));
}

#[test]
fn unfollow_without_account_toasts_with_unfollow_action() {
    // D6: the no-account toast distinguishes add (`follow`) from remove
    // (`unfollow`) — publish.rs:301 picks the action string off `add`.
    let (id, mut kernel) = fresh();
    let target = "f".repeat(64);
    let outbound = follow(&id, &mut kernel, &target, false, None, &mut Vec::new());
    assert!(outbound.is_empty());
    assert!(kernel
        .last_error_toast_snapshot()
        .is_some_and(|t| t.contains("unfollow") && t.contains("no active account")));
}

#[test]
fn follow_malformed_pubkey_toasts_and_refuses() {
    // The follow target must be a 64-hex pubkey. A malformed value is a
    // user-visible error (D6 toast), not a silent no-op — and must not panic.
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);
    let outbound = follow(&id, &mut kernel, "xyz", true, None, &mut Vec::new());
    assert!(
        outbound.is_empty(),
        "follow with a malformed pubkey must produce no outbound frames"
    );
    assert!(kernel
        .last_error_toast_snapshot()
        .is_some_and(|t| t.contains("follow") && t.contains("64-hex")));
    assert!(
        kernel.publish_queue_snapshot().is_empty(),
        "follow with a malformed pubkey must not enqueue a publish"
    );
}

// ── profile update (kind:0 metadata) via the generic publish path ──────────
//
// There is no dedicated profile-update command handler; profile metadata
// updates flow through `publish_unsigned_event` as a generic kind:0 event
// (the same code path `publish_unsigned_event_signs_and_publishes_arbitrary_kind`
// exercises with kind:30023). These pin kind:0 explicitly because it is the
// production-relevant kind for "update display name".

#[test]
fn profile_update_publishes_kind0_metadata_event() {
    // Updating a display name builds a kind:0 metadata event whose JSON
    // content carries the new profile fields; the signer overwrites the
    // pubkey with the active identity's key.
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);
    let active_pubkey = id.active_pubkey().unwrap();
    let unsigned = crate::substrate::UnsignedEvent {
        pubkey: "ignored-by-signer".into(),
        kind: 0,
        tags: Vec::new(),
        content: r#"{"name":"marcus","display_name":"Marcus Webb"}"#.into(),
        created_at: 1_700_000_000,
    };
    let outbound = publish_unsigned_event(&id, &mut kernel, unsigned, None, None, &mut Vec::new());
    assert!(
        !outbound.is_empty(),
        "kind:0 update must produce an EVENT frame"
    );
    let event = last_published_event_json(&outbound);
    assert_eq!(event["kind"], 0, "profile metadata must be kind:0");
    assert_eq!(
        event["pubkey"], active_pubkey,
        "signer must stamp the active identity's pubkey, not the caller's"
    );
    assert!(
        event["content"]
            .as_str()
            .is_some_and(|c| c.contains("Marcus Webb")),
        "kind:0 content must carry the updated display name"
    );
    assert_eq!(kernel.publish_queue_snapshot().last().unwrap().kind, 0);
}

#[test]
fn profile_update_without_account_toasts_and_no_outbound() {
    // D6: a kind:0 metadata update with no active account is a toast, never
    // an exception — the generic publish path can't sign without an identity.
    let (id, mut kernel) = fresh();
    let unsigned = crate::substrate::UnsignedEvent {
        pubkey: "ignored".into(),
        kind: 0,
        tags: Vec::new(),
        content: r#"{"display_name":"Nobody"}"#.into(),
        created_at: 1_700_000_000,
    };
    let outbound = publish_unsigned_event(&id, &mut kernel, unsigned, None, None, &mut Vec::new());
    assert!(
        outbound.is_empty(),
        "profile update with no active account must produce no outbound frames"
    );
    assert!(kernel
        .last_error_toast_snapshot()
        .is_some_and(|t| t.contains("publish") && t.contains("no active account")));
}

#[test]
fn add_and_remove_relay_edits_projection() {
    let (_id, mut kernel) = fresh();
    // T158: add_relay returns Some(url) on success, None on failure.
    let result = add_relay(&mut kernel, "wss://relay.damus.io", "both");
    assert_eq!(result, Some("wss://relay.damus.io".to_string()));
    let result2 = add_relay(&mut kernel, "wss://nos.lol", "write");
    assert_eq!(result2, Some("wss://nos.lol".to_string()));
    assert_eq!(kernel.configured_relays_snapshot().len(), 2);
    // Invalid URL scheme — returns None and sets a toast.
    let bad = add_relay(&mut kernel, "http://bad", "read");
    assert_eq!(bad, None);
    assert_eq!(kernel.configured_relays_snapshot().len(), 2);
    assert!(kernel
        .last_error_toast_snapshot()
        .is_some_and(|t| t.contains("invalid relay URL")));
    // Invalid role — returns None.
    let bad_role = add_relay(&mut kernel, "wss://nos.lol", "superwrite");
    assert_eq!(bad_role, None);
    remove_relay(&mut kernel, "wss://nos.lol");
    assert_eq!(kernel.configured_relays_snapshot().len(), 1);
    assert_eq!(
        kernel.configured_relays_snapshot()[0].url,
        "wss://relay.damus.io"
    );
}

#[test]
fn sign_in_bunker_seeds_handshake_progress() {
    // Stage 3 of NIP-46 wiring: a shape-valid bunker:// URI seeds the
    // snapshot with `"connecting"` so the SwiftUI sign-in flow can render
    // progress immediately. The broker (Stage 4) drives the real handshake
    // and pushes subsequent progress via `BunkerHandshakeProgress`.
    //
    // Stage 4 also added a fallback: if no broker hook is installed, the
    // actor clears the seeded "connecting" stage and surfaces a toast.
    // ADR-0052 §D3 — install a no-op hook into THIS runtime's per-app slot so
    // the test exercises the happy path (no process-global state).
    use std::sync::Arc;

    let (mut id, mut kernel) = fresh();
    id.install_bunker_hook_for_test(Arc::new(|_req| {}));
    let pk = "c".repeat(64);
    sign_in_bunker(
        &mut id,
        &mut kernel,
        &format!("bunker://{pk}?relay=wss://r.example"),
    );
    // D0: handshake state is an app noun — it is written to the identity
    // runtime's shared slot (read by the `"bunker_handshake"` projection),
    // not a typed kernel field.
    let handshake = id.bunker_handshake_for_test().expect("handshake seeded");
    assert_eq!(handshake.stage, "connecting");
    assert!(handshake.message.is_some());
    // No toast on the happy path — the seeded progress is the UX signal.
    assert!(kernel.last_error_toast_snapshot().is_none());
}

#[test]
fn sign_in_bunker_rejects_malformed_uri() {
    let (mut id, mut kernel) = fresh();
    sign_in_bunker(&mut id, &mut kernel, "bunker://nope");
    assert!(kernel
        .last_error_toast_snapshot()
        .is_some_and(|t| t.contains("invalid bunker")));
}

#[test]
fn sign_in_bunker_without_broker_clears_progress_and_toasts() {
    // Stage 4: if no broker hook is installed when a URI arrives, the actor
    // clears the seeded "connecting" stage and surfaces a toast so the user
    // knows the bunker subsystem is missing. In normal flow the broker installs
    // its hook at startup, before any URI can be submitted.
    //
    // ADR-0052 §D3 — the hook is now a PER-APP slot (no process-global), so
    // this test can exercise the *no-hook* path deterministically: a fresh
    // `IdentityRuntime` starts with an empty bunker hook slot. (The old global
    // design could not — its `OnceLock` stayed fired from a sibling test.)
    let (mut id, mut kernel) = fresh();
    // Deliberately install NO hook.
    let pk = "d".repeat(64);
    sign_in_bunker(
        &mut id,
        &mut kernel,
        &format!("bunker://{pk}?relay=wss://r.example"),
    );
    // No broker installed → the seeded "connecting" stage is cleared and a
    // toast naming the missing init call is surfaced (D6: error becomes state).
    assert!(
        id.bunker_handshake_for_test().is_none(),
        "no-hook path must clear the seeded handshake progress"
    );
    assert!(kernel
        .last_error_toast_snapshot()
        .is_some_and(|t| t.contains("broker not initialised")));
}

#[test]
fn snapshot_json_carries_new_projections() {
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);
    let unsigned = crate::substrate::UnsignedEvent {
        pubkey: String::new(),
        kind: 1,
        tags: Vec::new(),
        content: "json shape check".to_string(),
        created_at: 0,
    };
    publish_unsigned_event(&id, &mut kernel, unsigned, None, None, &mut Vec::new());
    add_relay(&mut kernel, "wss://relay.damus.io", "both");
    let json = kernel.make_update_json_for_test(true);
    assert!(json.contains("\"accounts\""));
    assert!(json.contains("\"active_account\""));
    assert!(json.contains("\"last_error_toast\""));
    // D0: the publish cluster (`publish_queue`, `publish_outbox`,
    // `configured_relays`) is no longer a set of typed `KernelSnapshot` fields —
    // all three are kernel-owned built-in entries in the host-extensible
    // `projections` map. They are always present (kernel-owned data, no host
    // registration step), unlike the host-registered `"bunker_handshake"`
    // projection. Decode the map and assert the keys nest under it.
    let parsed: serde_json::Value =
        serde_json::from_str(&json).expect("snapshot must be valid JSON");
    let projections = parsed
        .get("projections")
        .expect("snapshot must carry the projections map once the publish cluster is populated");
    assert!(projections.get("publish_queue").is_some());
    assert!(projections.get("publish_outbox").is_some());
    assert!(projections.get("outbox_summary").is_some());
    assert!(projections.get("configured_relays").is_some());
    let role_options = projections["relay_role_options"]
        .as_array()
        .expect("relay_role_options must be a projection array");
    assert_eq!(role_options[0]["value"].as_str(), Some("both,indexer"));
    assert_eq!(role_options[0]["label"].as_str(), Some("Both + Index"));
    assert_eq!(role_options[0]["tint"].as_str(), Some("accent"));
    assert_eq!(role_options[1]["value"].as_str(), Some("both"));
    assert_eq!(role_options[1]["is_default"].as_bool(), Some(true));
    let relay_rows = projections["configured_relays"]
        .as_array()
        .expect("configured_relays must be a projection array");
    assert!(
        !relay_rows.is_empty(),
        "configured_relays projection must have entries"
    );
    // D0: the views cluster (`profile`, `timeline`, `author_view`,
    // `inserted`, `updated`, `removed`) is no longer a typed `KernelSnapshot`
    // field set — all are kernel-owned built-in entries in the same `projections`
    // map. D5: `timeline`, `inserted`, `updated`, `removed` are present only
    // when `follow_feed_kinds` is non-empty (the shell has called
    // `nmp_app_chirp_open_home_feed`). `profile` is always present.
    // V-112 (ADR-0042): `author_view` / `thread_view` deleted from projections.
    assert!(projections.get("profile").is_some());
    // `timeline` and deltas are absent — no open_contact_feed call above.
    assert!(
        projections.get("timeline").is_none(),
        "D5: timeline must be absent before open_contact_feed"
    );
    assert!(
        projections.get("inserted").is_none(),
        "D5: inserted must be absent before open_contact_feed"
    );
    assert!(
        projections.get("updated").is_none(),
        "D5: updated must be absent before open_contact_feed"
    );
    assert!(
        projections.get("removed").is_none(),
        "D5: removed must be absent before open_contact_feed"
    );
    // V-112 (ADR-0042): `author_view` / `thread_view` deleted from snapshot.
    // `retarget_timeline` no longer calls `kernel.open_author()`.
    assert!(
        projections.get("author_view").is_none(),
        "V-112: author_view must be absent — deleted in ADR-0042 M2 migration"
    );
    assert!(
        projections.get("thread_view").is_none(),
        "V-112: thread_view must be absent — deleted in ADR-0042 M2 migration"
    );
    // The typed `KernelSnapshot` fields must be gone — a shell that still
    // reads them would silently get `null`.
    assert!(parsed.get("profile").is_none());
    assert!(parsed.get("items").is_none());
    assert!(parsed.get("author_view").is_none());
    assert!(parsed.get("thread_view").is_none());
    // D0: NIP-46 bunker handshake is no longer a typed `KernelSnapshot` field
    // — it is surfaced through the built-in `"bunker_handshake"` snapshot
    // projection registered in `nmp_app_new`. A bare `make_update` (no
    // projection registered) therefore does NOT carry the key; the projection
    // path is covered by `snapshot_carries_bunker_handshake_value` in
    // `remote_signer_tests.rs`.
}

// ── T-relay-url-normalize — add_relay canonicalization ───────────────────────

/// T-normalize-cmd-1: `add_relay` with uppercase + trailing slash must return
/// the canonical (lowercased, slash-stripped) URL.
#[test]
fn add_relay_canonicalizes_url() {
    let (_id, mut kernel) = fresh();
    let result = add_relay(&mut kernel, "WSS://Relay.Damus.IO/", "both");
    assert_eq!(
        result,
        Some("wss://relay.damus.io".to_string()),
        "T-normalize-cmd-1: add_relay must return canonical URL (lowercase scheme+host, no empty-path slash)"
    );
    let rows = kernel.configured_relays_snapshot();
    assert_eq!(rows.len(), 1, "exactly one row added");
    assert_eq!(
        rows[0].url, "wss://relay.damus.io",
        "AppRelay must store the canonical URL"
    );
}

/// T-normalize-cmd-2: adding the same relay via two URL-equivalent forms must
/// dedup to a single `AppRelay` (not two rows).
#[test]
fn add_relay_case_slash_variants_dedup_to_one_row() {
    let (_id, mut kernel) = fresh();
    let r1 = add_relay(&mut kernel, "WSS://R.Ex/", "both");
    let r2 = add_relay(&mut kernel, "wss://r.ex", "read");
    assert!(r1.is_some(), "first add must succeed");
    assert!(r2.is_some(), "second add must succeed (role update)");
    let rows = kernel.configured_relays_snapshot();
    assert_eq!(
        rows.len(),
        1,
        "T-normalize-cmd-2: URL-equivalent adds must dedup to one AppRelay, got {:?}",
        rows
    );
    assert_eq!(rows[0].url, "wss://r.ex");
    assert_eq!(rows[0].role, "read", "second add must update the role");
}

/// T-normalize-cmd-3: `remove_relay` with a URL-variant that differs from the
/// add form (trailing slash vs not) must still remove the row.
#[test]
fn remove_relay_canonical_matches_add_form() {
    let (_id, mut kernel) = fresh();
    add_relay(&mut kernel, "wss://r.ex", "both");
    assert_eq!(
        kernel.configured_relays_snapshot().len(),
        1,
        "row must exist after add"
    );
    // Remove with trailing slash (different bytes, same canonical form).
    remove_relay(&mut kernel, "wss://r.ex/");
    assert_eq!(
        kernel.configured_relays_snapshot().len(),
        0,
        "T-normalize-cmd-3: remove_relay with trailing-slash variant must remove the row"
    );
}

// ─── T140 — open_timeline must register M2 interests, not open_author ────────

/// T140 RED test: the `open_timeline()` actor command must register M2
/// `LogicalInterest`s in the lifecycle registry (for the active account's
/// follow set) so that `drain_lifecycle_tick()` emits follow-feed REQ frames.
///
/// Pre-T140: `open_contact_feed` → `open_author` → no follow-feed interests in
/// registry → `drain_lifecycle_tick` returns `Vec::new()`. FAILS.
///
/// Post-T140: `open_contact_feed` pushes per-follow `LogicalInterest`s → the
/// M2 planner compiles them → `drain_lifecycle_tick` returns REQ frame(s) for
/// the followed author's NIP-65 write relay. PASSES.
#[test]
fn t140_open_contact_feed_registers_m2_interests_drain_emits_req() {
    const ALICE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    let (mut id, mut kernel) = fresh();

    // Sign in so `open_timeline` has an active pubkey.
    sign_in_nsec(&mut id, &mut kernel, TEST_NSEC, false);
    let active_pk = id.active_pubkey().expect("active account after sign_in");

    // ALICE has a resolved write relay (via kind:10002 test support helper).
    kernel.seed_kind10002_for_test(ALICE, &["wss://alice-t140.relay/"]);

    // Inject kind:3 for the active account listing ALICE as a follow.
    // This populates `seed_contacts` via `ingest_contacts`.
    let follow_tags = vec![vec!["p".to_string(), ALICE.to_string()]];
    kernel.inject_replaceable_event(
        "0000000000000000000000000000000000000000000000000000000000000001",
        &active_pk,
        2_000,
        3,
        follow_tags,
        "wss://seed.relay/",
        2_000_000,
    );

    // Force the lifecycle selection budget so the compiler routes freely.
    kernel
        .lifecycle_mut()
        .set_selection_budget(usize::MAX, usize::MAX);

    // Call the actor command under test. `open_contact_feed` declares the
    // host kinds {1, 6} (Chirp's home feed) via `set_follow_feed_kinds`,
    // which re-registers the active account's M2 follow-feed interests.
    let _outbound = open_contact_feed(
        &id,
        &mut kernel,
        std::collections::BTreeSet::from([1u32, 6u32]),
    );

    // Drain the M2 planner — must emit follow-feed REQs after T140.
    let frames = kernel.drain_lifecycle_tick();
    let req_urls: Vec<String> = frames
        .iter()
        .filter_map(|f| match f {
            crate::subs::WireFrame::Req { relay_url, .. } => Some(relay_url.clone()),
            _ => None,
        })
        .collect();

    assert!(
        !req_urls.is_empty(),
        "T140: open_contact_feed must register follow-feed M2 interests so \
         drain_lifecycle_tick emits REQ frames (got {} total frames, 0 REQs)",
        frames.len(),
    );
    assert!(
        req_urls.iter().any(|u| u == "wss://alice-t140.relay/"),
        "T140: open_contact_feed REQ must target ALICE's resolved write relay \
         wss://alice-t140.relay/; got urls: {req_urls:?}"
    );
}

// ── open_contact_feed / close_contact_feed (RED tests — Step 1 of TDD) ──────

/// After `open_contact_feed({1,6})` the follow-feed interests are registered;
/// after `close_contact_feed()` they are withdrawn, a CLOSE frame is emitted,
/// `follow_feed_interest_ids` is empty, and `timeline_authors` is empty.
///
/// Verifies the full symmetric lifecycle required by the design: D5 cluster is
/// present after open, absent after close.
#[test]
fn close_contact_feed_withdraws_follow_interests_and_emits_close() {
    const ALICE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    let (mut id, mut kernel) = fresh();

    // Sign in so `open_contact_feed` has an active pubkey.
    sign_in_nsec(&mut id, &mut kernel, TEST_NSEC, false);
    let active_pk = id.active_pubkey().expect("active account after sign_in");

    // ALICE has a resolved write relay.
    kernel.seed_kind10002_for_test(ALICE, &["wss://alice-close-test.relay/"]);

    // Inject kind:3 for the active account listing ALICE as a follow.
    let follow_tags = vec![vec!["p".to_string(), ALICE.to_string()]];
    kernel.inject_replaceable_event(
        "0000000000000000000000000000000000000000000000000000000000000001",
        &active_pk,
        2_000,
        3,
        follow_tags,
        "wss://seed.relay/",
        2_000_000,
    );

    // Force the lifecycle selection budget so the compiler routes freely.
    kernel
        .lifecycle_mut()
        .set_selection_budget(usize::MAX, usize::MAX);

    // Open with kinds {1, 6}: interests should be registered.
    let _outbound = open_contact_feed(
        &id,
        &mut kernel,
        std::collections::BTreeSet::from([1u32, 6u32]),
    );

    // Drain — must emit REQ frames for ALICE after open.
    let open_frames = kernel.drain_lifecycle_tick();
    let req_count_after_open = open_frames
        .iter()
        .filter(|f| matches!(f, crate::subs::WireFrame::Req { .. }))
        .count();
    assert!(
        req_count_after_open > 0,
        "open_contact_feed must register follow-feed interests (got 0 REQs after open)"
    );

    // Close: interests should be withdrawn, CLOSE frames emitted.
    let _close_out = close_contact_feed(&id, &mut kernel);

    let close_frames = kernel.drain_lifecycle_tick();
    let close_count = close_frames
        .iter()
        .filter(|f| matches!(f, crate::subs::WireFrame::Close { .. }))
        .count();
    assert!(
        close_count > 0,
        "close_contact_feed must emit CLOSE frames (got 0 CLOSEs after close)"
    );

    // After close the follow-feed interest registry must be empty.
    assert!(
        kernel.follow_feed_interest_ids.is_empty(),
        "close_contact_feed must clear follow_feed_interest_ids"
    );

    // timeline_authors must be cleared as well (the kernel CLEAR branch).
    assert!(
        kernel.timeline_authors.is_empty(),
        "close_contact_feed must clear timeline_authors"
    );

    // D5 symmetry: take a snapshot after close and assert that the timeline /
    // delta-projection cluster is absent — mirroring the pre-open assertions
    // at tests.rs:1932-1950.  The headline design claim is that D5 gating is
    // symmetric: the cluster appears on open and disappears again on close.
    let post_close_json = kernel.make_update_json_for_test(true);
    let post_close: serde_json::Value =
        serde_json::from_str(&post_close_json).expect("post-close snapshot must be valid JSON");
    let post_projections = post_close
        .get("projections")
        .expect("snapshot must carry the projections map");
    assert!(
        post_projections.get("timeline").is_none(),
        "D5: timeline must be absent after close_contact_feed"
    );
    assert!(
        post_projections.get("inserted").is_none(),
        "D5: inserted must be absent after close_contact_feed"
    );
    assert!(
        post_projections.get("updated").is_none(),
        "D5: updated must be absent after close_contact_feed"
    );
    assert!(
        post_projections.get("removed").is_none(),
        "D5: removed must be absent after close_contact_feed"
    );
}

/// `open_contact_feed` with an empty kinds set acts as a clear (same as close):
/// any previously registered follow-feed interests are withdrawn.
#[test]
fn open_contact_feed_empty_kinds_is_clear() {
    const BOB: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    let (mut id, mut kernel) = fresh();
    sign_in_nsec(&mut id, &mut kernel, TEST_NSEC, false);
    let active_pk = id.active_pubkey().expect("active account after sign_in");

    kernel.seed_kind10002_for_test(BOB, &["wss://bob-empty-test.relay/"]);

    let follow_tags = vec![vec!["p".to_string(), BOB.to_string()]];
    kernel.inject_replaceable_event(
        "0000000000000000000000000000000000000000000000000000000000000002",
        &active_pk,
        2_000,
        3,
        follow_tags,
        "wss://seed.relay/",
        2_000_000,
    );

    kernel
        .lifecycle_mut()
        .set_selection_budget(usize::MAX, usize::MAX);

    // First open with non-empty kinds so there is something to clear.
    let _ = open_contact_feed(
        &id,
        &mut kernel,
        std::collections::BTreeSet::from([1u32, 6u32]),
    );
    let _ = kernel.drain_lifecycle_tick();

    // Now open with empty kinds set — behaves as clear.
    let _ = open_contact_feed(&id, &mut kernel, std::collections::BTreeSet::new());
    let _ = kernel.drain_lifecycle_tick();

    assert!(
        kernel.follow_feed_interest_ids.is_empty(),
        "open_contact_feed with empty kinds must clear follow_feed_interest_ids"
    );
}

// Issue #1246 kind:3 full-edit follow tests, extracted to keep this file under
// the file-size hard cap. Child module so `use super::*` inherits the shared
// test helpers (`fresh`, `seed_contact_list`, `follow`, ...).
#[path = "tests_follow_kind3_fulledit.rs"]
mod tests_follow_kind3_fulledit;
