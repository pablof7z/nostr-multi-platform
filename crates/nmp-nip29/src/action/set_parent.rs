//! `SetParent` action — adopt a NIP-29 group under a parent or detach it to a
//! root (nips PR #2319).
//!
//! Emits a single `kind:9002` (edit-metadata) carrying `["h", local_id]` and
//! either `["parent", <parent_id>]` (adopt) or no `parent` tag (detach →
//! root). The relay updates the child's `kind:39000` and syncs the parent's
//! `child` list; bilateral admin consent, cycle rejection, and parent
//! existence are relay-enforced (the PR is explicit: relays MUST reject).
//!
//! The 9002 tag construction reuses [`super::metadata_tags::metadata_edit_tags`]
//! — the single canonical builder shared with `CreateGroupAction` — so
//! there is one code path for kind:9002 authoring (AGENTS.md "no
//! fragmentation"). `SetParent` passes `None` for name/about/picture/
//! visibility/access so the relay retains the group's prior metadata (NIP-29:
//! absent tags keep prior values); only `parent` is set.

use nmp_core::actor::ActorCommand;
use nmp_core::substrate::{
    ActionContext, ActionModule, ActionPayload, ActionPayloadDecodeError, ActionRejection,
};
use serde::{Deserialize, Serialize};

use crate::group_id::GroupId;
use crate::kinds::KIND_EDIT_METADATA;

use super::metadata_tags::{metadata_edit_tags, validate_parent};
use super::publish_plan::PublishPlan;

/// Adopt a group under a parent, or detach it to a root.
///
/// `parent: None` detaches — the 9002 omits the `parent` tag, so the relay
/// promotes the group to a root (the spec: "no `parent` tag to detach").
/// `parent: Some(id)` adopts — the 9002 carries `["parent", id]`.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct SetParentInput {
    pub group: GroupId,
    /// The parent's in-relay local id (`d` identifier). `None` detaches to
    /// root. An empty string is treated as `None`.
    #[serde(default)]
    pub parent: Option<String>,
}

fn validate(action: &SetParentInput) -> Result<(), ActionRejection> {
    action
        .group
        .require_routable()
        .map_err(ActionRejection::Invalid)?;
    if !(action.group.host_relay_url.starts_with("wss://")
        || action.group.host_relay_url.starts_with("ws://"))
    {
        return Err(ActionRejection::Invalid(
            "group.host_relay_url must start with wss:// or ws://".into(),
        ));
    }
    // Self-reference is a length-one cycle; relays MUST reject it, so fail
    // early at publish-time (the planner never sends a doomed 9002).
    validate_parent(action.parent.as_deref(), &action.group.local_id)
        .map_err(ActionRejection::Invalid)?;
    set_parent_plan(action)
        .validate_no_unpinned_h()
        .map_err(|_| ActionRejection::Invalid("missing host pin for set-parent".into()))
}

fn set_parent_plan(action: &SetParentInput) -> PublishPlan {
    let tags = metadata_edit_tags(
        &action.group.local_id,
        None,
        None,
        None,
        None,
        None,
        action.parent.as_deref(),
    );
    PublishPlan::pinned(&action.group, KIND_EDIT_METADATA, "", tags)
}

/// `nmp.nip29.set_parent` — adopt/detach a NIP-29 subgroup (nips PR #2319).
pub struct SetParentAction;
impl ActionModule for SetParentAction {
    const NAMESPACE: nmp_core::substrate::DeclaredActionNamespace =
        nmp_core::substrate::DeclaredActionNamespace::framework(
            "nmp.nip29.set_parent",
            "action.nmp.nip29.set_parent",
        );
    type Action = SetParentInput;

    /// ADR-0064 / S9 (#1747): opt into the typed FlatBuffers payload doorway;
    /// the fail-closed `schema_version` gate runs in `decode` (BEFORE `start`).
    fn decode_payload(bytes: &[u8]) -> Option<Result<Self::Action, ActionPayloadDecodeError>> {
        Some(<SetParentInput as ActionPayload>::decode(bytes))
    }

    fn start(&self, _ctx: &mut ActionContext, action: Self::Action) -> Result<(), ActionRejection> {
        validate(&action)
    }

