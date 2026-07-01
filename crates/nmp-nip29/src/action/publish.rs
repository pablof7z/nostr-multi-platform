//! Legacy/raw NIP-29 group publish action.
//!
//! NIP-29 is a kind-agnostic group transport. Its only concerns are the
//! `["h", local_id]` group tag, the `["previous", …]` timeline references, and
//! host-relay routing. Protocol-owned artifacts should be built by their owning
//! crate and carried through `publish_plan::wrap_owned_draft`; this raw action
//! remains for unprotected/app-private group events and is still publish-gated
//! before signing.
//!
//! This keeps the important boundary clear: `kind:7 + h` is a NIP-25 reaction
//! carried through a NIP-29 envelope, not a NIP-29 reaction.
//!
//! ## `previous` tags come from the kernel store, not a crate-local cache
//!
//! The `["previous", …]` anti-spam timeline references point at the most recent
//! events seen in the group. Rather than maintain a parallel per-group cache,
//! the envelope composer issues a **cache-only** `StoreQuery::Tags { #h, limit }`
//! against the kernel event store (which already indexes every ingested event by
//! its single-letter `h` tag) at publish time. The store handle is read through
//! the execution [`ActionContext`], so all action-local reads use one bounded
//! primitive instead of per-action slot captures. No relay round-trip, no async,
//! bounded by `limit`.

use std::collections::{BTreeMap, BTreeSet};

