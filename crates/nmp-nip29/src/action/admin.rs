//! Admin-only actions (signer must be in latest 39001 except for `CreateGroup`,
//! which has no admin check per `kinds.md` §2.3). All emit host-pinned plans.

use nmp_core::substrate::{ActionContext, ActionModule, ActionRejection};
use nmp_core::ActorCommand;
use serde::{Deserialize, Serialize};

use crate::group_id::GroupId;
use crate::kinds::{
    KIND_CREATE_GROUP, KIND_CREATE_INVITE, KIND_DELETE_EVENT, KIND_DELETE_GROUP,
    KIND_EDIT_METADATA, KIND_PUT_USER, KIND_REMOVE_USER,
};

use super::publish_plan::PublishPlan;

/// Generate an admin `ActionModule` impl plus its executor command function.
///
/// `$namespace` is the snake_case wire suffix; the generated `NAMESPACE`
/// constant is `concat!("nip29.", $namespace)`. It is passed explicitly (not
/// derived from the CamelCase `$Module` ident) so admin verbs match the
/// snake_case convention every other NIP-29 namespace already uses
/// (`nip29.join_request`, `nip29.post_chat_message`, …) — the wire format is
/// a public API and must stay consistent across all 15 namespaces.
///
/// `$build_plan` is the single source of truth for the action's wire shape:
/// [`$Module::start`] consults it for validation, and the generated
/// `$command_fn` consults the same closure so the executor can never drift
/// from the validator. `$command_fn` parses the typed input from JSON, builds
/// the plan, and converts it into an [`ActorCommand`] via
/// [`PublishPlan::into_actor_command`] — the bridge that finally lets these
/// dormant validators drive a real publish.
///
/// `$require` is a closure `fn(&$Input) -> Result<(), &'static str>` checking
/// per-action required fields BEFORE the plan is built. Actions whose plan
/// builder reads an `Option` field that NIP-29 marks mandatory (the `p` tag on
/// `put_user`/`remove_user`, the `e` tag on `delete_event`) MUST pass a real
/// check here — otherwise a missing field silently becomes an empty wire value
/// (`["p", ""]`) and `dispatch_action` returns a success `correlation_id` for
/// a malformed admin event published under the user's signature (a D6
/// violation: a missing required field must be rejection data, never a
/// published artifact). Actions with no required fields beyond the group
/// (`create_group`, `delete_group`, `edit_metadata`, `create_invite`) pass the
/// always-accepting `|_| Ok(())`.
macro_rules! admin_action {
    ($Module:ident, $Input:ident, $command_fn:ident, $namespace:literal, $kind_const:expr, $require:expr, $build_plan:expr) => {
        #[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
        pub struct $Input {
            pub group: GroupId,
            #[serde(default)]
            pub fields: ActionFields,
        }

        pub struct $Module;
        impl ActionModule for $Module {
            const NAMESPACE: &'static str = concat!("nip29.", $namespace);
            type Action = $Input;
            fn start(
                _ctx: &mut ActionContext,
                action: Self::Action,
            ) -> Result<(), ActionRejection> {
                let require: fn(&$Input) -> Result<(), &'static str> = $require;
                require(&action).map_err(|msg| ActionRejection::Invalid(msg.into()))?;
                let plan: PublishPlan = $build_plan(&action);
                if plan.validate_no_unpinned_h().is_err() {
                    return Err(ActionRejection::Invalid(
                        "missing host pin for group event".into(),
                    ));
                }
                let _ = $kind_const; // sanity-link the constant
                Ok(())
            }
        }

        /// Map a validated admin action JSON to the [`ActorCommand`] that
        /// publishes its group event, host-pinned to the group's own relay.
        /// Re-decodes its own input AND re-runs the required-field check — the
        /// executor never trusts an upstream shape it did not verify, so a
        /// missing mandatory field is an `Err` here too, never a malformed
        /// published event.
        pub fn $command_fn(action_json: &str) -> Result<ActorCommand, String> {
            let input: $Input =
                serde_json::from_str(action_json).map_err(|e| e.to_string())?;
            let require: fn(&$Input) -> Result<(), &'static str> = $require;
            require(&input).map_err(|msg| msg.to_string())?;
            let build = $build_plan;
            build(&input).into_actor_command()
        }
    };
}

/// Free-form fields shared across the admin actions; per-action validation of
/// required vs optional happens in the plan builders below.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct ActionFields {
    pub target_pubkey: Option<String>,
    pub target_event_id: Option<String>,
    pub role: Option<String>,
    pub reason: Option<String>,
    pub name: Option<String>,
    pub about: Option<String>,
    pub picture: Option<String>,
    pub visibility_private: Option<bool>,
    pub access_closed: Option<bool>,
    pub restricted: Option<bool>,
    #[serde(default)]
    pub invite_codes: Vec<String>,
}

