//! Helpers for constructing host-pinned `LogicalInterest`s.
//!
//! Per `docs/design/nip29/routing.md` §3, every NIP-29 subscription declares
//! `relay_pin: Some(host_relay_url)` so the compiler routes via Case E (the third
//! routing lane) — bypassing NIP-65 mailbox lookup entirely.

use std::collections::{BTreeMap, BTreeSet};

use nmp_core::planner::{InterestId, InterestLifecycle, InterestScope, LogicalInterest};
use nmp_core::substrate::ViewDependencies;

use crate::group_id::GroupId;
use crate::kinds::{
    KIND_GROUP_ADMINS, KIND_GROUP_MEMBERS, KIND_GROUP_METADATA, KIND_GROUP_ROLES,
};

/// Build a tailing host-pinned interest for the live group surface (chat /
/// discussions / etc.) — caller supplies the `kinds` they care about and any
/// additional tag filters.
///
/// The `h` tag is always added with the group's `local_id`, matching the
/// relay's structural filter requirement for group-scoped events.
pub fn host_pinned_interest(
    id: u64,
    group: &GroupId,
    kinds: impl IntoIterator<Item = u32>,
    extra_tags: BTreeMap<String, BTreeSet<String>>,
    lifecycle: InterestLifecycle,
) -> LogicalInterest {
    let mut tag_refs: Vec<(String, String)> = extra_tags
        .into_iter()
        .flat_map(|(key, vals)| vals.into_iter().map(move |v| (key.clone(), v)))
        .collect();
    tag_refs.push(("h".to_string(), group.local_id.clone()));

    ViewDependencies {
        kinds: kinds.into_iter().collect(),
        tag_refs,
        relay_pin: Some(group.host_relay_url.clone()),
        ..Default::default()
    }
    .into_logical_interest(InterestId(id), InterestScope::ActiveAccount, lifecycle)
}

/// Build a one-shot interest for the relay-signed metadata snapshot of a single
/// group (39000-39003, filtered by `d` tag).
pub fn metadata_interest(id: u64, group: &GroupId) -> LogicalInterest {
    ViewDependencies {
        kinds: vec![
            KIND_GROUP_METADATA,
            KIND_GROUP_ADMINS,
            KIND_GROUP_MEMBERS,
            KIND_GROUP_ROLES,
        ],
        tag_refs: vec![("d".to_string(), group.local_id.clone())],
        relay_pin: Some(group.host_relay_url.clone()),
        ..Default::default()
    }
    .into_logical_interest(
        InterestId(id),
        InterestScope::Global,
        InterestLifecycle::OneShot,
    )
}

/// Build a tailing host-pinned interest for **group discovery** on a single
/// relay — kinds 39000 / 39001 / 39002 with no `d` tag filter so every group
/// the relay hosts surfaces.
///
/// This is the read-side companion to the `nmp.nip29.discover` action: the
/// action enqueues this interest via `ActorCommand::PushInterest`, the relay
/// streams its metadata catalog back, and the `DiscoveredGroupsProjection`
/// (a `KernelEventObserver`) accumulates it into a flat list.
///
/// `InterestId` is derived deterministically from `host_relay_url` so a
/// repeated discover on the same relay is idempotent (the kernel de-dupes by
/// id and the REQ filter is identical).
pub fn relay_discovery_interest(host_relay_url: &str) -> LogicalInterest {
    let id = InterestId(nmp_core::stable_hash::stable_hash64((
        "nip29.discover",
        host_relay_url,
    )));
    ViewDependencies {
        kinds: vec![
            KIND_GROUP_METADATA,
            KIND_GROUP_ADMINS,
            KIND_GROUP_MEMBERS,
        ],
        relay_pin: Some(host_relay_url.to_string()),
        ..Default::default()
    }
    .into_logical_interest(id, InterestScope::Global, InterestLifecycle::Tailing)
}

/// Build a tailing interest for the `JoinedGroups` view: one per host relay in
/// the user's `JoinedHostsCache`, filtered to 39001/39002 events whose member
/// or admin list includes the user.
///
/// The relay does NOT actually filter by p-tag value across 39001/39002 (those
/// events embed members as `p` tags), so we encode the membership filter as
/// a `#p` tag dimension — the relay returns all 39001/39002 mentioning the
/// user, which is the correct surface.
pub fn joined_groups_for_host(
    id: u64,
    user_pubkey: &str,
    host_relay_url: &str,
) -> LogicalInterest {
    ViewDependencies {
        kinds: vec![KIND_GROUP_ADMINS, KIND_GROUP_MEMBERS],
        tag_refs: vec![("p".to_string(), user_pubkey.to_string())],
        relay_pin: Some(host_relay_url.to_string()),
        ..Default::default()
    }
    .into_logical_interest(
        InterestId(id),
        InterestScope::ActiveAccount,
        InterestLifecycle::Tailing,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group() -> GroupId {
        GroupId::new("wss://groups.example.com", "room-a")
    }

    #[test]
    fn host_pinned_interest_pins_to_host() {
        let i = host_pinned_interest(
            1,
            &group(),
            [9],
            BTreeMap::new(),
            InterestLifecycle::Tailing,
        );
        assert_eq!(i.shape.relay_pin.as_deref(), Some("wss://groups.example.com"));
        assert!(i.shape.tags.get("h").unwrap().contains("room-a"));
    }

    #[test]
    fn metadata_interest_targets_all_four_kinds() {
        let i = metadata_interest(2, &group());
        for k in [
            KIND_GROUP_METADATA,
            KIND_GROUP_ADMINS,
            KIND_GROUP_MEMBERS,
            KIND_GROUP_ROLES,
        ] {
            assert!(i.shape.kinds.contains(&k));
        }
        assert!(i.shape.tags.get("d").unwrap().contains("room-a"));
    }

    #[test]
    fn joined_groups_for_host_uses_pin() {
        let i = joined_groups_for_host(3, "pubkey", "wss://h.example.com");
        assert_eq!(i.shape.relay_pin.as_deref(), Some("wss://h.example.com"));
        assert!(i.shape.tags.get("p").unwrap().contains("pubkey"));
    }

    #[test]
    fn relay_discovery_interest_pins_three_metadata_kinds_with_no_d_filter() {
        let i = relay_discovery_interest("wss://groups.example.com");
        assert_eq!(
            i.shape.relay_pin.as_deref(),
            Some("wss://groups.example.com")
        );
        for k in [
            KIND_GROUP_METADATA,
            KIND_GROUP_ADMINS,
            KIND_GROUP_MEMBERS,
        ] {
            assert!(i.shape.kinds.contains(&k));
        }
        // No `d` tag filter — discovery is per-relay, not per-group.
        assert!(
            i.shape.tags.get("d").is_none(),
            "discovery interest must not constrain by group id"
        );
    }

    #[test]
    fn relay_discovery_interest_id_is_deterministic_per_relay() {
        let a1 = relay_discovery_interest("wss://groups.example.com");
        let a2 = relay_discovery_interest("wss://groups.example.com");
        let b = relay_discovery_interest("wss://other.example.com");
        // Same relay → same id (idempotent re-dispatch).
        assert_eq!(a1.id, a2.id);
        // Different relay → different id.
        assert_ne!(a1.id, b.id);
    }
}
