use super::*;
use nmp_core::substrate::{EventId, KernelEvent};

fn projection_for(active: Option<&str>) -> MuteListProjection {
    let slot = Arc::new(Mutex::new(active.map(|s| s.to_string())));
    MuteListProjection::new(slot)
}

fn mute_event(author: &str, p_tags: &[&str], e_tags: &[&str]) -> KernelEvent {
    let mut tags: Vec<Vec<String>> = p_tags
        .iter()
        .map(|pk| vec!["p".to_string(), pk.to_string()])
        .collect();
    for eid in e_tags {
        tags.push(vec!["e".to_string(), eid.to_string()]);
    }
    KernelEvent {
        id: EventId::from(
            "0000000000000000000000000000000000000000000000000000000000000001".to_string(),
        ),
        author: author.to_string(),
        kind: 10000,
        created_at: 100,
        tags,
        content: String::new(),
        relay_provenance: Vec::new(),
    }
}

const ALICE: &str = "aa11223344556677889900aabbccddeeff00112233445566778899aabbccddee";
const BOB: &str = "bb11223344556677889900aabbccddeeff00112233445566778899aabbccddff";
const CAROL: &str = "cc11223344556677889900aabbccddeeff00112233445566778899aabbccddee";
const EID_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

#[test]
fn empty_when_no_active_account() {
    let proj = projection_for(None);
    assert!(!proj.is_suppressed_author(BOB));
    assert!(!proj.is_suppressed_event(EID_A));
}

#[test]
fn empty_when_no_kind10000_received() {
    let proj = projection_for(Some(ALICE));
    assert!(!proj.is_suppressed_author(BOB));
}

#[test]
fn non_kind10000_event_is_ignored() {
    let proj = projection_for(Some(ALICE));
    let mut ev = mute_event(ALICE, &[BOB], &[]);
    ev.kind = 1;
    proj.on_kernel_event(&ev);
    assert!(!proj.is_suppressed_author(BOB));
    assert_eq!(proj.muted_pubkey_count(), 0);
}

#[test]
fn kind10000_for_other_account_is_ignored() {
    let proj = projection_for(Some(ALICE));
    proj.on_kernel_event(&mute_event(CAROL, &[BOB], &[]));
    assert!(!proj.is_suppressed_author(BOB));
    assert_eq!(proj.muted_pubkey_count(), 0);
}

#[test]
fn kind10000_for_active_account_suppresses_muted_author() {
    let proj = projection_for(Some(ALICE));
    proj.on_kernel_event(&mute_event(ALICE, &[BOB], &[]));
    assert!(proj.is_suppressed_author(BOB));
    assert!(!proj.is_suppressed_author(CAROL));
}

#[test]
fn kind10000_for_active_account_suppresses_muted_event_id() {
    let proj = projection_for(Some(ALICE));
    proj.on_kernel_event(&mute_event(ALICE, &[], &[EID_A]));
    assert!(proj.is_suppressed_event(EID_A));
    assert!(!proj.is_suppressed_event("other_event_id"));
}

#[test]
fn newer_kind10000_replaces_older_mute_list() {
    let proj = projection_for(Some(ALICE));
    proj.on_kernel_event(&mute_event(ALICE, &[BOB], &[]));
    assert!(proj.is_suppressed_author(BOB));
    proj.on_kernel_event(&mute_event(ALICE, &[CAROL], &[]));
    assert!(
        !proj.is_suppressed_author(BOB),
        "Bob should no longer be muted"
    );
    assert!(proj.is_suppressed_author(CAROL));
}

#[test]
fn muted_pubkeys_exposes_public_p_tags_as_pubkey_source() {
    let proj = projection_for(Some(ALICE));
    proj.on_kernel_event(&mute_event(ALICE, &[BOB, CAROL], &[EID_A]));
    let members = proj.muted_pubkeys();
    assert!(members.contains(BOB));
    assert!(members.contains(CAROL));
    assert_eq!(members.len(), 2);
}

#[test]
fn empty_replacement_clears_pubkey_source_members() {
    let proj = projection_for(Some(ALICE));
    proj.on_kernel_event(&mute_event(ALICE, &[BOB], &[]));
    assert!(proj.muted_pubkeys().contains(BOB));
    proj.on_kernel_event(&mute_event(ALICE, &[], &[]));
    assert!(proj.muted_pubkeys().is_empty());
}

#[test]
fn multiple_muted_pubkeys_all_suppressed() {
    let proj = projection_for(Some(ALICE));
    proj.on_kernel_event(&mute_event(ALICE, &[BOB, CAROL], &[]));
    assert!(proj.is_suppressed_author(BOB));
    assert!(proj.is_suppressed_author(CAROL));
    assert_eq!(proj.muted_pubkey_count(), 2);
}

#[test]
fn snapshot_json_reflects_mute_list() {
    let proj = projection_for(Some(ALICE));
    proj.on_kernel_event(&mute_event(ALICE, &[BOB], &[EID_A]));
    let snap = proj.snapshot();
    assert_eq!(snap.muted_pubkeys, vec![BOB]);
    assert_eq!(snap.muted_event_ids, vec![EID_A]);
}

#[test]
fn account_switch_clears_previous_mute_set() {
    let slot = Arc::new(Mutex::new(Some(ALICE.to_string())));
    let proj = MuteListProjection::new(Arc::clone(&slot));

    proj.on_kernel_event(&mute_event(ALICE, &[BOB], &[]));
    assert!(
        proj.is_suppressed_author(BOB),
        "Bob should be suppressed while Alice is active"
    );

    *slot.lock().unwrap() = Some(CAROL.to_string());

    assert!(
        !proj.is_suppressed_author(BOB),
        "after switch to Carol (who has no kind:10000), Alice's stale mutes \
         must not suppress Bob"
    );
    assert!(
        proj.muted_pubkeys().is_empty(),
        "pubkey-source reads must fail closed after account switch"
    );

    *slot.lock().unwrap() = None;
    assert!(
        !proj.is_suppressed_author(BOB),
        "after logout, nobody's mutes should be active"
    );
}

#[test]
fn no_active_account_drops_all_inserts() {
    let proj = projection_for(None);
    proj.on_kernel_event(&mute_event(ALICE, &[BOB], &[]));
    assert_eq!(proj.muted_pubkey_count(), 0);
}
