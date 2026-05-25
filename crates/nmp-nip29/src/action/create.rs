//! Public group creation action: publish a kind:9007 create-group request
//! followed by the conventional kind:9002 metadata edit.
//!
//! Per `docs/design/nip29/kinds.md` §2.3, kind:9007 establishes the group
//! and the relay treats the signer as the founding admin. The immediate
//! 9002 sets the user-visible metadata and marks the group public/open.

use nmp_core::substrate::{ActionContext, ActionModule, ActionRejection};
use nmp_core::ActorCommand;
use serde::{Deserialize, Serialize};

use crate::group_id::GroupId;
use crate::kinds::{KIND_CREATE_GROUP, KIND_EDIT_METADATA};

use super::publish_plan::PublishPlan;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CreatePublicGroupInput {
    pub group: GroupId,
    pub name: String,
    #[serde(default)]
    pub about: Option<String>,
}

fn create_group_plan(action: &CreatePublicGroupInput) -> PublishPlan {
    PublishPlan::pinned(
        &action.group,
        KIND_CREATE_GROUP,
        "",
        vec![vec!["h".into(), action.group.local_id.clone()]],
    )
}

fn metadata_plan(action: &CreatePublicGroupInput) -> PublishPlan {
    let mut tags = vec![
        vec!["h".into(), action.group.local_id.clone()],
        vec!["name".into(), action.name.trim().to_string()],
        vec!["public".into()],
        vec!["open".into()],
    ];
    if let Some(about) = action
        .about
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        tags.push(vec!["about".into(), about.to_string()]);
    }
    PublishPlan::pinned(&action.group, KIND_EDIT_METADATA, "", tags)
}

fn validate_group_id(group: &GroupId) -> Result<(), String> {
    group.require_routable()?;
    if !(group.host_relay_url.starts_with("wss://") || group.host_relay_url.starts_with("ws://")) {
        return Err("group.host_relay_url must start with wss:// or ws://".into());
    }
    if !group
        .local_id
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
    {
        return Err("group.local_id must use [a-z0-9-_]".into());
    }
    Ok(())
}

pub struct CreatePublicGroupAction;
impl ActionModule for CreatePublicGroupAction {
    const NAMESPACE: &'static str = "nmp.nip29.create_public_group";
    type Action = CreatePublicGroupInput;

    fn start(_ctx: &mut ActionContext, action: Self::Action) -> Result<(), ActionRejection> {
        validate_group_id(&action.group).map_err(ActionRejection::Invalid)?;
        if action.name.trim().is_empty() {
            return Err(ActionRejection::Invalid(
                "group name must not be empty".into(),
            ));
        }
        create_group_plan(&action)
            .validate_no_unpinned_h()
            .map_err(|_| ActionRejection::Invalid("missing host pin for group create".into()))?;
        metadata_plan(&action)
            .validate_no_unpinned_h()
            .map_err(|_| ActionRejection::Invalid("missing host pin for group metadata".into()))?;
        Ok(())
    }

    fn execute(
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        let cid = Some(correlation_id.to_string());
        send(create_group_plan(&action).into_actor_command(cid.clone())?);
        send(metadata_plan(&action).into_actor_command(cid)?);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn input() -> CreatePublicGroupInput {
        CreatePublicGroupInput {
            group: GroupId::new("wss://groups.example.com", "rust-nostr"),
            name: "Rust Nostr".to_string(),
            about: Some("Protocol work".to_string()),
        }
    }

    fn run_execute(input: CreatePublicGroupInput) -> Result<Vec<ActorCommand>, String> {
        let captured: RefCell<Vec<ActorCommand>> = RefCell::new(Vec::new());
        CreatePublicGroupAction::execute(input, "cid-create", &|cmd| {
            captured.borrow_mut().push(cmd);
        })?;
        Ok(captured.into_inner())
    }

    #[test]
    fn well_formed_passes_validator() {
        let mut ctx = ActionContext::default();
        assert!(CreatePublicGroupAction::start(&mut ctx, input()).is_ok());
    }

    #[test]
    fn execute_emits_create_then_metadata_commands() {
        let cmds = run_execute(input()).expect("well-formed input executes");
        assert_eq!(
            cmds.len(),
            2,
            "create must emit 9007 then 9002, got {cmds:?}"
        );

        match &cmds[0] {
            ActorCommand::PublishUnsignedEventToRelays {
                event,
                relays,
                correlation_id,
            } => {
                assert_eq!(event.kind, KIND_CREATE_GROUP);
                assert_eq!(relays, &vec!["wss://groups.example.com".to_string()]);
                assert_eq!(event.content, "");
                assert!(event
                    .tags
                    .iter()
                    .any(|t| t == &vec!["h".to_string(), "rust-nostr".to_string()]));
                assert_eq!(correlation_id.as_deref(), Some("cid-create"));
            }
            other => panic!("expected kind:9007 publish, got {other:?}"),
        }

        match &cmds[1] {
            ActorCommand::PublishUnsignedEventToRelays {
                event,
                relays,
                correlation_id,
            } => {
                assert_eq!(event.kind, KIND_EDIT_METADATA);
                assert_eq!(relays, &vec!["wss://groups.example.com".to_string()]);
                assert!(event
                    .tags
                    .iter()
                    .any(|t| t == &vec!["name".to_string(), "Rust Nostr".to_string()]));
                assert!(event.tags.iter().any(|t| t == &vec!["public".to_string()]));
                assert!(event.tags.iter().any(|t| t == &vec!["open".to_string()]));
                assert!(event
                    .tags
                    .iter()
                    .any(|t| t == &vec!["about".to_string(), "Protocol work".to_string()]));
                assert_eq!(correlation_id.as_deref(), Some("cid-create"));
            }
            other => panic!("expected kind:9002 publish, got {other:?}"),
        }
    }

    #[test]
    fn invalid_local_id_is_rejected() {
        let mut ctx = ActionContext::default();
        let action = CreatePublicGroupInput {
            group: GroupId::new("wss://groups.example.com", "Rust Nostr"),
            ..input()
        };
        assert!(matches!(
            CreatePublicGroupAction::start(&mut ctx, action),
            Err(ActionRejection::Invalid(_))
        ));
    }

    #[test]
    fn non_websocket_host_is_rejected() {
        let mut ctx = ActionContext::default();
        let action = CreatePublicGroupInput {
            group: GroupId::new("https://groups.example.com", "room"),
            ..input()
        };
        assert!(matches!(
            CreatePublicGroupAction::start(&mut ctx, action),
            Err(ActionRejection::Invalid(_))
        ));
    }

    #[test]
    fn empty_name_is_rejected() {
        let mut ctx = ActionContext::default();
        let action = CreatePublicGroupInput {
            name: "  ".to_string(),
            ..input()
        };
        assert!(matches!(
            CreatePublicGroupAction::start(&mut ctx, action),
            Err(ActionRejection::Invalid(_))
        ));
    }
}