    fn execute(
        &self,
        _ctx: &ActionContext,
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        send(set_parent_plan(&action).into_actor_command(Some(correlation_id.to_string()))?);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_core::actor::PublishCommand;
    use std::cell::RefCell;

    fn group() -> GroupId {
        GroupId::new("wss://groups.example.com", "nostr")
    }

    fn run_execute(input: SetParentInput) -> Result<Vec<ActorCommand>, String> {
        let captured: RefCell<Vec<ActorCommand>> = RefCell::new(Vec::new());
        SetParentAction.execute(
            &nmp_core::substrate::ActionContext::default(),
            input,
            "cid-sp",
            &|cmd| {
                captured.borrow_mut().push(cmd);
            },
        )?;
        Ok(captured.into_inner())
    }

    fn tags(cmds: &[ActorCommand]) -> &[Vec<String>] {
        match &cmds[0] {
            ActorCommand::Publish(PublishCommand::OwnedUnsignedEventToRelays { event, .. }) => {
                &event.tags
            }
            other => panic!("expected kind:9002 publish, got {other:?}"),
        }
    }

    #[test]
    fn adopt_emits_parent_tag_on_9002() {
        let action = SetParentInput {
            group: group(),
            parent: Some("tech".to_string()),
        };
        let cmds = run_execute(action).expect("executes");
        assert_eq!(cmds.len(), 1);
        match &cmds[0] {
            ActorCommand::Publish(PublishCommand::OwnedUnsignedEventToRelays {
                event,
                relays,
                correlation_id,
                ..
            }) => {
                assert_eq!(event.kind, KIND_EDIT_METADATA);
                assert_eq!(relays, &vec!["wss://groups.example.com".to_string()]);
                assert_eq!(correlation_id.as_deref(), Some("cid-sp"));
            }
            other => panic!("expected 9002 publish, got {other:?}"),
        }
        let t = tags(&cmds);
        assert!(t
            .iter()
            .any(|x| x == &vec!["h".to_string(), "nostr".to_string()]));
        assert!(t
            .iter()
            .any(|x| x == &vec!["parent".to_string(), "tech".to_string()]));
        // No name/about/visibility/access tags — those stay relay-side.
        assert!(!t.iter().any(|x| x.first() == Some(&"name".to_string())));
        assert!(!t.iter().any(|x| x == &vec!["public".to_string()]));
    }

    #[test]
    fn detach_omits_parent_tag() {
        let action = SetParentInput {
            group: group(),
            parent: None,
        };
        let cmds = run_execute(action).expect("executes");
        let t = tags(&cmds);
        assert!(t
            .iter()
            .any(|x| x == &vec!["h".to_string(), "nostr".to_string()]));
        assert!(
            !t.iter().any(|x| x.first() == Some(&"parent".to_string())),
            "detach must omit the parent tag, got {t:?}"
        );
    }

    #[test]
    fn empty_parent_string_detaches() {
        let action = SetParentInput {
            group: group(),
            parent: Some("".to_string()),
        };
        let cmds = run_execute(action).expect("executes");
        let t = tags(&cmds);
        assert!(!t.iter().any(|x| x.first() == Some(&"parent".to_string())));
    }

    #[test]
    fn self_reference_is_rejected() {
        let mut ctx = ActionContext::default();
        let action = SetParentInput {
            group: group(),
            parent: Some("nostr".to_string()),
        };
        assert!(matches!(
            SetParentAction.start(&mut ctx, action),
            Err(ActionRejection::Invalid(_))
        ));
    }

    #[test]
    fn non_websocket_host_is_rejected() {
        let mut ctx = ActionContext::default();
        let action = SetParentInput {
            group: GroupId::new("https://groups.example.com", "nostr"),
            parent: Some("tech".to_string()),
        };
        assert!(matches!(
            SetParentAction.start(&mut ctx, action),
            Err(ActionRejection::Invalid(_))
        ));
    }

    #[test]
    fn empty_host_is_rejected() {
        let mut ctx = ActionContext::default();
        let action = SetParentInput {
            group: GroupId::new("", "nostr"),
            parent: Some("tech".to_string()),
        };
        assert!(matches!(
            SetParentAction.start(&mut ctx, action),
            Err(ActionRejection::Invalid(_))
        ));
    }

    #[test]
    fn parent_defaults_to_none_in_json() {
        let json = concat!(
            "{",
            "\"group\":{\"host_relay_url\":\"wss://groups.example.com\",\"local_id\":\"nostr\"}",
            "}"
        );
        let parsed: SetParentInput = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.parent, None);
    }
}
