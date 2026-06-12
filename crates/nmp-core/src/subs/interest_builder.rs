//! Build `(SubIdentity, LogicalInterest)` pairs for `open_interest` /
//! `close_interest` commands.
//!
//! Always-compiled (not gated behind `#[cfg(feature = "native")]`) so the
//! wasm32 `KernelReducer` surface can call it as well as the native actor.
//! The native actor delegates through
//! `actor::dispatch::build_open_interest`, which calls [`build_interest_pair`]
//! directly.

use crate::planner::{InterestLifecycle, InterestScope, LogicalInterest};
use crate::subs::sub_key::{SubIdentity, SubKey, SubOwnerKey, SubScope};

/// M2 (ADR-0042) — build the `(SubIdentity, LogicalInterest)` pair for an
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
/// Returns `None` when `filter_json` is not a valid NIP-01 filter object
/// (D6 — the caller treats this as a silent no-op).
pub(crate) fn build_interest_pair(
    filter_json: &str,
    consumer_id: &str,
    scope: u32,
) -> Option<(SubIdentity, LogicalInterest)> {
    let shape = crate::planner::InterestShape::from_filter_json(filter_json)?;

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
    let key = SubKey::builder("open-interest")
        .with(&shape)
        .with(scope)
        .finish();
    let identity = SubIdentity::new(SubOwnerKey::new(consumer_id), key, sub_scope);

    let interest = LogicalInterest {
        scope: interest_scope,
        shape,
        // `open_interest` is always a tailing feed subscription (never OneShot).
        lifecycle: InterestLifecycle::Tailing,
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
        let (identity, interest) =
            build_interest_pair(r#"{"kinds":[1,6],"authors":["aa"]}"#, "author-aa", 0)
                .expect("valid filter");

        assert_eq!(interest.lifecycle, InterestLifecycle::Tailing);
        assert_eq!(interest.scope, InterestScope::ActiveAccount);
        assert_eq!(interest.shape.kinds, [1u32, 6u32].into_iter().collect());
        assert_eq!(interest.shape.authors, ["aa".to_string()].into_iter().collect());
        let _ = identity;
    }

    #[test]
    fn scope_one_maps_to_global() {
        let (_id, interest) =
            build_interest_pair(r##"{"kinds":[1],"#t":["bitcoin"]}"##, "tag-bitcoin", 1).unwrap();
        assert_eq!(interest.scope, InterestScope::Global);
    }

    #[test]
    fn malformed_filter_is_none() {
        assert!(build_interest_pair("not json", "c", 0).is_none());
        assert!(build_interest_pair("[]", "c", 0).is_none());
    }

    #[test]
    fn same_filter_different_json_order_dedups_to_one_slot() {
        let mut reg = InterestRegistry::new();
        let (id_a, int_a) =
            build_interest_pair(r#"{"kinds":[1,6],"authors":["aa","bb"]}"#, "c", 0).unwrap();
        let (id_b, int_b) =
            build_interest_pair(r#"{"authors":["bb","aa"],"kinds":[6,1]}"#, "c", 0).unwrap();

        assert!(reg.ensure_sub(id_a, int_a), "first open installs");
        assert!(
            !reg.ensure_sub(id_b, int_b),
            "same filter+consumer is a no-op install (already present)"
        );
        assert_eq!(reg.len(), 1, "deduped to a single slot");
    }

    #[test]
    fn distinct_consumers_share_the_slot_and_last_close_drops_it() {
        let mut reg = InterestRegistry::new();
        let filter = r#"{"kinds":[1,6],"authors":["aa"]}"#;
        let (id1, int1) = build_interest_pair(filter, "consumer-1", 0).unwrap();
        let (id2, int2) = build_interest_pair(filter, "consumer-2", 0).unwrap();

        assert!(reg.ensure_sub(id1.clone(), int1), "consumer-1 installs");
        assert!(!reg.ensure_sub(id2.clone(), int2), "consumer-2 attaches");
        assert_eq!(reg.len(), 1);

        let (close1, _) = build_interest_pair(filter, "consumer-1", 0).unwrap();
        assert!(!reg.drop_owner(&close1), "slot survives first close");
        assert_eq!(reg.len(), 1);

        let (close2, _) = build_interest_pair(filter, "consumer-2", 0).unwrap();
        assert!(reg.drop_owner(&close2), "last close drops the slot");
        assert!(reg.is_empty());
    }

    #[test]
    fn active_account_and_global_scope_of_same_filter_are_distinct_slots() {
        let mut reg = InterestRegistry::new();
        let filter = r##"{"kinds":[1],"#t":["bitcoin"]}"##;
        let (id_active, int_active) = build_interest_pair(filter, "c", 0).unwrap();
        let (id_global, int_global) = build_interest_pair(filter, "c", 1).unwrap();

        assert!(reg.ensure_sub(id_active, int_active));
        assert!(
            reg.ensure_sub(id_global, int_global),
            "different scope → newly installed, not a dedup"
        );
        assert_eq!(reg.len(), 2);
    }
}
