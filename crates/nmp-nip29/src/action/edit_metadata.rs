//! `EditMetadata` action — edit an EXISTING NIP-29 group's metadata.
//!
//! Emits a single `kind:9002` (edit-metadata) carrying `["h", local_id]` plus
//! whichever of `name` / `about` / `picture` / visibility / access the caller
//! set. This is the post-creation counterpart to `CreatePublicGroupAction`
//! (which emits 9007 + an initial 9002) and the sibling of `SetParentAction`
//! (which edits only the `parent` field of the same 9002 surface).
//!
//! The 9002 tag construction reuses [`super::metadata_tags::metadata_edit_tags`]
//! — the single canonical builder shared with create / set-parent — so there is
//! one code path for kind:9002 authoring (AGENTS.md "no fragmentation"). Only
//! the fields the caller set are emitted; absent fields omit their tag and the
//! relay retains the prior value (NIP-29: absent tags keep prior values).
//!
//! ## Admin-gated
//!
//! Editing group metadata is an admin action — relay29 / NIP-29 relays reject a
//! 9002 from a non-admin. Like the other admin actions (`PutUser`,
//! `CreateInvite`), this module performs the structural + host-pin validation
//! and lets the relay enforce admin authority; the relay-signed 39000/39001
//! snapshots remain the source of truth for who may edit.

use nmp_core::actor::ActorCommand;
use nmp_core::substrate::{
    ActionContext, ActionModule, ActionPayload, ActionPayloadDecodeError, ActionRejection,
};
use serde::{Deserialize, Serialize};

use crate::group_id::GroupId;
use crate::kinds::KIND_EDIT_METADATA;

use super::metadata_tags::metadata_edit_tags;
use super::publish_plan::PublishPlan;
use super::{GroupAccess, GroupVisibility};

/// Edit an existing group's metadata. Every field is optional; `None` (or an
/// empty-after-trim string) leaves the relay's prior value untouched. At least
/// one field must be set — `start()` rejects a no-op edit.
///
/// `parent` is deliberately NOT editable here: re-parenting is the orthogonal
/// `nmp.nip29.set_parent` action (different intent, bilateral admin consent).
/// Omitting the `parent` tag on this 9002 keeps the group's prior parent.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct EditMetadataInput {
    pub group: GroupId,
    /// New `["name", _]`. `None`/empty leaves the prior name.
    #[serde(default)]
    pub name: Option<String>,
    /// New `["about", _]`. `None`/empty leaves the prior about text.
    #[serde(default)]
    pub about: Option<String>,
    /// New `["picture", _]`. `None`/empty leaves the prior picture.
    #[serde(default)]
    pub picture: Option<String>,
    /// New `["public"]` / `["private"]` marker. `None` leaves the prior
    /// visibility.
    #[serde(default)]
    pub visibility: Option<GroupVisibility>,
    /// New `["open"]` / `["closed"]` marker. `None` leaves the prior access.
    #[serde(default)]
    pub access: Option<GroupAccess>,
}

impl EditMetadataInput {
    /// Whether this edit would change at least one field. A 9002 carrying only
    /// the `h` tag is a relay no-op, so the validator rejects it.
    fn edits_something(&self) -> bool {
        self.name.as_deref().map(str::trim).is_some_and(|s| !s.is_empty())
            || self.about.as_deref().map(str::trim).is_some_and(|s| !s.is_empty())
            || self.picture.as_deref().map(str::trim).is_some_and(|s| !s.is_empty())
            || self.visibility.is_some()
            || self.access.is_some()
    }
}

fn edit_metadata_plan(action: &EditMetadataInput) -> PublishPlan {
    let tags = metadata_edit_tags(
        &action.group.local_id,
        action.name.as_deref(),
        action.about.as_deref(),
        action.picture.as_deref(),
        action.visibility,
        action.access,
        // `parent` stays out of edit_metadata (set_parent owns re-parenting).
        None,
    );
    PublishPlan::pinned(&action.group, KIND_EDIT_METADATA, "", tags)
}

fn validate(action: &EditMetadataInput) -> Result<(), ActionRejection> {
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
    if !action.edits_something() {
        return Err(ActionRejection::Invalid(
            "edit_metadata must change at least one of name/about/picture/visibility/access".into(),
        ));
    }
    edit_metadata_plan(action)
        .validate_no_unpinned_h()
        .map_err(|_| ActionRejection::Invalid("missing host pin for edit-metadata".into()))
}

/// `nmp.nip29.edit_metadata` — edit an existing group's name/about/picture/
/// visibility/access (kind:9002 edit-metadata).
pub struct EditMetadataAction;
impl ActionModule for EditMetadataAction {
    const NAMESPACE: &'static str = "nmp.nip29.edit_metadata";
    type Action = EditMetadataInput;

