use nmp_core::planner::{InterestId, InterestLifecycle, InterestScope, LogicalInterest};
use nmp_core::stable_hash::stable_hash64;
use nmp_core::subs::{SubIdentity, SubKey, SubOwnerKey, SubScope};
use nmp_core::substrate::{
    ActionContext, ActionModule, ActionRegistrar, ActionRejection, ViewDependencies,
};
use nmp_core::ActorCommand;
use serde::{Deserialize, Serialize};

use crate::KIND_SHORT_NOTE;

pub const VISIBLE_NOTE_RELATIONS_LIMIT: u32 = 200;
pub const VISIBLE_NOTE_RELATIONS_NAMESPACE: &str = "nmp.nip01.visible_note_relations";

#[must_use]
pub fn visible_note_relations_interest_id(event_id: &str) -> InterestId {
    InterestId(stable_hash64(("nmp.visible-note-relations", event_id)))
}

#[must_use]
pub fn visible_note_relations_interest(event_id: &str) -> LogicalInterest {
    ViewDependencies {
        kinds: vec![
            KIND_SHORT_NOTE,
            nmp_nip18::KIND_REPOST,
            7,
            nmp_nip57::KIND_ZAP_RECEIPT,
        ],
        tag_refs: vec![("e".to_string(), event_id.to_string())],
        limit: Some(VISIBLE_NOTE_RELATIONS_LIMIT),
        ..Default::default()
    }
    .into_logical_interest(
        visible_note_relations_interest_id(event_id),
        InterestScope::Global,
        InterestLifecycle::Tailing,
    )
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "op")]
pub enum VisibleNoteRelationsAction {
    Claim {
        event_id: String,
        consumer_id: String,
    },
    Release {
        event_id: String,
        consumer_id: String,
    },
}

pub struct VisibleNoteRelationsModule;

impl ActionModule for VisibleNoteRelationsModule {
    const NAMESPACE: &'static str = VISIBLE_NOTE_RELATIONS_NAMESPACE;
    type Action = VisibleNoteRelationsAction;

    fn start(_ctx: &mut ActionContext, action: Self::Action) -> Result<(), ActionRejection> {
        let (event_id, consumer_id) = action.parts();
        if !is_hex64(event_id) {
            return Err(ActionRejection::Invalid(
                "visible note relations requires a 64-hex event_id".to_string(),
            ));
        }
        if consumer_id.is_empty() {
            return Err(ActionRejection::Invalid(
                "visible note relations requires a consumer_id".to_string(),
            ));
        }
        Ok(())
    }

    fn execute(
        action: Self::Action,
        _correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        match action {
            VisibleNoteRelationsAction::Claim {
                event_id,
                consumer_id,
            } => send(ActorCommand::EnsureInterest {
                identity: visible_note_relations_identity(&event_id, &consumer_id),
                interest: visible_note_relations_interest(&event_id),
            }),
            VisibleNoteRelationsAction::Release {
                event_id,
                consumer_id,
            } => send(ActorCommand::DropInterestOwner(
                visible_note_relations_identity(&event_id, &consumer_id),
            )),
        }
        Ok(())
    }
}

impl VisibleNoteRelationsAction {
    fn parts(&self) -> (&str, &str) {
        match self {
            Self::Claim {
                event_id,
                consumer_id,
            }
            | Self::Release {
                event_id,
                consumer_id,
            } => (event_id, consumer_id),
        }
    }
}

#[must_use]
pub fn visible_note_relations_identity(event_id: &str, consumer_id: &str) -> SubIdentity {
    SubIdentity::new(
        SubOwnerKey::new(("nmp.visible-note-relations.owner", event_id, consumer_id)),
        SubKey::builder("nmp.visible-note-relations")
            .with(event_id)
            .finish(),
        SubScope::Global,
    )
}

pub fn register_visible_note_relation_actions(app: &mut impl ActionRegistrar) {
    app.register_action::<VisibleNoteRelationsModule>();
}

fn is_hex64(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    const EVENT: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    #[test]
    fn interest_targets_all_note_relation_kinds_by_e_tag() {
        let interest = visible_note_relations_interest(EVENT);
        assert_eq!(interest.id, visible_note_relations_interest_id(EVENT));
        assert!(interest.shape.kinds.contains(&1));
        assert!(interest.shape.kinds.contains(&6));
        assert!(interest.shape.kinds.contains(&7));
        assert!(interest.shape.kinds.contains(&9735));
        assert_eq!(interest.shape.limit, Some(VISIBLE_NOTE_RELATIONS_LIMIT));
        assert!(interest
            .shape
            .tags
            .get("e")
            .is_some_and(|targets| targets.contains(EVENT)));
    }

    #[test]
    fn action_module_claim_and_release_use_refcounted_interest_identity() {
        let mut ctx = ActionContext::default();
        let claim = VisibleNoteRelationsAction::Claim {
            event_id: EVENT.to_string(),
            consumer_id: "row".to_string(),
        };
        assert!(VisibleNoteRelationsModule::start(&mut ctx, claim.clone()).is_ok());

        let (tx, rx) = std::sync::mpsc::channel();
        VisibleNoteRelationsModule::execute(claim, "corr", &|cmd| {
            tx.send(cmd).expect("test channel accepts command");
        })
        .expect("claim action should enqueue");
        let mut cmds = rx.try_iter().collect::<Vec<_>>();
        let ActorCommand::EnsureInterest { identity, interest } = &cmds[0] else {
            panic!("expected EnsureInterest");
        };
        assert_eq!(*identity, visible_note_relations_identity(EVENT, "row"));
        assert_eq!(interest.id, visible_note_relations_interest_id(EVENT));

        let release = VisibleNoteRelationsAction::Release {
            event_id: EVENT.to_string(),
            consumer_id: "row".to_string(),
        };
        let (tx, rx) = std::sync::mpsc::channel();
        VisibleNoteRelationsModule::execute(release, "corr", &|cmd| {
            tx.send(cmd).expect("test channel accepts command");
        })
        .expect("release action should enqueue");
        cmds.extend(rx.try_iter());
        assert!(matches!(
            &cmds[1],
            ActorCommand::DropInterestOwner(identity)
                if *identity == visible_note_relations_identity(EVENT, "row")
        ));
    }
}
