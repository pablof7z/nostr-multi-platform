//! Unit proofs for the closed-data custom feed-policy registry.

use super::*;
use crate::params::{CustomAdmissionId, CustomOrderId, CustomSourceId, TagTerm};

fn source_id(s: &str) -> CustomSourceId {
    CustomSourceId(s.to_string())
}

fn admission_id(s: &str) -> CustomAdmissionId {
    CustomAdmissionId(s.to_string())
}

fn order_id(s: &str) -> CustomOrderId {
    CustomOrderId(s.to_string())
}

fn tag_scope(term: &str) -> FeedScope {
    FeedScope::Tag {
        term: TagTerm(term.to_string()),
    }
}

#[test]
fn unregistered_ids_resolve_to_none_fail_closed() {
    let reg = CustomFeedPolicyRegistry::default();

    assert!(reg.get_source(&source_id("missing")).is_none());
    assert!(reg.get_admission(&admission_id("missing")).is_none());
    assert!(reg.get_order(&order_id("missing")).is_none());
    assert!(!reg.is_source_registered(&source_id("missing")));
    assert!(!reg.is_admission_registered(&admission_id("missing")));
    assert!(!reg.is_order_registered(&order_id("missing")));
    assert!(reg.is_empty());
}

#[test]
fn registered_ids_resolve_only_for_their_phase() {
    let reg = CustomFeedPolicyRegistry::default();
    let source = CustomSourceDef::new(tag_scope("rust"));
    let admission = CustomAdmissionDef::new(tag_scope("nostr"));
    let order = CustomOrderDef::new(FeedOrder::NewestByFeedPosition);

    assert!(reg.register_source(source_id("timeline"), source.clone()));
    assert!(reg.register_admission(admission_id("safe"), admission.clone()));
    assert!(reg.register_order(order_id("engagement"), order.clone()));

    assert_eq!(reg.get_source(&source_id("timeline")), Some(source));
    assert_eq!(reg.get_admission(&admission_id("safe")), Some(admission));
    assert_eq!(reg.get_order(&order_id("engagement")), Some(order));
    assert_eq!(reg.len(), 3);
}

#[test]
fn register_is_register_once_immutable_no_overwrite() {
    let reg = CustomFeedPolicyRegistry::default();
    assert!(reg.register_source(
        source_id("timeline"),
        CustomSourceDef::new(tag_scope("rust"))
    ));
    assert!(
        !reg.register_source(
            source_id("timeline"),
            CustomSourceDef::new(tag_scope("nostr"))
        ),
        "re-register under a live id is rejected"
    );
    assert_eq!(reg.len(), 1);
    assert_eq!(
        reg.get_source(&source_id("timeline")).map(|d| d.source),
        Some(tag_scope("rust")),
        "the first definition stands"
    );
}

#[test]
fn definitions_carry_phase_specific_contracts() {
    let source = CustomSourceDef::new(tag_scope("source"));
    let admission = CustomAdmissionDef::new(tag_scope("gate"));
    let order = CustomOrderDef::new(FeedOrder::OldestByFeedPosition);

    assert_eq!(source.source, tag_scope("source"));
    assert_eq!(admission.gate, tag_scope("gate"));
    assert_eq!(order.order, FeedOrder::OldestByFeedPosition);
}