fn group_h_tag(group: &GroupId) -> Vec<String> {
    vec!["h".into(), group.local_id.clone()]
}

/// Reject an absent or empty optional field that the action's wire shape
/// treats as mandatory. `serde(default)` lets an admin action JSON omit the
/// whole `fields` object, so a required tag value can arrive as `None` (or as
/// `Some("")`); either way the published event would carry a blank tag value.
fn require_nonempty(field: &Option<String>, msg: &'static str) -> Result<(), &'static str> {
    match field {
        Some(v) if !v.is_empty() => Ok(()),
        _ => Err(msg),
    }
}

admin_action!(CreateGroupAction, CreateGroupInput, create_group_command, "create_group", KIND_CREATE_GROUP, |_| Ok(()), |a: &CreateGroupInput| {
    PublishPlan::pinned(&a.group, KIND_CREATE_GROUP, "", vec![group_h_tag(&a.group)])
});

admin_action!(EditMetadataAction, EditMetadataInput, edit_metadata_command, "edit_metadata", KIND_EDIT_METADATA, |_| Ok(()), |a: &EditMetadataInput| {
    let mut tags = vec![group_h_tag(&a.group)];
    if let Some(name) = &a.fields.name { tags.push(vec!["name".into(), name.clone()]); }
    if let Some(about) = &a.fields.about { tags.push(vec!["about".into(), about.clone()]); }
    if let Some(picture) = &a.fields.picture { tags.push(vec!["picture".into(), picture.clone()]); }
    if matches!(a.fields.visibility_private, Some(true)) { tags.push(vec!["private".into()]); }
    if matches!(a.fields.access_closed, Some(true)) { tags.push(vec!["closed".into()]); }
    if matches!(a.fields.restricted, Some(true)) { tags.push(vec!["restricted".into()]); }
    PublishPlan::pinned(&a.group, KIND_EDIT_METADATA, "", tags)
});

// kind:9000 carries the moderated user as a `["p", <pubkey>]` tag — the
// pubkey is mandatory, so reject a missing `target_pubkey` rather than
// publishing `["p", ""]`.
admin_action!(PutUserAction, PutUserInput, put_user_command, "put_user", KIND_PUT_USER,
    |a: &PutUserInput| require_nonempty(&a.fields.target_pubkey, "put_user requires fields.target_pubkey"),
    |a: &PutUserInput| {
    let pubkey = a.fields.target_pubkey.clone().unwrap_or_default();
    let mut p_tag = vec!["p".into(), pubkey];
    if let Some(role) = &a.fields.role { p_tag.push(role.clone()); }
    let mut tags = vec![group_h_tag(&a.group), p_tag];
    if let Some(reason) = &a.fields.reason { tags.push(vec!["reason".into(), reason.clone()]); }
    PublishPlan::pinned(&a.group, KIND_PUT_USER, "", tags)
});

// kind:9001 carries the removed user as a `["p", <pubkey>]` tag — mandatory.
admin_action!(RemoveUserAction, RemoveUserInput, remove_user_command, "remove_user", KIND_REMOVE_USER,
    |a: &RemoveUserInput| require_nonempty(&a.fields.target_pubkey, "remove_user requires fields.target_pubkey"),
    |a: &RemoveUserInput| {
    let pubkey = a.fields.target_pubkey.clone().unwrap_or_default();
    let mut tags = vec![group_h_tag(&a.group), vec!["p".into(), pubkey]];
    if let Some(reason) = &a.fields.reason { tags.push(vec!["reason".into(), reason.clone()]); }
    PublishPlan::pinned(&a.group, KIND_REMOVE_USER, "", tags)
});

admin_action!(CreateInviteAction, CreateInviteInput, create_invite_command, "create_invite", KIND_CREATE_INVITE, |_| Ok(()), |a: &CreateInviteInput| {
    let mut tags = vec![group_h_tag(&a.group)];
    for code in &a.fields.invite_codes {
        tags.push(vec!["code".into(), code.clone()]);
    }
    PublishPlan::pinned(&a.group, KIND_CREATE_INVITE, "", tags)
});

// kind:9005 carries the event to delete as a `["e", <event_id>]` tag — an
// empty value is a "delete nothing" admin event under the user's signature.
admin_action!(DeleteEventAction, DeleteEventInput, delete_event_command, "delete_event", KIND_DELETE_EVENT,
    |a: &DeleteEventInput| require_nonempty(&a.fields.target_event_id, "delete_event requires fields.target_event_id"),
    |a: &DeleteEventInput| {
    let evt = a.fields.target_event_id.clone().unwrap_or_default();
    PublishPlan::pinned(
        &a.group,
        KIND_DELETE_EVENT,
        "",
        vec![group_h_tag(&a.group), vec!["e".into(), evt]],
    )
});

admin_action!(DeleteGroupAction, DeleteGroupInput, delete_group_command, "delete_group", KIND_DELETE_GROUP, |_| Ok(()), |a: &DeleteGroupInput| {
    PublishPlan::pinned(&a.group, KIND_DELETE_GROUP, "", vec![group_h_tag(&a.group)])
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_user_command_emits_host_pinned_kind_9000() {
        let json = r#"{"group":{"host_relay_url":"wss://groups.example.com","local_id":"room"},"fields":{"target_pubkey":"deadbeef","role":"moderator","reason":"trusted","invite_codes":[]}}"#;
        match put_user_command(json).expect("well-formed put-user") {
            ActorCommand::PublishUnsignedEventToRelays { event, relays } => {
                assert_eq!(relays, vec!["wss://groups.example.com".to_string()]);
                assert_eq!(event.kind, KIND_PUT_USER);
                assert!(event
                    .tags
                    .contains(&vec!["h".to_string(), "room".to_string()]));
                assert!(event.tags.contains(&vec![
                    "p".to_string(),
                    "deadbeef".to_string(),
                    "moderator".to_string()
                ]));
                assert!(event
                    .tags
                    .contains(&vec!["reason".to_string(), "trusted".to_string()]));
            }
            other => panic!("expected PublishUnsignedEventToRelays, got {other:?}"),
        }
    }

    #[test]
    fn create_group_command_emits_host_pinned_kind_9007() {
        let json = r#"{"group":{"host_relay_url":"wss://groups.example.com","local_id":"room"}}"#;
        match create_group_command(json).expect("well-formed create-group") {
            ActorCommand::PublishUnsignedEventToRelays { event, relays } => {
                assert_eq!(relays, vec!["wss://groups.example.com".to_string()]);
                assert_eq!(event.kind, KIND_CREATE_GROUP);
                assert_eq!(
                    event.tags,
                    vec![vec!["h".to_string(), "room".to_string()]]
                );
            }
            other => panic!("expected PublishUnsignedEventToRelays, got {other:?}"),
        }
    }

    #[test]
    fn admin_command_rejects_malformed_body() {
        assert!(delete_group_command(r#"{"no_group":true}"#).is_err());
    }

    /// A `put_user` with no `target_pubkey` must be rejected — otherwise the
    /// executor would publish a kind:9000 carrying `["p", ""]` (a malformed
    /// admin event under the user's signature).
    #[test]
    fn put_user_command_rejects_missing_target_pubkey() {
        let json = r#"{"group":{"host_relay_url":"wss://groups.example.com","local_id":"room"}}"#;
        let err = put_user_command(json).expect_err("missing target_pubkey must reject");
        assert!(err.contains("target_pubkey"), "got: {err}");
    }

    /// An empty-string `target_pubkey` is rejected too — `Some("")` is as
    /// malformed on the wire as `None`.
    #[test]
    fn put_user_command_rejects_empty_target_pubkey() {
        let json = r#"{"group":{"host_relay_url":"wss://groups.example.com","local_id":"room"},"fields":{"target_pubkey":""}}"#;
        assert!(put_user_command(json).is_err());
    }

    #[test]
    fn remove_user_command_rejects_missing_target_pubkey() {
        let json = r#"{"group":{"host_relay_url":"wss://groups.example.com","local_id":"room"}}"#;
        let err = remove_user_command(json).expect_err("missing target_pubkey must reject");
        assert!(err.contains("target_pubkey"), "got: {err}");
    }

    /// A `delete_event` with no `target_event_id` must be rejected — an empty
    /// `["e", ""]` tag is a "delete nothing" admin event.
    #[test]
    fn delete_event_command_rejects_missing_target_event_id() {
        let json = r#"{"group":{"host_relay_url":"wss://groups.example.com","local_id":"room"}}"#;
        let err = delete_event_command(json).expect_err("missing target_event_id must reject");
        assert!(err.contains("target_event_id"), "got: {err}");
    }

    /// The `start()` validator rejects the same missing-field case the executor
    /// does, so `dispatch_action` returns an error rather than a success
    /// `correlation_id` for a malformed admin event.
    #[test]
    fn put_user_start_rejects_missing_target_pubkey() {
        let action = PutUserInput {
            group: GroupId::new("wss://groups.example.com", "room"),
            fields: ActionFields::default(),
        };
        let mut ctx = ActionContext { now_ms: 0 };
        assert!(matches!(
            PutUserAction::start(&mut ctx, action),
            Err(ActionRejection::Invalid(_))
        ));
    }
}
