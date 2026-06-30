//! Rung 2 — NIP-19/21 references (every entity) + the entity-class allow-list
//! (the `DisallowedScope` refusal for a valid ref outside the requested scopes).

use nmp_nip19::{
    encode_naddr, encode_nevent, encode_note, encode_nprofile, encode_npub, NaddrData, NeventData,
    NprofileData,
};
use nmp_core::substrate::{InputIntentRejection, InputIntentTarget, InputScopeId};

use super::{
    classify_bare, expect_rejection, expect_single, longform_scope, notes_scope, profiles_scope,
    req, EVID, PK,
};

#[test]
fn bare_npub_routes_to_directref_profile() {
    let npub = encode_npub(PK).unwrap();
    let r = req(&npub, vec![profiles_scope()]);
    let cand = expect_single(classify_bare(&r));
    assert_eq!(cand.scope, InputScopeId::nostr_ref());
    assert_eq!(cand.target, InputIntentTarget::DirectRef { uri: npub });
}

#[test]
fn nostr_prefixed_npub_routes_to_directref() {
    let npub = encode_npub(PK).unwrap();
    let uri = format!("nostr:{npub}");
    let r = req(&uri, vec![profiles_scope()]);
    let cand = expect_single(classify_bare(&r));
    assert_eq!(cand.target, InputIntentTarget::DirectRef { uri });
}

#[test]
fn nprofile_routes_to_directref_profile() {
    let nprofile = encode_nprofile(&NprofileData {
        pubkey: PK.to_string(),
        relays: vec!["wss://relay.example.com".to_string()],
    })
    .unwrap();
    let r = req(&nprofile, vec![profiles_scope()]);
    let cand = expect_single(classify_bare(&r));
    assert_eq!(cand.scope, InputScopeId::nostr_ref());
    assert!(matches!(cand.target, InputIntentTarget::DirectRef { .. }));
}

#[test]
fn note_routes_to_directref_under_notes_scope() {
    let note = encode_note(EVID).unwrap();
    let r = req(&note, vec![notes_scope()]);
    let cand = expect_single(classify_bare(&r));
    assert_eq!(cand.target, InputIntentTarget::DirectRef { uri: note });
}

#[test]
fn nevent_routes_to_directref_under_notes_scope() {
    let nevent = encode_nevent(&NeventData {
        event_id: EVID.to_string(),
        relays: vec![],
        author: Some(PK.to_string()),
        kind: Some(1),
    })
    .unwrap();
    let r = req(&nevent, vec![notes_scope()]);
    let cand = expect_single(classify_bare(&r));
    assert!(matches!(cand.target, InputIntentTarget::DirectRef { .. }));
}

#[test]
fn naddr_routes_to_directref_under_longform_scope() {
    let naddr = encode_naddr(&NaddrData {
        identifier: "my-article".to_string(),
        pubkey: PK.to_string(),
        kind: 30_023,
        relays: vec![],
    })
    .unwrap();
    let r = req(&naddr, vec![longform_scope()]);
    let cand = expect_single(classify_bare(&r));
    assert!(matches!(cand.target, InputIntentTarget::DirectRef { .. }));
}

#[test]
fn explicit_nostr_ref_scope_allows_any_entity_class() {
    let naddr = encode_naddr(&NaddrData {
        identifier: "x".to_string(),
        pubkey: PK.to_string(),
        kind: 30_023,
        relays: vec![],
    })
    .unwrap();
    let r = req(&naddr, vec![InputScopeId::nostr_ref()]);
    let cand = expect_single(classify_bare(&r));
    assert!(matches!(cand.target, InputIntentTarget::DirectRef { .. }));
}

#[test]
fn naddr_in_users_only_scope_set_is_disallowed() {
    let naddr = encode_naddr(&NaddrData {
        identifier: "my-article".to_string(),
        pubkey: PK.to_string(),
        kind: 30_023,
        relays: vec![],
    })
    .unwrap();
    let r = req(&naddr, vec![profiles_scope()]);
    assert_eq!(
        expect_rejection(classify_bare(&r)),
        InputIntentRejection::DisallowedScope {
            scope: InputScopeId::nostr_ref()
        }
    );
}

#[test]
fn npub_in_notes_only_scope_set_is_disallowed() {
    let npub = encode_npub(PK).unwrap();
    let r = req(&npub, vec![notes_scope()]);
    assert_eq!(
        expect_rejection(classify_bare(&r)),
        InputIntentRejection::DisallowedScope {
            scope: InputScopeId::nostr_ref()
        }
    );
}
