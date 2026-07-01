use std::sync::{Arc, Mutex};

use super::{
    simple_groups_from_tags, SimpleGroupListProjection, SimpleGroupListSourceEffect, SimpleGroupRef,
};
use nmp_core::substrate::{EventId, KernelEvent};
use nmp_core::ObservedProjectionSink;
use nmp_kinds::KIND_SIMPLE_GROUPS;

const ALICE: &str = "aa11223344556677889900aabbccddeeff00112233445566778899aabbccddee";
const BOB: &str = "bb11223344556677889900aabbccddeeff00112233445566778899aabbccddff";

fn simple_group_event(author: &str, groups: &[(&str, &str)], created_at: u64) -> KernelEvent {
    let tags = groups
        .iter()
        .map(|(local, relay)| {
            vec![
                "group".to_string(),
                (*local).to_string(),
                (*relay).to_string(),
            ]
        })
        .collect();
    KernelEvent {
        id: EventId::from("01".repeat(32)),
        author: author.to_string(),
        kind: KIND_SIMPLE_GROUPS,
        created_at,
        tags,
        content: String::new(),
        relay_provenance: Vec::new(),
    }
}

fn effects(proj: &SimpleGroupListProjection) -> Arc<Mutex<Vec<SimpleGroupListSourceEffect>>> {
    let effects = Arc::new(Mutex::new(Vec::new()));
    let effects_cb = Arc::clone(&effects);
    proj.on_source_effect(Box::new(move |effect| {
        effects_cb.lock().unwrap().push(effect.clone());
    }));
    effects
}

#[test]
fn parser_extracts_public_group_tags() {
    let groups = simple_groups_from_tags(&[
        vec![
            "group".to_string(),
            "room-a".to_string(),
            "WSS://Groups.Example/".to_string(),
            "Room A".to_string(),
        ],
        vec!["group".to_string(), "".to_string(), "wss://bad".to_string()],
        vec![
            "group".to_string(),
            "room-b".to_string(),
            "https://bad".to_string(),
        ],
        vec!["r".to_string(), "wss://groups.example".to_string()],
    ]);

    assert_eq!(
        groups,
        [SimpleGroupRef::new("room-a", "wss://groups.example")]
            .into_iter()
            .collect()
    );
}

#[test]
fn groups_resolve_for_active_owner() {
    let slot = Arc::new(Mutex::new(Some(ALICE.to_string())));
    let proj = SimpleGroupListProjection::new(slot);
    proj.on_kernel_event(&simple_group_event(
        ALICE,
        &[("room-a", "wss://relay-a"), ("room-b", "wss://relay-b")],
        10,
    ));

    assert_eq!(
        proj.groups(),
        [
            SimpleGroupRef::new("room-a", "wss://relay-a"),
            SimpleGroupRef::new("room-b", "wss://relay-b")
        ]
        .into_iter()
        .collect()
    );
}

#[test]
fn non_active_owner_is_ignored() {
    let slot = Arc::new(Mutex::new(Some(ALICE.to_string())));
    let proj = SimpleGroupListProjection::new(slot);
    proj.on_kernel_event(&simple_group_event(BOB, &[("room-a", "wss://relay-a")], 10));
    assert!(proj.groups().is_empty());
}

#[test]
fn newer_event_replaces_group_set() {
    let slot = Arc::new(Mutex::new(Some(ALICE.to_string())));
    let proj = SimpleGroupListProjection::new(slot);
    proj.on_kernel_event(&simple_group_event(
        ALICE,
        &[("room-a", "wss://relay-a")],
        10,
    ));
    proj.on_kernel_event(&simple_group_event(
        ALICE,
        &[("room-b", "wss://relay-b")],
        20,
    ));

    assert_eq!(
        proj.groups(),
        [SimpleGroupRef::new("room-b", "wss://relay-b")]
            .into_iter()
            .collect()
    );
}

#[test]
fn older_event_does_not_replace_or_fire_source_effect() {
    let slot = Arc::new(Mutex::new(Some(ALICE.to_string())));
    let proj = SimpleGroupListProjection::new(slot);
    let effects = effects(&proj);

    proj.on_kernel_event(&simple_group_event(
        ALICE,
        &[("room-a", "wss://relay-a")],
        20,
    ));
    proj.on_kernel_event(&simple_group_event(
        ALICE,
        &[("room-b", "wss://relay-b")],
        10,
    ));

    assert_eq!(
        proj.groups(),
        [SimpleGroupRef::new("room-a", "wss://relay-a")]
            .into_iter()
            .collect()
    );
    assert_eq!(effects.lock().unwrap().len(), 1);
}

#[test]
fn account_switch_emits_source_effect_and_clears_visible_groups() {
    let slot = Arc::new(Mutex::new(Some(ALICE.to_string())));
    let proj = SimpleGroupListProjection::new(Arc::clone(&slot));
    let effects = effects(&proj);

    proj.on_kernel_event(&simple_group_event(
        ALICE,
        &[("room-a", "wss://relay-a")],
        10,
    ));
    assert_eq!(effects.lock().unwrap().len(), 1);

    *slot.lock().unwrap() = Some(BOB.to_string());
    proj.notify_account_changed();

    assert!(proj.groups().is_empty());
    assert_eq!(effects.lock().unwrap().len(), 2);
}