use nmp_core::actor::ActorCommand;
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
/// tags from the kernel store cache. Best-effort: an unavailable context,
/// poisoned store slot, or store error degrades to no `previous` tags (D6).
fn previous_prefixes_from_context(
    ctx: &ActionContext,
    group: &GroupId,
    limit: usize,
) -> Vec<EventIdPrefix> {
    if limit == 0 {
        return Vec::new();
    }
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
    match ctx.query_local_events(&query, limit) {
        Ok(events) => events
            .iter()
            .map(|ev| previous_tag_prefix(&ev.raw.id))
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// Compose the host-pinned `PublishPlan` for a raw event authored into a NIP-29
/// group: the caller's tags, plus the injected `["h", local_id]` envelope tag
/// and the `["previous", …]` timeline references read from the store cache.
///
/// The generic `publish_group_event` action flows through it so the `h` /
/// `previous` / pin envelope is built in exactly one place for the legacy raw
/// doorway.
pub(crate) fn group_publish_plan(
    ctx: &ActionContext,
    group: &GroupId,
    kind: u32,
    content: impl Into<String>,
    caller_tags: Vec<Vec<String>>,
) -> PublishPlan {
    let previous = previous_prefixes_from_context(ctx, group, DEFAULT_PREVIOUS_LIMIT);
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

/// The generic "publish this event to group X" action.
pub struct PublishGroupEventAction;

impl ActionModule for PublishGroupEventAction {
    const NAMESPACE: nmp_core::substrate::DeclaredActionNamespace =
        nmp_core::substrate::DeclaredActionNamespace::framework(
            "nmp.nip29.publish_group_event",
            "action.nmp.nip29.publish_group_event",
        );
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
        reject_raw_protected_artifact(action.kind, &action.tags)?;
        Ok(())
    }

    fn execute(
        &self,
        ctx: &ActionContext,
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        let plan = group_publish_plan(ctx, &action.group, action.kind, action.content, action.tags);
        send(plan.into_actor_command(Some(correlation_id.to_string()))?);
        Ok(())
    }
}

fn reject_raw_protected_artifact(kind: u32, tags: &[Vec<String>]) -> Result<(), ActionRejection> {
    let reason = match kind {
        7 => Some("kind:7 reactions must be built by nmp-nip25 and wrapped by nmp-nip29"),
        5 if deletes_kind_7_reaction(tags) => {
            Some("kind:5 reaction deletions must be built by the reaction/deletion owner")
        }
        39000..=39003 => Some("NIP-29 group metadata kinds use the typed NIP-29 owner actions"),
        9000 | 9001 | 9002 | 9005 | 9007 | 9008 | 9009 | 9021 | 9022 => {
            Some("NIP-29 group management kinds use the typed NIP-29 owner actions")
        }
        _ => None,
    };
    if let Some(reason) = reason {
        return Err(ActionRejection::Invalid(reason.to_string()));
    }
    Ok(())
}

fn deletes_kind_7_reaction(tags: &[Vec<String>]) -> bool {
    tags.iter().any(|tag| {
        tag.first().is_some_and(|name| name == "k") && tag.get(1).is_some_and(|kind| kind == "7")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use nmp_store::{EventStore, MemEventStore, RawEvent, VerifiedEvent};

    fn input() -> PublishGroupEventInput {
        PublishGroupEventInput {
            group: GroupId::new("wss://groups.example.com", "room"),
            kind: 9,
            content: "hello".to_string(),
            tags: Vec::new(),
        }
    }

    fn stored_group_event(id: &str, group: &str, created_at: u64) -> VerifiedEvent {
        let raw = RawEvent {
            id: id.to_string(),
            pubkey: "11".repeat(32),
            created_at,
            kind: 9,
            tags: vec![vec!["h".to_string(), group.to_string()]],
            content: "cached".to_string(),
            sig: "aa".repeat(64),
        };
        VerifiedEvent::from_raw_unchecked(raw)
    }

    #[test]
    fn well_formed_passes_validator() {
        let action = PublishGroupEventAction;
        let mut ctx = ActionContext::default();
        assert!(action.start(&mut ctx, input()).is_ok());
    }

    #[test]
    fn empty_host_relay_url_rejected_in_start() {
        let action = PublishGroupEventAction;
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
        let action = PublishGroupEventAction;
        let mut ctx = ActionContext::default();
        for envelope in [
            vec!["h".to_string(), "room".to_string()],
            vec!["previous".to_string(), "abc".to_string()],
        ] {
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
    fn raw_group_publish_rejects_protected_artifacts() {
        let action = PublishGroupEventAction;
        let mut ctx = ActionContext::default();
        for bad in [
            PublishGroupEventInput { kind: 7, ..input() },
            PublishGroupEventInput {
                kind: 5,
                tags: vec![vec!["k".to_string(), "7".to_string()]],
                ..input()
            },
            PublishGroupEventInput {
                kind: 9000,
                ..input()
            },
            PublishGroupEventInput {
                kind: 39000,
                ..input()
            },
        ] {
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
        let ctx = ActionContext::default();
        let plan = group_publish_plan(
            &ctx,
            &group,
            9,
            "hi",
            vec![vec![
                "e".to_string(),
                "deadbeef".to_string(),
                String::new(),
                "reply".to_string(),
            ]],
        );
        assert_eq!(plan.kind, 9);
        assert_eq!(plan.tags[0], vec!["h".to_string(), "g1".to_string()]);
        assert_eq!(plan.tags[1][0], "e");
        assert!(plan.pin_to.is_some());
    }

    #[test]
    fn composer_reads_previous_tags_through_action_context() {
        let store = MemEventStore::default();
        store
            .insert(
                stored_group_event(&"22".repeat(32), "g1", 2),
                &"wss://r".to_string(),
                2,
            )
            .unwrap();
        store
            .insert(
                stored_group_event(&"33".repeat(32), "g1", 1),
                &"wss://r".to_string(),
                1,
            )
            .unwrap();
        store
            .insert(
                stored_group_event(&"44".repeat(32), "other", 3),
                &"wss://r".to_string(),
                3,
            )
            .unwrap();

        let ctx = ActionContext::with_event_store(Arc::new(store));
        let group = GroupId::new("wss://h.example.com", "g1");
        let plan = group_publish_plan(&ctx, &group, 9, "hi", Vec::new());

        assert_eq!(plan.tags[0], vec!["h".to_string(), "g1".to_string()]);
        assert_eq!(
            plan.tags[1],
            vec!["previous".to_string(), "22222222".to_string()]
        );
        assert_eq!(
            plan.tags[2],
            vec!["previous".to_string(), "33333333".to_string()]
        );
        assert_eq!(plan.tags.len(), 3);
    }
}
