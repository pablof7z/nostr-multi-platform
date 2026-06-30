use nmp_core::actor::{ActorCommand, InterestsCommand};
use nmp_core::substrate::{
    ActionContext, ActionModule, ActionPayload, ActionPayloadDecodeError, ActionRegistrar,
    ActionRejection, ProtocolDescriptor,
};
use serde::{Deserialize, Serialize};

use crate::visible_relations::{
    validate_visible_note_relations_action, visible_note_relation_interests,
};

pub const VISIBLE_NOTE_RELATIONS_NAMESPACE: &str = "nmp.nip01.visible_note_relations";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VisibleNoteRelationsLifecycle {
    Claim,
    Release,
}

/// Claim or release relation-count subscriptions for one visible note row.
///
/// `target_address` is optional because non-addressable events are identified
/// by id. For addressable targets, provide the NIP-01 `kind:pubkey:d`
/// coordinate so relation wrappers that tag the address (`#a`/`#A`) can be
/// queried without forcing shells to know protocol policy.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct VisibleNoteRelationsAction {
    pub lifecycle: VisibleNoteRelationsLifecycle,
    pub target_event_id: String,
    pub target_kind: u32,
    pub consumer_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_address: Option<String>,
}

pub struct VisibleNoteRelationsModule;

impl ActionModule for VisibleNoteRelationsModule {
    const NAMESPACE: &'static str = VISIBLE_NOTE_RELATIONS_NAMESPACE;
    type Action = VisibleNoteRelationsAction;

    fn decode_payload(bytes: &[u8]) -> Option<Result<Self::Action, ActionPayloadDecodeError>> {
        Some(<Self::Action as ActionPayload>::decode(bytes))
    }

    fn start(&self, _ctx: &mut ActionContext, action: Self::Action) -> Result<(), ActionRejection> {
        validate_visible_note_relations_action(&action).map_err(ActionRejection::Invalid)
    }

    fn execute(
        &self,
        _ctx: &ActionContext,
        action: Self::Action,
        _correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        let interests = visible_note_relation_interests(&action)?;
        match action.lifecycle {
            VisibleNoteRelationsLifecycle::Claim => {
                for item in interests {
                    send(ActorCommand::Interests(InterestsCommand::EnsureInterest {
                        identity: item.identity,
                        interest: item.interest,
                    }));
                }
            }
            VisibleNoteRelationsLifecycle::Release => {
                for item in interests {
                    send(ActorCommand::Interests(
                        InterestsCommand::DropInterestOwner(item.identity),
                    ));
                }
            }
        }
        Ok(())
    }
}

pub struct RelationsDescriptor;

impl ProtocolDescriptor for RelationsDescriptor {
    fn register_actions(&self, app: &mut impl ActionRegistrar) {
        register_visible_note_relation_actions(app);
    }
}

pub fn register_actions(app: &mut impl ActionRegistrar) {
    register_visible_note_relation_actions(app);
}

pub fn register_visible_note_relation_actions(app: &mut impl ActionRegistrar) {
    app.register_action(VisibleNoteRelationsModule)
        .expect("duplicate registration: nmp-relations VisibleNoteRelationsModule");
    // doctrine-allow: D6 — startup-only call; duplicate wiring is a programmer error
}
