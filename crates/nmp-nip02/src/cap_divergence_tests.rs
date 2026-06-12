//! Cross-observer follow-cap conformance (cap-divergence regression).
//!
//! The kernel caps a kind:3 contact list at `TIMELINE_AUTHOR_LIMIT` (500)
//! follows in tag order (`ingest_contacts` →
//! `nmp_core::tags::capped_contact_follows`). Two `KernelEventObserver`s in
//! this crate rebuild the active account's follow set independently:
//! [`ActiveFollowSet`] (the predicate producer) and [`FollowListProjection`]
//! (the `nmp.follow_list` snapshot). Before this regression test both rebuilt
//! an **uncapped** set, so for a >500-follow account:
//!
//! * `ActiveFollowSet::predicate` qualified authors the router never
//!   subscribes to (the kernel only REQs the first 500), and
//! * `FollowListProjection` advertised follows the feed can never serve.
//!
//! Both observers must now derive the *identical* capped set the kernel
//! derives — same membership, same 500-element bound — by routing through the
//! one shared pure function `nmp_core::tags::capped_contact_follows`. This file
//! pins that invariant with a 600-`p`-tag kind:3.

use std::sync::{Arc, Mutex};

use nmp_core::kinds::KIND_CONTACT_LIST;
use nmp_core::substrate::KernelEvent;
use nmp_core::tags::{capped_contact_follows, TIMELINE_AUTHOR_LIMIT};
use nmp_core::KernelEventObserver;

use crate::active_follow_set::ActiveFollowSet;
use crate::projection::FollowListProjection;

/// Deterministic, distinct, valid 64-hex pubkey for index `i`.
///
/// Encodes `i` as 16 hex chars (big-endian u64) followed by a fixed 48-hex
/// tail. Distinct `i` ⇒ distinct pubkey; every output passes
/// `is_hex_pubkey` (64 lowercase hex chars).
fn hex_pubkey(i: usize) -> String {
    format!("{:016x}{}", i as u64, "0123456789abcdef0123456789abcdef0123456789abcdef")
}

/// The active account's own pubkey — distinct from every `hex_pubkey(i)`
/// (which all begin with a zero-padded counter; this begins with `ff…`).
fn active_pubkey() -> String {
    format!("ffffffffffffffff{}", "0123456789abcdef0123456789abcdef0123456789abcdef")
}

/// Build a kind:3 authored by `author` carrying one `["p", pk]` tag per entry
/// of `follows`, in order.
fn kind3(author: &str, follows: &[String]) -> KernelEvent {
    let tags: Vec<Vec<String>> = follows
        .iter()
        .map(|pk| vec!["p".to_string(), pk.clone()])
        .collect();
    KernelEvent {
        id: nmp_core::substrate::EventId::from(
            "0000000000000000000000000000000000000000000000000000000000000001".to_string(),
        ),
        author: author.to_string(),
        kind: KIND_CONTACT_LIST,
        created_at: 100,
        tags,
        content: String::new(),
    }
}

/// 600 distinct valid-hex follows in tag order.
fn six_hundred_follows() -> Vec<String> {
    (0..600).map(hex_pubkey).collect()
}

#[test]
fn shared_cap_function_takes_first_500_in_tag_order() {
    // The oracle: the same derivation the kernel applies in `ingest_contacts`.
    let follows = six_hundred_follows();
    let event = kind3(&active_pubkey(), &follows);
    let capped = capped_contact_follows(&event.tags);

    assert_eq!(capped.len(), TIMELINE_AUTHOR_LIMIT, "must cap at 500");
    // First-500-in-tag-order, no reordering, no dedup pass dropping order.
    assert_eq!(
        capped,
        follows[..TIMELINE_AUTHOR_LIMIT].to_vec(),
        "capped set must be the first 500 p-tags in document order"
    );
    // The 501st..600th follows are excluded.
    for dropped in &follows[TIMELINE_AUTHOR_LIMIT..] {
        assert!(
            !capped.contains(dropped),
            "follow beyond the cap must be excluded"
        );
    }
}

#[test]
fn active_follow_set_caps_at_500_matching_kernel() {
    let me = active_pubkey();
    let follows = six_hundred_follows();
    let set = ActiveFollowSet::new(Arc::new(Mutex::new(Some(me.clone()))));
    set.on_kernel_event(&kind3(&me, &follows));

    let kernel_capped = capped_contact_follows(&kind3(&me, &follows).tags);

    let observed: Vec<String> = set.follows();
    // ActiveFollowSet self-includes the active account → exactly cap + 1.
    assert_eq!(
        observed.len(),
        TIMELINE_AUTHOR_LIMIT + 1,
        "ActiveFollowSet must hold the 500 capped follows plus self, not 600+1"
    );
    assert!(observed.contains(&me), "self-inclusion preserved");

    // Same membership as the kernel's capped set (modulo self-inclusion).
    for pk in &kernel_capped {
        assert!(
            observed.contains(pk),
            "every kernel-capped follow must be a member"
        );
    }
    // The dropped tail must NOT qualify the predicate.
    let predicate = set.predicate();
    for dropped in &follows[TIMELINE_AUTHOR_LIMIT..] {
        assert!(
            !predicate(dropped),
            "predicate must reject authors beyond the 500 cap"
        );
    }
    // A within-cap follow does qualify.
    assert!(predicate(&follows[0]));
    assert!(predicate(&follows[TIMELINE_AUTHOR_LIMIT - 1]));
}

#[test]
fn follow_list_projection_caps_at_500_matching_kernel() {
    let me = active_pubkey();
    let follows = six_hundred_follows();
    let proj = FollowListProjection::new(Arc::new(Mutex::new(Some(me.clone()))));
    proj.on_kernel_event(&kind3(&me, &follows));

    let kernel_capped = capped_contact_follows(&kind3(&me, &follows).tags);

    let snap = proj.snapshot();
    assert_eq!(
        snap.follows.len(),
        TIMELINE_AUTHOR_LIMIT,
        "FollowListProjection must advertise exactly the 500 capped follows, not 600"
    );
    // Identical membership AND order to the kernel's capped set.
    let advertised: Vec<String> = snap.follows.iter().map(|e| e.pubkey.clone()).collect();
    assert_eq!(
        advertised, kernel_capped,
        "projection must mirror the kernel's first-500-in-tag-order set exactly"
    );
}

#[test]
fn both_observers_agree_on_capped_membership() {
    // The whole point of the fix: the predicate producer and the snapshot
    // projection must never diverge on which follows count.
    let me = active_pubkey();
    let follows = six_hundred_follows();

    let set = ActiveFollowSet::new(Arc::new(Mutex::new(Some(me.clone()))));
    set.on_kernel_event(&kind3(&me, &follows));
    let proj = FollowListProjection::new(Arc::new(Mutex::new(Some(me.clone()))));
    proj.on_kernel_event(&kind3(&me, &follows));

    let predicate = set.predicate();
    for entry in proj.snapshot().follows {
        assert!(
            predicate(&entry.pubkey),
            "every follow the projection advertises must satisfy the predicate"
        );
    }
}
