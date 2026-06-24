//! Public group creation action: publish a kind:9007 create-group request
//! followed by the conventional kind:9002 metadata edit.
//!
//! Per `docs/design/nip29/kinds.md` §2.3, kind:9007 establishes the group
//! and the relay treats the signer as the founding admin. The immediate
//! 9002 sets the user-visible metadata and the caller-chosen visibility and
//! access markers.

use nmp_core::substrate::{
    ActionContext, ActionModule, ActionPayload, ActionPayloadDecodeError, ActionRejection,
};
use nmp_core::actor::ActorCommand;
use serde::{Deserialize, Serialize};

use crate::group_id::GroupId;
use crate::kinds::{KIND_CREATE_GROUP, KIND_EDIT_METADATA};

use super::metadata_tags::metadata_edit_tags;
use super::publish_plan::PublishPlan;

/// Whether the group is publicly listed or unlisted (private).
/// Serialises as `"public"` / `"private"` in JSON and as the corresponding
/// NIP-29 tag on the kind:9002 metadata event.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum GroupVisibility {
    #[default]
    Public,
    Private,
}

/// Whether the group is open (anyone may join) or closed (invite-only).
/// Serialises as `"open"` / `"closed"` in JSON and as the corresponding
/// NIP-29 tag on the kind:9002 metadata event.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum GroupAccess {
    #[default]
    Open,
    Closed,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct CreatePublicGroupInput {
    pub group: GroupId,
    pub name: String,
    #[serde(default)]
    pub about: Option<String>,
    /// URL for the group picture. When `Some` and non-empty, emits a
    /// `["picture", url]` tag on the kind:9002 event.
    #[serde(default)]
    pub picture: Option<String>,
    /// Controls the `["public"]` / `["private"]` NIP-29 visibility tag.
    /// Defaults to `Public`.
    #[serde(default)]
    pub visibility: GroupVisibility,
    /// Controls the `["open"]` / `["closed"]` NIP-29 access tag.
    /// Defaults to `Open`.
    #[serde(default)]
    pub access: GroupAccess,
    /// NIP-29 subgroups (nips PR #2319): optional `parent` local id. When
    /// `Some` and non-empty, the kind:9002 metadata edit carries
    /// `["parent", <id>]` so the relay adopts the new group under that
    /// parent. `None` (default) creates a root group. The value MUST NOT
    /// equal the group's own `local_id` (self-reference); `start()`
    /// rejects that client-side.
    #[serde(default)]
    pub parent: Option<String>,
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
    // Single canonical 9002 tag builder (metadata_tags.rs) — shared with the
    // `SetParent` action so there is one code path for kind:9002 authoring
    // (AGENTS.md "no fragmentation"). Create passes every field; SetParent
    // passes only `parent`.
    let tags = metadata_edit_tags(
        &action.group.local_id,
        Some(&action.name),
        action.about.as_deref(),
        action.picture.as_deref(),
        Some(action.visibility),
        Some(action.access),
        action.parent.as_deref(),
    );
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

    /// ADR-0064 / S9 (#1747): opt into the typed FlatBuffers payload doorway; the
    /// fail-closed `schema_version` gate runs in `decode` (BEFORE `start`).
    fn decode_payload(bytes: &[u8]) -> Option<Result<Self::Action, ActionPayloadDecodeError>> {
        Some(<CreatePublicGroupInput as ActionPayload>::decode(bytes))
    }

    fn start(&self, _ctx: &mut ActionContext, action: Self::Action) -> Result<(), ActionRejection> {
        validate_group_id(&action.group).map_err(ActionRejection::Invalid)?;
        if action.name.trim().is_empty() {
            return Err(ActionRejection::Invalid(
                "group name must not be empty".into(),
            ));
        }
        // NIP-29 subgroups (#2319): client-side self-reference guard. The
        // spec says relays MUST reject a self-referential parent (a cycle);
        // fail early so the publish planner never sends a doomed 9002.
        super::metadata_tags::validate_parent(action.parent.as_deref(), &action.group.local_id)
            .map_err(ActionRejection::Invalid)?;
        create_group_plan(&action)
            .validate_no_unpinned_h()
            .map_err(|_| ActionRejection::Invalid("missing host pin for group create".into()))?;
        metadata_plan(&action)
            .validate_no_unpinned_h()
            .map_err(|_| ActionRejection::Invalid("missing host pin for group metadata".into()))?;
        Ok(())
    }

    fn execute(
        &self,
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
    use nmp_core::actor::PublishCommand;

    use super::*;
    use std::cell::RefCell;

    fn input() -> CreatePublicGroupInput {
        CreatePublicGroupInput {
            group: GroupId::new("wss://groups.example.com", "rust-nostr"),
            name: "Rust Nostr".to_string(),
            about: Some("Protocol work".to_string()),
            ..Default::default()
        }
    }

    fn run_execute(input: CreatePublicGroupInput) -> Result<Vec<ActorCommand>, String> {
        let captured: RefCell<Vec<ActorCommand>> = RefCell::new(Vec::new());
        CreatePublicGroupAction.execute(input, "cid-create", &|cmd| {
            captured.borrow_mut().push(cmd);
        })?;
        Ok(captured.into_inner())
    }

    fn metadata_tags(cmds: &[ActorCommand]) -> &[Vec<String>] {
        match &cmds[1] {
            ActorCommand::Publish(PublishCommand::UnsignedEventToRelays { event, .. }) => &event.tags,
            other => panic!("expected kind:9002 publish, got {other:?}"),
        }
    }

    #[test]
    fn well_formed_passes_validator() {
        let mut ctx = ActionContext::default();
        assert!(CreatePublicGroupAction.start(&mut ctx, input()).is_ok());
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
            ActorCommand::Publish(PublishCommand::UnsignedEventToRelays {
                event,
                relays,
                correlation_id,
                ..
            }) => {
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
            ActorCommand::Publish(PublishCommand::UnsignedEventToRelays {
                event,
                relays,
                correlation_id,
                ..
            }) => {
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
            CreatePublicGroupAction.start(&mut ctx, action),
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
            CreatePublicGroupAction.start(&mut ctx, action),
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
            CreatePublicGroupAction.start(&mut ctx, action),
            Err(ActionRejection::Invalid(_))
        ));
    }

    // ── new field tests ──────────────────────────────────────────────────────

    #[test]
    fn private_visibility_emits_private_tag_not_public() {
        let action = CreatePublicGroupInput {
            visibility: GroupVisibility::Private,
            ..input()
        };
        let cmds = run_execute(action).expect("executes");
        let tags = metadata_tags(&cmds);
        assert!(
            tags.iter().any(|t| t == &vec!["private".to_string()]),
            "expected [\"private\"] tag, got {tags:?}"
        );
        assert!(
            !tags.iter().any(|t| t == &vec!["public".to_string()]),
            "must not emit [\"public\"] when Private, got {tags:?}"
        );
    }

    #[test]
    fn closed_access_emits_closed_tag_not_open() {
        let action = CreatePublicGroupInput {
            access: GroupAccess::Closed,
            ..input()
        };
        let cmds = run_execute(action).expect("executes");
        let tags = metadata_tags(&cmds);
        assert!(
            tags.iter().any(|t| t == &vec!["closed".to_string()]),
            "expected [\"closed\"] tag, got {tags:?}"
        );
        assert!(
            !tags.iter().any(|t| t == &vec!["open".to_string()]),
            "must not emit [\"open\"] when Closed, got {tags:?}"
        );
    }

    #[test]
    fn picture_some_non_empty_emits_picture_tag() {
        let action = CreatePublicGroupInput {
            picture: Some("https://example.com/img.jpg".to_string()),
            ..input()
        };
        let cmds = run_execute(action).expect("executes");
        let tags = metadata_tags(&cmds);
        assert!(
            tags.iter().any(|t| t
                == &vec![
                    "picture".to_string(),
                    "https://example.com/img.jpg".to_string()
                ]),
            "expected [\"picture\", url] tag, got {tags:?}"
        );
    }

    #[test]
    fn picture_none_does_not_emit_picture_tag() {
        let action = CreatePublicGroupInput {
            picture: None,
            ..input()
        };
        let cmds = run_execute(action).expect("executes");
        let tags = metadata_tags(&cmds);
        assert!(
            !tags
                .iter()
                .any(|t| t.first().map(|s| s == "picture").unwrap_or(false)),
            "must not emit [\"picture\", ...] when None, got {tags:?}"
        );
    }

    #[test]
    fn picture_some_empty_does_not_emit_picture_tag() {
        let action = CreatePublicGroupInput {
            picture: Some("   ".to_string()),
            ..input()
        };
        let cmds = run_execute(action).expect("executes");
        let tags = metadata_tags(&cmds);
        assert!(
            !tags
                .iter()
                .any(|t| t.first().map(|s| s == "picture").unwrap_or(false)),
            "must not emit [\"picture\", ...] for blank picture, got {tags:?}"
        );
    }

    #[test]
    fn missing_new_fields_in_json_deserialise_to_defaults() {
        // Simulates what the existing chirp-tui runtime_commands sends over FFI:
        // JSON with only {group, name} — no picture/visibility/access.
        // Avoid raw-string literals here: the doctrine-lint brace counter does
        // not handle `r#"..."#` and would miscount the JSON braces, causing a
        // false-positive D6 finding inside this cfg(test) module.
        let json = concat!(
            "{",
            "\"group\":{\"host_relay_url\":\"wss://groups.example.com\",\"local_id\":\"room-1\"},",
            "\"name\":\"Test Room\"",
            "}"
        );
        let parsed: CreatePublicGroupInput = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.picture, None);
        assert_eq!(parsed.visibility, GroupVisibility::Public);
        assert_eq!(parsed.access, GroupAccess::Open);
    }

    #[test]
    fn visibility_and_access_roundtrip_lowercase_json() {
        let action = CreatePublicGroupInput {
            group: GroupId::new("wss://groups.example.com", "room-2"),
            name: "Room".to_string(),
            visibility: GroupVisibility::Private,
            access: GroupAccess::Closed,
            ..Default::default()
        };
        let json = serde_json::to_string(&action).unwrap();
        assert!(
            json.contains("\"private\""),
            "visibility must serialise as \"private\", got {json}"
        );
        assert!(
            json.contains("\"closed\""),
            "access must serialise as \"closed\", got {json}"
        );
        let roundtripped: CreatePublicGroupInput = serde_json::from_str(&json).unwrap();
        assert_eq!(roundtripped.visibility, GroupVisibility::Private);
        assert_eq!(roundtripped.access, GroupAccess::Closed);
    }

    // ── NIP-29 subgroups (#2319): `parent` on create ────────────────────────

    #[test]
    fn parent_some_emits_parent_tag_on_metadata_9002() {
        let action = CreatePublicGroupInput {
            parent: Some("tech".to_string()),
            ..input()
        };
        let cmds = run_execute(action).expect("executes");
        let tags = metadata_tags(&cmds);
        assert!(
            tags.iter().any(|t| t == &vec!["parent".to_string(), "tech".to_string()]),
            "expected [\"parent\", \"tech\"] on the 9002, got {tags:?}"
        );
    }

    #[test]
    fn parent_none_omits_parent_tag() {
        let action = CreatePublicGroupInput {
            parent: None,
            ..input()
        };
        let cmds = run_execute(action).expect("executes");
        let tags = metadata_tags(&cmds);
        assert!(
            !tags.iter().any(|t| t.first() == Some(&"parent".to_string())),
            "must not emit a parent tag when None, got {tags:?}"
        );
    }

    #[test]
    fn parent_equal_to_local_id_is_rejected_as_self_reference() {
        let mut ctx = ActionContext::default();
        let action = CreatePublicGroupInput {
            group: GroupId::new("wss://groups.example.com", "rust-nostr"),
            name: "Rust Nostr".to_string(),
            parent: Some("rust-nostr".to_string()),
            ..Default::default()
        };
        assert!(matches!(
            CreatePublicGroupAction.start(&mut ctx, action),
            Err(ActionRejection::Invalid(_))
        ));
    }

    #[test]
    fn parent_defaults_to_none_in_json() {
        let json = concat!(
            "{",
            "\"group\":{\"host_relay_url\":\"wss://groups.example.com\",\"local_id\":\"room\"},",
            "\"name\":\"Test Room\"",
            "}"
        );
        let parsed: CreatePublicGroupInput = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.parent, None);
    }
}
