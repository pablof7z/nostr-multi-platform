//! `ReactInGroup` (kind:7+h).
//!
//! This is the "host-pinned variant of an otherwise cross-protocol action"
//! per `kinds.md` §4. It lives here because the routing concern (the `h`
//! tag) is the discriminator. It is a thin convenience over the generic
//! group-publish route: the envelope (`h` / `previous` / pin) is composed by
//! [`super::publish::group_publish_plan`]; this action only shapes the kind:7
//! reaction's caller tags (`e` / `p`).

use nmp_core::actor::ActorCommand;
use nmp_core::slots::EventStoreSlot;
use nmp_core::substrate::{
    ActionContext, ActionModule, ActionPayload, ActionPayloadDecodeError, ActionRejection,
};
use serde::{Deserialize, Serialize};

use crate::group_id::GroupId;

use super::publish::group_publish_plan;
use super::publish_plan::PublishPlan;

/// NIP-25 reaction kind. Kept file-private to `composed.rs` because NIP-29
/// does not own kind:7 — it only adds the `h`-tag routing concern. The
/// producer for the `h`-tagged variant lives here per `kinds.md` §4; the
/// kind constant itself stays inlined to avoid asserting NIP-29 ownership
/// over a foreign-NIP kind.
const REACTION_KIND: u32 = 7;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ReactInGroupInput {
    pub group: GroupId,
    pub target_event_id: String,
    pub target_author_pubkey: Option<String>,
    pub content: String,
}

/// The kind:7 reaction's caller tags (`e` target, optional `p` author). The
/// NIP-29 envelope tags are injected by [`group_publish_plan`].
fn react_caller_tags(action: &ReactInGroupInput) -> Vec<Vec<String>> {
    let mut tags = vec![vec!["e".to_string(), action.target_event_id.clone()]];
    if let Some(p) = &action.target_author_pubkey {
        tags.push(vec!["p".to_string(), p.clone()]);
    }
    tags
}

/// Build the kind:7 in-group reaction `PublishPlan`, composing the NIP-29
/// envelope (`h` / `previous` / pin) from the store cache.
fn react_in_group_plan(store_slot: &EventStoreSlot, action: &ReactInGroupInput) -> PublishPlan {
    group_publish_plan(
        store_slot,
        &action.group,
        REACTION_KIND,
        action.content.clone(),
        react_caller_tags(action),
    )
}

pub struct ReactInGroupAction {
    store_slot: EventStoreSlot,
}

impl ReactInGroupAction {
    #[must_use]
    pub fn new(store_slot: EventStoreSlot) -> Self {
        Self { store_slot }
    }
}

impl ActionModule for ReactInGroupAction {
    const NAMESPACE: &'static str = "nmp.nip29.react_in_group";
    type Action = ReactInGroupInput;

    /// ADR-0064 / S9 (#1747): opt into the typed FlatBuffers payload doorway; the
    /// fail-closed `schema_version` gate runs in `decode` (BEFORE `start`).
    fn decode_payload(bytes: &[u8]) -> Option<Result<Self::Action, ActionPayloadDecodeError>> {
        Some(<ReactInGroupInput as ActionPayload>::decode(bytes))
    }

    fn start(&self, _ctx: &mut ActionContext, action: Self::Action) -> Result<(), ActionRejection> {
        action
            .group
            .require_routable()
            .map_err(ActionRejection::Invalid)?;
        if action.target_event_id.is_empty() {
            return Err(ActionRejection::Invalid("target_event_id is empty".into()));
        }
        if action.content.is_empty() {
            return Err(ActionRejection::Invalid("reaction content is empty".into()));
        }
        Ok(())
    }
    fn execute(
        &self,
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        send(
            react_in_group_plan(&self.store_slot, &action)
                .into_actor_command(Some(correlation_id.to_string()))?,
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_core::actor::PublishCommand;
    use nmp_core::slots::new_event_store_slot;
    use std::cell::RefCell;

    fn action() -> ReactInGroupAction {
        ReactInGroupAction::new(new_event_store_slot())
    }

    fn react_input() -> ReactInGroupInput {
        ReactInGroupInput {
            group: GroupId::new("wss://groups.example.com", "room"),
            target_event_id: "deadbeef".to_string(),
            target_author_pubkey: None,
            content: "+".to_string(),
        }
    }

    #[test]
    fn react_well_formed_passes_validator() {
        let mut ctx = ActionContext::default();
        assert!(action().start(&mut ctx, react_input()).is_ok());
    }

    #[test]
    fn react_empty_host_relay_url_rejected_in_start() {
        let mut ctx = ActionContext::default();
        let input = ReactInGroupInput {
            group: GroupId::new("", "room"),
            ..react_input()
        };
        assert!(matches!(
            action().start(&mut ctx, input),
            Err(ActionRejection::Invalid(_))
        ));
    }

    #[test]
    fn react_empty_local_id_rejected_in_start() {
        let mut ctx = ActionContext::default();
        let input = ReactInGroupInput {
            group: GroupId::new("wss://h", ""),
            ..react_input()
        };
        assert!(matches!(
            action().start(&mut ctx, input),
            Err(ActionRejection::Invalid(_))
        ));
    }

    #[test]
    fn react_empty_target_event_id_rejected_in_start() {
        let mut ctx = ActionContext::default();
        let input = ReactInGroupInput {
            target_event_id: String::new(),
            ..react_input()
        };
        assert!(matches!(
            action().start(&mut ctx, input),
            Err(ActionRejection::Invalid(_))
        ));
    }

    #[test]
    fn react_empty_content_rejected_in_start() {
        let mut ctx = ActionContext::default();
        let input = ReactInGroupInput {
            content: String::new(),
            ..react_input()
        };
        assert!(matches!(
            action().start(&mut ctx, input),
            Err(ActionRejection::Invalid(_))
        ));
    }

    #[test]
    fn react_execute_emits_host_pinned_kind7_publish_command() {
        let captured: RefCell<Vec<ActorCommand>> = RefCell::new(Vec::new());
        action()
            .execute(react_input(), "react-cid", &|cmd| {
                captured.borrow_mut().push(cmd);
            })
            .expect("well-formed input executes");
        let cmds = captured.into_inner();
        assert_eq!(
            cmds.len(),
            1,
            "react executor must send exactly one command, got {cmds:?}"
        );
        match cmds.into_iter().next().unwrap() {
            ActorCommand::Publish(PublishCommand::UnsignedEventToRelays {
                event,
                relays,
                correlation_id,
                ..
            }) => {
                assert_eq!(event.kind, REACTION_KIND, "react must emit kind:7");
                assert_eq!(
                    relays,
                    vec!["wss://groups.example.com".to_string()],
                    "react must be pinned to the group's host relay"
                );
                assert!(
                    event
                        .tags
                        .iter()
                        .any(|t| t == &["h".to_string(), "room".to_string()]),
                    "must carry the ['h', local_id] group tag, got {:?}",
                    event.tags
                );
                assert_eq!(event.content, "+");
                assert_eq!(correlation_id.as_deref(), Some("react-cid"));
            }
            other => panic!("expected PublishUnsignedEventToRelays, got {other:?}"),
        }
    }
}
