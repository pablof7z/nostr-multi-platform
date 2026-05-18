//! Helpers for constructing host-pinned `LogicalInterest`s.
//!
//! Per `docs/design/nip29/routing.md` §3, every NIP-29 subscription declares
//! `pin_to: Some(host_relay_url)` so the compiler routes via Case E (the third
//! routing lane) — bypassing NIP-65 mailbox lookup entirely.

use std::collections::{BTreeMap, BTreeSet};

use nmp_core::planner::{InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest};

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
    let mut tags = extra_tags;
    tags.entry("h".to_string())
        .or_default()
        .insert(group.local_id.clone());

    LogicalInterest {
        id: InterestId(id),
        scope: InterestScope::ActiveAccount,
        shape: InterestShape {
            kinds: kinds.into_iter().collect(),
            tags,
            pin_to: Some(group.host_relay_url.clone()),
            ..Default::default()
        },
        hints: Vec::new(),
        lifecycle,
    }
}

/// Build a one-shot interest for the relay-signed metadata snapshot of a single
/// group (39000-39003, filtered by `d` tag).
pub fn metadata_interest(id: u64, group: &GroupId) -> LogicalInterest {
    let mut tags = BTreeMap::new();
    tags.insert(
        "d".to_string(),
        [group.local_id.clone()].into_iter().collect(),
    );
    LogicalInterest {
        id: InterestId(id),
        scope: InterestScope::Global,
        shape: InterestShape {
            kinds: [
                KIND_GROUP_METADATA,
                KIND_GROUP_ADMINS,
                KIND_GROUP_MEMBERS,
                KIND_GROUP_ROLES,
            ]
            .into_iter()
            .collect(),
            tags,
            pin_to: Some(group.host_relay_url.clone()),
            ..Default::default()
        },
        hints: Vec::new(),
        lifecycle: InterestLifecycle::OneShot,
    }
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
    let mut tags = BTreeMap::new();
    tags.insert(
        "p".to_string(),
        [user_pubkey.to_string()].into_iter().collect(),
    );
    LogicalInterest {
        id: InterestId(id),
        scope: InterestScope::ActiveAccount,
        shape: InterestShape {
            kinds: [KIND_GROUP_ADMINS, KIND_GROUP_MEMBERS]
                .into_iter()
                .collect(),
            tags,
            pin_to: Some(host_relay_url.to_string()),
            ..Default::default()
        },
        hints: Vec::new(),
        lifecycle: InterestLifecycle::Tailing,
    }
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
        assert_eq!(i.shape.pin_to.as_deref(), Some("wss://groups.example.com"));
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
        assert_eq!(i.shape.pin_to.as_deref(), Some("wss://h.example.com"));
        assert!(i.shape.tags.get("p").unwrap().contains("pubkey"));
    }
}
