//! Build `(SubIdentity, LogicalInterest)` pairs for `open_interest` /
//! `close_interest` commands.
//!
//! Always-compiled (not gated behind `#[cfg(feature = "native")]`) so the
//! wasm32 `KernelReducer` surface can call it as well as the native actor.
//! The native actor delegates through
//! `actor::dispatch::build_open_interest`, which calls [`build_interest_pair`]
//! directly.

use crate::planner::{InterestId, InterestLifecycle, InterestScope, LogicalInterest};
use crate::subs::sub_key::{SubIdentity, SubKey, SubOwnerKey, SubScope};

/// M2 (ADR-0076) — build the `(SubIdentity, LogicalInterest)` pair for an
/// `OpenInterest` / `CloseInterest` command from the raw FFI arguments.
///
/// Shared by both arms so an open and its matching close land on the SAME
/// registry `(scope, key)` slot: the `SubKey` is the hash of the parsed
/// `InterestShape` (order-independent — see `InterestShape::from_filter_json`),
/// the `SubOwnerKey` is the hash of `consumer_id`, and the `SubScope` folds
/// `ActiveAccount` → `Global` (the registry's existing `legacy_scope`
/// convention — the registry's `SubScope` has no `ActiveAccount` variant, so
/// the real `InterestScope::ActiveAccount` rides on the `LogicalInterest`
/// instead, where the compiler reads it to re-route on account switch).
///
/// `scope == 0` → `InterestScope::ActiveAccount` (re-route on account switch).
/// Any other value → `InterestScope::Global`.
///
/// `relay_pin` — when `Some`, the parsed shape's `relay_pin` is set to that
/// host so the planner's relay-pin lane (`case_e_relay_pinned`) routes the
/// interest to exactly that relay, bypassing NIP-65 outbox routing (the
/// substrate-generic mechanism NIP-50 search and NIP-29 groups both need). The
/// pin participates in the `InterestShape` hash, so a pinned open and its
/// matching pinned close land on the same registry slot, while opens pinned to
/// different relays occupy distinct slots. `None` leaves the shape unpinned
/// (the normal outbox-routed path).
///
/// `is_indexer_discovery` opts sparse global reads into the planner's
/// indexer-discovery relay lane. It participates in the registry key because
/// `EnsureAbsent` must not let a non-indexer open mask a later indexer open of
/// the same filter.
///
/// `lifecycle` selects the compiled REQ's close semantics
/// ([`InterestLifecycle::Tailing`] stays open after EOSE;
/// [`InterestLifecycle::OneShot`] CLOSEs on EOSE). It rides the
/// `LogicalInterest` into the compiler + wire-emitter, which already honour both
/// lifecycles (`kernel/requests/mod.rs` registers the wire sub persistent only
/// when `Tailing`; `kernel/ingest/eose.rs` CLOSEs + evicts everything else). It
/// deliberately does NOT participate in the registry key: the key encodes what
/// routes the sub (shape + scope + indexer-discovery), not when it closes.
///
/// Returns `None` when `filter_json` is not a valid NIP-01 filter object
/// (D6 — the caller treats this as a silent no-op).
pub(crate) fn build_interest_pair(
    filter_json: &str,
    consumer_id: &str,
    scope: u32,
    relay_pin: Option<&str>,
    is_indexer_discovery: bool,
    lifecycle: InterestLifecycle,
) -> Option<(SubIdentity, LogicalInterest)> {
    let mut shape = crate::planner::InterestShape::from_filter_json(filter_json)?;
    shape.relay_pin = relay_pin.map(str::to_string);

    // `0` = ActiveAccount (re-route on switch), anything else = Global.
    let interest_scope = if scope == 0 {
        InterestScope::ActiveAccount
    } else {
        InterestScope::Global
    };

    // Registry key: the SubScope mirrors `InterestRegistry::legacy_scope`
    // (ActiveAccount shares the Global slot space until per-account isolation
    // resolves the active pubkey). The real account-context lives on the
    // LogicalInterest below.
    let sub_scope = SubScope::Global;
    // Fold the scope discriminant into the key so an ActiveAccount and a Global
    // open of the *same* filter never collide on one slot (they route
    // differently).
    let mut key_builder = SubKey::builder("open-interest").with(&shape).with(scope);
    if is_indexer_discovery {
        key_builder = key_builder.with("indexer-discovery");
    }
    let key = key_builder.finish();
    let identity = SubIdentity::new(SubOwnerKey::new(consumer_id), key, sub_scope);

    let interest = LogicalInterest {
        id: InterestId(key.0),
        scope: interest_scope,
        shape,
        // Lifecycle is caller-selected (M2 threaded #2948): feed/interest opens
        // pass `Tailing`; the read-demand path passes the demand's own lifecycle
        // so a OneShot collection CLOSEs on EOSE.
        lifecycle,
        is_indexer_discovery,
        ..LogicalInterest::default()
    };

    Some((identity, interest))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planner::{InterestLifecycle, InterestScope};
    use crate::subs::InterestRegistry;

    #[test]
    fn parses_filter_into_tailing_interest_with_scope() {
        let (identity, interest) = build_interest_pair(
            r#"{"kinds":[1,6],"authors":["aa"]}"#,
            "author-aa",
            0,
            None,
            false,
            InterestLifecycle::Tailing,
        )
        .expect("valid filter");

        assert_eq!(interest.lifecycle, InterestLifecycle::Tailing);
        assert_eq!(interest.scope, InterestScope::ActiveAccount);
        assert_eq!(interest.id, InterestId(identity.key.0));
        assert_eq!(interest.shape.kinds, [1u32, 6u32].into_iter().collect());
        assert_eq!(
            interest.shape.authors,
            ["aa".to_string()].into_iter().collect()
        );
        let _ = identity;
    }

    #[test]
    fn scope_one_maps_to_global() {
        let (_id, interest) = build_interest_pair(
            r##"{"kinds":[1],"#t":["bitcoin"]}"##,
            "tag-bitcoin",
            1,
            None,
            false,
            InterestLifecycle::Tailing,
        )
        .unwrap();
        assert_eq!(interest.scope, InterestScope::Global);
    }

    #[test]
    fn malformed_filter_is_none() {
        assert!(
            build_interest_pair("not json", "c", 0, None, false, InterestLifecycle::Tailing)
                .is_none()
        );
        assert!(build_interest_pair("[]", "c", 0, None, false, InterestLifecycle::Tailing).is_none());
    }

    #[test]
    fn same_filter_different_json_order_dedups_to_one_slot() {
        use crate::kernel::cache_serve::{InterestWrite, RegistryWriteToken};
        let mut reg = InterestRegistry::new();
        let t = RegistryWriteToken::for_test();
        let (id_a, int_a) = build_interest_pair(
            r#"{"kinds":[1,6],"authors":["aa","bb"]}"#,
            "c",
            0,
            None,
            false,
            InterestLifecycle::Tailing,
        )
        .unwrap();
        let (id_b, int_b) = build_interest_pair(
            r#"{"authors":["bb","aa"],"kinds":[6,1]}"#,
            "c",
            0,
            None,
            false,
            InterestLifecycle::Tailing,
        )
        .unwrap();

        let r1 = reg.apply(&t, InterestWrite::EnsureAbsent, id_a, int_a);
        assert!(r1.newly_installed, "first open installs");
        let r2 = reg.apply(&t, InterestWrite::EnsureAbsent, id_b, int_b);
        assert!(
            !r2.newly_installed,
            "same filter+consumer is a no-op install (already present)"
        );
        assert_eq!(reg.len(), 1, "deduped to a single slot");
    }

    #[test]
    fn distinct_consumers_share_the_slot_and_last_close_drops_it() {
        use crate::kernel::cache_serve::{InterestWrite, RegistryWriteToken};
        let mut reg = InterestRegistry::new();
        let t = RegistryWriteToken::for_test();
        let filter = r#"{"kinds":[1,6],"authors":["aa"]}"#;
        let (id1, int1) = build_interest_pair(filter, "consumer-1", 0, None, false, InterestLifecycle::Tailing).unwrap();
        let (id2, int2) = build_interest_pair(filter, "consumer-2", 0, None, false, InterestLifecycle::Tailing).unwrap();

        let r1 = reg.apply(&t, InterestWrite::EnsureAbsent, id1.clone(), int1);
        assert!(r1.newly_installed, "consumer-1 installs");
        let r2 = reg.apply(&t, InterestWrite::EnsureAbsent, id2.clone(), int2);
        assert!(!r2.newly_installed, "consumer-2 attaches");
        assert_eq!(reg.len(), 1);

        let (close1, _) = build_interest_pair(filter, "consumer-1", 0, None, false, InterestLifecycle::Tailing).unwrap();
        assert!(!reg.drop_owner(&close1), "slot survives first close");
        assert_eq!(reg.len(), 1);

        let (close2, _) = build_interest_pair(filter, "consumer-2", 0, None, false, InterestLifecycle::Tailing).unwrap();
        assert!(reg.drop_owner(&close2), "last close drops the slot");
        assert!(reg.is_empty());
    }

    #[test]
    fn active_account_and_global_scope_of_same_filter_are_distinct_slots() {
        use crate::kernel::cache_serve::{InterestWrite, RegistryWriteToken};
        let mut reg = InterestRegistry::new();
        let t = RegistryWriteToken::for_test();
        let filter = r##"{"kinds":[1],"#t":["bitcoin"]}"##;
        let (id_active, int_active) =
            build_interest_pair(filter, "c", 0, None, false, InterestLifecycle::Tailing).unwrap();
        let (id_global, int_global) =
            build_interest_pair(filter, "c", 1, None, false, InterestLifecycle::Tailing).unwrap();

        let r1 = reg.apply(&t, InterestWrite::EnsureAbsent, id_active, int_active);
        assert!(r1.newly_installed);
        let r2 = reg.apply(&t, InterestWrite::EnsureAbsent, id_global, int_global);
        assert!(
            r2.newly_installed,
            "different scope → newly installed, not a dedup"
        );
        assert_eq!(reg.len(), 2);
    }

    #[test]
    fn relay_pin_sets_shape_and_distinct_pins_are_distinct_slots() {
        use crate::kernel::cache_serve::{InterestWrite, RegistryWriteToken};
        let filter = r#"{"kinds":[0],"search":"nostr"}"#;

        // The pin lands on the shape verbatim.
        let (_id, interest) = build_interest_pair(
            filter,
            "search-c",
            1,
            Some("wss://search-relay.example/"),
            false,
            InterestLifecycle::Tailing,
        )
        .unwrap();
        assert_eq!(
            interest.shape.relay_pin.as_deref(),
            Some("wss://search-relay.example/")
        );

        // Same filter+consumer, two different pins → two distinct registry slots
        // (the pin participates in the InterestShape hash → distinct SubKey).
        let mut reg = InterestRegistry::new();
        let t = RegistryWriteToken::for_test();
        let (id_a, int_a) =
            build_interest_pair(
                filter,
                "search-c",
                1,
                Some("wss://a.example/"),
                false,
                InterestLifecycle::Tailing,
            )
            .unwrap();
        let (id_b, int_b) =
            build_interest_pair(
                filter,
                "search-c",
                1,
                Some("wss://b.example/"),
                false,
                InterestLifecycle::Tailing,
            )
            .unwrap();
        assert!(
            reg.apply(&t, InterestWrite::EnsureAbsent, id_a, int_a)
                .newly_installed
        );
        assert!(
            reg.apply(&t, InterestWrite::EnsureAbsent, id_b, int_b)
                .newly_installed,
            "a different relay pin is a distinct slot, not a dedup"
        );
        assert_eq!(reg.len(), 2);

        // A pinned close reconstructs the same slot (pin matches) and drops it.
        let (close_a, _) =
            build_interest_pair(
                filter,
                "search-c",
                1,
                Some("wss://a.example/"),
                false,
                InterestLifecycle::Tailing,
            )
            .unwrap();
        assert!(reg.drop_owner(&close_a), "pinned close drops its own slot");
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn indexer_discovery_sets_interest_and_identity() {
        use crate::kernel::cache_serve::{InterestWrite, RegistryWriteToken};
        let filter = r#"{"kinds":[10154]}"#;

        let (_id, interest) =
            build_interest_pair(filter, "podcast-discovery", 1, None, true, InterestLifecycle::Tailing)
                .unwrap();
        assert!(
            interest.is_indexer_discovery,
            "the routing bit must reach LogicalInterest"
        );

        let mut reg = InterestRegistry::new();
        let t = RegistryWriteToken::for_test();
        let (normal_id, normal_interest) =
            build_interest_pair(
                filter,
                "podcast-discovery",
                1,
                None,
                false,
                InterestLifecycle::Tailing,
            )
            .unwrap();
        let (indexer_id, indexer_interest) =
            build_interest_pair(filter, "podcast-discovery", 1, None, true, InterestLifecycle::Tailing)
                .unwrap();

        assert!(
            reg.apply(&t, InterestWrite::EnsureAbsent, normal_id, normal_interest)
                .newly_installed
        );
        assert!(
            reg.apply(
                &t,
                InterestWrite::EnsureAbsent,
                indexer_id.clone(),
                indexer_interest
            )
            .newly_installed,
            "routing bit must be part of the key so a normal open cannot mask an indexer open"
        );
        assert_eq!(reg.len(), 2);
        assert!(
            reg.drop_owner(&indexer_id),
            "close must reconstruct the indexer-discovery identity"
        );
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn caller_selected_lifecycle_reaches_interest() {
        // #2948: the read-demand path can now ask for a OneShot compiled REQ.
        // The selected lifecycle rides the LogicalInterest verbatim.
        let filter = r##"{"kinds":[30402],"#a":["1:pub:collection"]}"##;

        let (_id, oneshot) =
            build_interest_pair(filter, "ad-collection", 1, None, false, InterestLifecycle::OneShot)
                .unwrap();
        assert_eq!(
            oneshot.lifecycle,
            InterestLifecycle::OneShot,
            "a OneShot demand must lower to a OneShot LogicalInterest (CLOSE on EOSE)"
        );

        let (_id, tailing) =
            build_interest_pair(filter, "ad-collection", 1, None, false, InterestLifecycle::Tailing)
                .unwrap();
        assert_eq!(
            tailing.lifecycle,
            InterestLifecycle::Tailing,
            "the same filter defaults to Tailing when the caller does not opt into OneShot"
        );
    }

    #[test]
    fn lifecycle_is_not_part_of_the_registry_key() {
        // Lifecycle changes WHEN a sub closes, not WHERE it routes, so a
        // OneShot and a Tailing open of the same filter+consumer+scope+pin
        // dedup to one slot (the last close drops it). This keeps the "zero
        // behavior change" guarantee: threading lifecycle does not re-key any
        // existing Tailing interest.
        use crate::kernel::cache_serve::{InterestWrite, RegistryWriteToken};
        let filter = r#"{"kinds":[1]}"#;
        let mut reg = InterestRegistry::new();
        let t = RegistryWriteToken::for_test();
        let (id_tailing, int_tailing) =
            build_interest_pair(filter, "c", 1, None, false, InterestLifecycle::Tailing).unwrap();
        let (id_oneshot, int_oneshot) =
            build_interest_pair(filter, "c", 1, None, false, InterestLifecycle::OneShot).unwrap();
        assert_eq!(
            id_tailing.key.0, id_oneshot.key.0,
            "lifecycle must not change the registry key"
        );

        assert!(
            reg.apply(&t, InterestWrite::EnsureAbsent, id_tailing, int_tailing)
                .newly_installed
        );
        assert!(
            !reg.apply(&t, InterestWrite::EnsureAbsent, id_oneshot, int_oneshot)
                .newly_installed,
            "same key regardless of lifecycle → the second open attaches, not a new slot"
        );
        assert_eq!(reg.len(), 1);
    }
}
