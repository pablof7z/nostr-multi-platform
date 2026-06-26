//! The generic NIP-29 publish surface: author **any** event kind into a group.
//!
//! NIP-29 is a kind-agnostic group transport. Its only concerns are the
//! `["h", local_id]` group tag, the `["previous", …]` timeline references, and
//! host-relay routing. The event *kind*, *content*, and any kind-specific
//! *tags* are the app's concern — "chat" is just `kind:9`, one more event kind.
//!
//! So the single app-facing surface is [`PublishGroupEventAction`]
//! (`nmp.nip29.publish_group_event`): the app says "publish this
//! `(kind, content, tags)` to group X" and this crate injects the envelope.
//!
//! ## `previous` tags come from the kernel store, not a crate-local cache
//!
//! The `["previous", …]` anti-spam timeline references point at the most recent
//! events seen in the group. Rather than maintain a parallel per-group cache,
//! the envelope composer issues a **cache-only** `StoreQuery::Tags { #h, limit }`
//! against the kernel event store (which already indexes every ingested event by
//! its single-letter `h` tag) at publish time. The store handle is read through
//! the V-83 [`EventStoreSlot`] publish-back slot — the same synchronous,
//! local-only read pattern as `nmp_core::slots::following_count_from_store`. No
//! relay round-trip, no async, bounded by `limit`.

use std::collections::{BTreeMap, BTreeSet};

use nmp_core::actor::ActorCommand;
use nmp_core::slots::EventStoreSlot;
use nmp_core::substrate::{
    ActionContext, ActionModule, ActionPayload, ActionPayloadDecodeError, ActionRejection,
};
use nmp_store::StoreQuery;
use serde::{Deserialize, Serialize};

use crate::cache::{previous_tag_prefix, EventIdPrefix};
use crate::group_id::GroupId;

use super::publish_plan::PublishPlan;

/// How many recent group events to reference in `["previous", …]` tags.
pub const DEFAULT_PREVIOUS_LIMIT: usize = 5;

/// Typed input for [`PublishGroupEventAction`].
///
/// `tags` carries only the caller's kind-specific tags (e.g. a reply `["e", …]`
/// or a `["t", …]` hashtag). The NIP-29 envelope tags (`h` / `previous`) are
/// injected by this crate and are **rejected** if supplied here.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct PublishGroupEventInput {
    pub group: GroupId,
    pub kind: u32,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub tags: Vec<Vec<String>>,
}