    /// ADR-0064 / S9 (#1747): opt into the typed FlatBuffers payload doorway;
    /// the fail-closed `schema_version` gate runs in `decode` (BEFORE `start`).
    fn decode_payload(bytes: &[u8]) -> Option<Result<Self::Action, ActionPayloadDecodeError>> {
        Some(<EditMetadataInput as ActionPayload>::decode(bytes))
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
        send(edit_metadata_plan(&action).into_actor_command(Some(correlation_id.to_string()))?);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_core::actor::PublishCommand;
    use std::cell::RefCell;

    fn group() -> GroupId {
        GroupId::new("wss://groups.example.com", "rust-nostr")
    }

    fn run_execute(input: EditMetadataInput) -> Result<Vec<ActorCommand>, String> {
        let captured: RefCell<Vec<ActorCommand>> = RefCell::new(Vec::new());
        EditMetadataAction.execute(
            &ActionContext::default(),
            input,
            "cid-edit",
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
    fn name_edit_emits_9002_with_h_and_name() {
        let action = EditMetadataInput {
            group: group(),
            name: Some("Renamed Room".into()),
            ..Default::default()
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
                assert_eq!(correlation_id.as_deref(), Some("cid-edit"));
            }
            other => panic!("expected 9002 publish, got {other:?}"),
        }
        let t = tags(&cmds);
        assert!(t.iter().any(|x| x == &vec!["h".to_string(), "rust-nostr".to_string()]));
        assert!(t.iter().any(|x| x == &vec!["name".to_string(), "Renamed Room".to_string()]));
    }

    #[test]
    fn visibility_and_access_edit_emit_markers() {
        let action = EditMetadataInput {
            group: group(),
            visibility: Some(GroupVisibility::Private),
            access: Some(GroupAccess::Closed),
            ..Default::default()
        };
        let cmds = run_execute(action).expect("executes");
        let t = tags(&cmds);
        assert!(t.iter().any(|x| x == &vec!["private".to_string()]));
        assert!(t.iter().any(|x| x == &vec!["closed".to_string()]));
        // No name/about/picture tags when those are None.
        assert!(!t.iter().any(|x| x.first() == Some(&"name".to_string())));
    }

    #[test]
    fn about_and_picture_edit_emit_tags() {
        let action = EditMetadataInput {
            group: group(),
            about: Some("New description".into()),
            picture: Some("https://x/p.png".into()),
            ..Default::default()
        };
        let cmds = run_execute(action).expect("executes");
        let t = tags(&cmds);
        assert!(t.iter().any(|x| x == &vec!["about".to_string(), "New description".to_string()]));
        assert!(t.iter().any(|x| x == &vec!["picture".to_string(), "https://x/p.png".to_string()]));
    }

    #[test]
    fn no_op_edit_is_rejected() {
        let mut ctx = ActionContext::default();
        let action = EditMetadataInput {
            group: group(),
            ..Default::default()
        };
        assert!(matches!(
            EditMetadataAction.start(&mut ctx, action),
            Err(ActionRejection::Invalid(_))
        ));
    }

    #[test]
    fn blank_strings_only_is_rejected_as_no_op() {
        let mut ctx = ActionContext::default();
        let action = EditMetadataInput {
            group: group(),
            name: Some("   ".into()),
            about: Some("".into()),
            ..Default::default()
        };
        assert!(matches!(
            EditMetadataAction.start(&mut ctx, action),
            Err(ActionRejection::Invalid(_))
        ));
    }

    #[test]
    fn non_websocket_host_is_rejected() {
        let mut ctx = ActionContext::default();
        let action = EditMetadataInput {
            group: GroupId::new("https://groups.example.com", "room"),
            name: Some("X".into()),
            ..Default::default()
        };
        assert!(matches!(
            EditMetadataAction.start(&mut ctx, action),
            Err(ActionRejection::Invalid(_))
        ));
    }

    #[test]
    fn well_formed_edit_passes_validator() {
        let mut ctx = ActionContext::default();
        let action = EditMetadataInput {
            group: group(),
            name: Some("Room".into()),
            ..Default::default()
        };
        assert!(EditMetadataAction.start(&mut ctx, action).is_ok());
    }

    #[test]
    fn optional_fields_default_to_none_in_json() {
        let json = concat!(
            "{",
            "\"group\":{\"host_relay_url\":\"wss://groups.example.com\",\"local_id\":\"room\"},",
            "\"name\":\"Room\"",
            "}"
        );
        let parsed: EditMetadataInput = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.about, None);
        assert_eq!(parsed.picture, None);
        assert_eq!(parsed.visibility, None);
        assert_eq!(parsed.access, None);
    }
}
