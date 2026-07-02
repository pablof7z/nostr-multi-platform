use std::collections::{BTreeMap, BTreeSet};

use nmp_core::substrate::KernelEvent;
use nmp_note_feed::HostedGroupContext;
use nmp_planner::InterestShape;

pub(super) fn group_event_shapes(
    groups: &BTreeSet<nmp_nip51::SimpleGroupRef>,
    kinds: &BTreeSet<u32>,
) -> Vec<InterestShape> {
    if groups.is_empty() || kinds.is_empty() {
        return Vec::new();
    }
    let mut by_host: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for group in groups {
        if let Some(group_id) = routable_group_id(group) {
            by_host
                .entry(group_id.host_relay_url)
                .or_default()
                .insert(group_id.local_id);
        }
    }

    by_host
        .into_iter()
        .map(|(host, local_ids)| {
            let mut shape = InterestShape {
                kinds: kinds.clone(),
                relay_pin: Some(host),
                ..InterestShape::default()
            };
            shape.tags.insert("h".to_string(), local_ids);
            shape
        })
        .collect()
}

pub(super) fn group_event_admitted(
    groups: &BTreeSet<nmp_nip51::SimpleGroupRef>,
    kinds: &BTreeSet<u32>,
    event: &KernelEvent,
) -> bool {
    group_event_context(groups, kinds, event).is_some()
}

pub(super) fn group_event_context(
    groups: &BTreeSet<nmp_nip51::SimpleGroupRef>,
    kinds: &BTreeSet<u32>,
    event: &KernelEvent,
) -> Option<HostedGroupContext> {
    if groups.is_empty() || !kinds.contains(&event.kind) {
        return None;
    }
    let local_ids: BTreeSet<&str> = event
        .tags
        .iter()
        .filter_map(|tag| {
            (tag.first().map(String::as_str) == Some("h"))
                .then(|| tag.get(1).map(String::as_str))
                .flatten()
        })
        .collect();
    if local_ids.is_empty() {
        return None;
    }
    let mut matching_groups = groups
        .iter()
        .filter_map(routable_group_id)
        .filter(|group_id| local_ids.contains(group_id.local_id.as_str()));
    let by_relay = matching_groups.find_map(|group_id| {
        event
            .relay_provenance
            .iter()
            .any(|relay| relay == &group_id.host_relay_url)
            .then_some(HostedGroupContext {
                host_relay_url: group_id.host_relay_url,
                local_id: group_id.local_id,
            })
    });
    if by_relay.is_some() {
        return by_relay;
    }
    if !event
        .relay_provenance
        .iter()
        .any(|relay| relay == "local://publish")
    {
        return None;
    }

    let mut local_matches = groups
        .iter()
        .filter_map(routable_group_id)
        .filter(|group_id| local_ids.contains(group_id.local_id.as_str()));
    let only_match = local_matches.next()?;
    local_matches
        .next()
        .is_none()
        .then_some(HostedGroupContext {
            host_relay_url: only_match.host_relay_url,
            local_id: only_match.local_id,
        })
}

fn routable_group_id(group: &nmp_nip51::SimpleGroupRef) -> Option<nmp_nip29::GroupId> {
    let group_id = nmp_nip29::GroupId::new(group.host_relay_url.clone(), group.local_id.clone());
    group_id.require_routable().ok()?;
    Some(group_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_core::substrate::EventId;

    fn groups() -> BTreeSet<nmp_nip51::SimpleGroupRef> {
        [
            nmp_nip51::SimpleGroupRef::new("room-a", "wss://relay-a"),
            nmp_nip51::SimpleGroupRef::new("room-b", "wss://relay-a"),
            nmp_nip51::SimpleGroupRef::new("room-a", "wss://relay-b"),
        ]
        .into_iter()
        .collect()
    }

    fn event(local_id: &str, relay: &str, kind: u32) -> KernelEvent {
        KernelEvent {
            id: EventId::from("01".repeat(32)),
            author: "aa".repeat(32),
            kind,
            created_at: 10,
            tags: vec![vec!["h".to_string(), local_id.to_string()]],
            content: String::new(),
            relay_provenance: vec![relay.to_string()],
        }
    }

    #[test]
    fn group_event_shapes_group_by_host_relay() {
        let shapes = group_event_shapes(&groups(), &BTreeSet::from([1_u32, 9_u32]));
        assert_eq!(shapes.len(), 2);
        let relay_a = shapes
            .iter()
            .find(|shape| shape.relay_pin.as_deref() == Some("wss://relay-a"))
            .expect("relay-a shape");
        assert_eq!(
            relay_a.tags.get("h").cloned().unwrap_or_default(),
            ["room-a".to_string(), "room-b".to_string()]
                .into_iter()
                .collect()
        );
        let relay_b = shapes
            .iter()
            .find(|shape| shape.relay_pin.as_deref() == Some("wss://relay-b"))
            .expect("relay-b shape");
        assert_eq!(
            relay_b.tags.get("h").cloned().unwrap_or_default(),
            ["room-a".to_string()].into_iter().collect()
        );
    }

    #[test]
    fn admission_requires_matching_host_and_h_tag() {
        let groups = groups();
        let kinds = BTreeSet::from([9_u32]);
        assert!(group_event_admitted(
            &groups,
            &kinds,
            &event("room-a", "wss://relay-a", 9)
        ));
        assert!(group_event_admitted(
            &groups,
            &kinds,
            &event("room-a", "wss://relay-b", 9)
        ));
        assert!(!group_event_admitted(
            &groups,
            &kinds,
            &event("room-b", "wss://relay-b", 9)
        ));
        assert!(!group_event_admitted(
            &groups,
            &kinds,
            &event("room-a", "wss://relay-a", 1)
        ));
    }

    #[test]
    fn context_uses_the_matched_group_host_and_local_id() {
        let context = group_event_context(
            &groups(),
            &BTreeSet::from([9_u32]),
            &event("room-b", "wss://relay-a", 9),
        )
        .expect("matched context");
        assert_eq!(
            context,
            HostedGroupContext {
                host_relay_url: "wss://relay-a".to_string(),
                local_id: "room-b".to_string(),
            }
        );
    }

    #[test]
    fn local_publish_context_requires_unambiguous_group() {
        assert!(group_event_context(
            &groups(),
            &BTreeSet::from([9_u32]),
            &event("room-a", "local://publish", 9),
        )
        .is_none());

        let context = group_event_context(
            &groups(),
            &BTreeSet::from([9_u32]),
            &event("room-b", "local://publish", 9),
        )
        .expect("unambiguous local group");
        assert_eq!(
            context,
            HostedGroupContext {
                host_relay_url: "wss://relay-a".to_string(),
                local_id: "room-b".to_string(),
            }
        );
    }
}