/// Read up to `limit` recent event-id prefixes for the group's `["previous", …]`
/// tags from the kernel store cache. Best-effort: an unpublished slot, a
/// poisoned lock, or a store error degrades to no `previous` tags (D6).
fn previous_prefixes_from_store(
    store_slot: &EventStoreSlot,
    group: &GroupId,
    limit: usize,
) -> Vec<EventIdPrefix> {
    if limit == 0 {
        return Vec::new();
    }
    let store = match store_slot.lock() {
        Ok(guard) => match guard.clone() {
            Some(store) => store,
            None => return Vec::new(),
        },
        Err(_) => return Vec::new(),
    };
    let Ok(h) = nostr::SingleLetterTag::from_char('h') else {
        return Vec::new();
    };
    let mut tags: BTreeMap<nostr::SingleLetterTag, BTreeSet<String>> = BTreeMap::new();
    tags.insert(h, BTreeSet::from([group.local_id.clone()]));
    let query = StoreQuery::Tags {
        authors: BTreeSet::new(),
        kinds: Vec::new(),
        tags,
        since: None,
        until: None,
    };
    // `query` returns newest-first, capped at `limit`.
    match store.query(&query, limit) {
        Ok(events) => events
            .iter()
            .map(|ev| previous_tag_prefix(&ev.raw.id))
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// Compose the host-pinned `PublishPlan` for an event authored into a NIP-29
/// group: the caller's tags, plus the injected `["h", local_id]` envelope tag
/// and the `["previous", …]` timeline references read from the store cache.
///
/// This is the single NIP-29 publish route; every group-publishing action
/// (`publish_group_event`, `react_in_group`, `share_event_in_group`,
/// `repost_in_group`) flows through it so the envelope is built in exactly one
/// place.
pub(crate) fn group_publish_plan(
    store_slot: &EventStoreSlot,
    group: &GroupId,
    kind: u32,
    content: impl Into<String>,
    caller_tags: Vec<Vec<String>>,
) -> PublishPlan {
    let previous = previous_prefixes_from_store(store_slot, group, DEFAULT_PREVIOUS_LIMIT);
    let mut tags = Vec::with_capacity(caller_tags.len() + 1 + previous.len());
    tags.push(vec!["h".to_string(), group.local_id.clone()]);
    for prefix in previous {
        tags.push(vec!["previous".to_string(), prefix]);
    }
    tags.extend(caller_tags);
    PublishPlan::pinned(group, kind, content, tags)
}

/// Reject caller-supplied NIP-29 envelope tags (`h` / `previous`) and malformed
/// (empty) tag rows. Envelope ownership belongs to this crate, not the caller.
pub(crate) fn reject_caller_envelope_tags(tags: &[Vec<String>]) -> Result<(), ActionRejection> {
    for tag in tags {
        let Some(name) = tag.first() else {
            return Err(ActionRejection::Invalid("empty tag row".into()));
        };
        if name == "h" || name == "previous" {
            return Err(ActionRejection::Invalid(format!(
                "caller must not supply the NIP-29 envelope tag `{name}`; it is injected by nmp-nip29"
            )));
        }
    }
    Ok(())
}

/// The generic "publish this event to group X" action. Stateful: it captures the
/// V-83 [`EventStoreSlot`] at composition so `execute` can read recent group
/// events for the `["previous", …]` tags.
pub struct PublishGroupEventAction {
    store_slot: EventStoreSlot,
}

impl PublishGroupEventAction {
    #[must_use]
    pub fn new(store_slot: EventStoreSlot) -> Self {
        Self { store_slot }
    }
}

impl ActionModule for PublishGroupEventAction {
    const NAMESPACE: &'static str = "nmp.nip29.publish_group_event";
    type Action = PublishGroupEventInput;

    /// ADR-0064 / S9 (#1747): opt into the typed FlatBuffers payload doorway; the
    /// fail-closed `schema_version` gate runs in `decode` (BEFORE `start`).
    fn decode_payload(bytes: &[u8]) -> Option<Result<Self::Action, ActionPayloadDecodeError>> {
        Some(<PublishGroupEventInput as ActionPayload>::decode(bytes))
    }

    fn start(&self, _ctx: &mut ActionContext, action: Self::Action) -> Result<(), ActionRejection> {
        action
            .group
            .require_routable()
            .map_err(ActionRejection::Invalid)?;
        reject_caller_envelope_tags(&action.tags)?;
        Ok(())
    }

    fn execute(
        &self,
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        let plan = group_publish_plan(
            &self.store_slot,
            &action.group,
            action.kind,
            action.content,
            action.tags,
        );
        send(plan.into_actor_command(Some(correlation_id.to_string()))?);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_core::slots::new_event_store_slot;

    fn input() -> PublishGroupEventInput {
        PublishGroupEventInput {
            group: GroupId::new("wss://groups.example.com", "room"),
            kind: 9,
            content: "hello".to_string(),
            tags: Vec::new(),
        }
    }

    #[test]
    fn well_formed_passes_validator() {
        let action = PublishGroupEventAction::new(new_event_store_slot());
        let mut ctx = ActionContext::default();
        assert!(action.start(&mut ctx, input()).is_ok());
    }

    #[test]
    fn empty_host_relay_url_rejected_in_start() {
        let action = PublishGroupEventAction::new(new_event_store_slot());
        let mut ctx = ActionContext::default();
        let bad = PublishGroupEventInput {
            group: GroupId::new("", "room"),
            ..input()
        };
        assert!(matches!(
            action.start(&mut ctx, bad),
            Err(ActionRejection::Invalid(_))
        ));
    }

    #[test]
    fn caller_supplied_envelope_tags_rejected() {
        let action = PublishGroupEventAction::new(new_event_store_slot());
        let mut ctx = ActionContext::default();
        for envelope in [vec!["h".to_string(), "room".to_string()], vec![
            "previous".to_string(),
            "abc".to_string(),
        ]] {
            let bad = PublishGroupEventInput {
                tags: vec![envelope],
                ..input()
            };
            assert!(matches!(
                action.start(&mut ctx, bad),
                Err(ActionRejection::Invalid(_))
            ));
        }
    }

    #[test]
    fn composer_injects_h_tag_and_preserves_caller_tags() {
        // No store published → no `previous` tags; `h` is always injected and
        // caller tags are preserved in order after it.
        let group = GroupId::new("wss://h.example.com", "g1");
        let plan = group_publish_plan(
            &new_event_store_slot(),
            &group,
            9,
            "hi",
            vec![vec!["e".to_string(), "deadbeef".to_string(), String::new(), "reply".to_string()]],
        );
        assert_eq!(plan.kind, 9);
        assert_eq!(plan.tags[0], vec!["h".to_string(), "g1".to_string()]);
        assert_eq!(plan.tags[1][0], "e");
        assert!(plan.pin_to.is_some());
    }
}
