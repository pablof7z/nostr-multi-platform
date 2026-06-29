use nmp_core::subs::{SubIdentity, SubKey, SubOwnerKey, SubScope};
use nmp_core::substrate::ViewDependencies;
use nmp_kinds::{KIND_REACTION, KIND_SHORT_TEXT_NOTE};
use nmp_planner::stable_hash::stable_hash64;
use nmp_planner::{InterestId, InterestLifecycle, InterestScope, LogicalInterest};

use crate::action::{VisibleNoteRelationsAction, VISIBLE_NOTE_RELATIONS_NAMESPACE};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VisibleNoteRelationInterest {
    pub lane: &'static str,
    pub identity: SubIdentity,
    pub interest: LogicalInterest,
}

#[must_use]
pub fn visible_note_relation_interests(
    action: &VisibleNoteRelationsAction,
) -> Result<Vec<VisibleNoteRelationInterest>, String> {
    validate_visible_note_relations_action(action)?;
    let target = TargetRef::for_action(action);
    let lanes = relation_lanes(action, &target);
    Ok(lanes
        .into_iter()
        .map(|lane| VisibleNoteRelationInterest {
            lane: lane.name,
            identity: visible_note_relation_identity(action, &lane),
            interest: visible_note_relation_interest(action, &lane),
        })
        .collect())
}

pub fn validate_visible_note_relations_action(
    action: &VisibleNoteRelationsAction,
) -> Result<(), String> {
    if !is_hex64(action.target_event_id.trim()) {
        return Err("visible_note_relations requires a 64-hex target_event_id".to_string());
    }
    if action.target_kind == 0 {
        return Err("visible_note_relations requires a non-zero target_kind".to_string());
    }
    if action.consumer_id.trim().is_empty() {
        return Err("visible_note_relations requires a non-empty consumer_id".to_string());
    }
    if let Some(address) = action.target_address.as_deref() {
        let coord = nmp_nip18::AddressCoordinate::parse(address.trim()).ok_or_else(|| {
            "visible_note_relations target_address must be kind:pubkey:d".to_string()
        })?;
        if coord.kind != action.target_kind {
            return Err(
                "visible_note_relations target_address kind must match target_kind".to_string(),
            );
        }
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum TargetRef {
    Event(String),
    Address(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RelationLane {
    name: &'static str,
    kind: u32,
    tag: &'static str,
    value: String,
}

impl TargetRef {
    fn for_action(action: &VisibleNoteRelationsAction) -> Self {
        action
            .target_address
            .as_deref()
            .map(str::trim)
            .filter(|address| !address.is_empty())
            .map(|address| Self::Address(address.to_string()))
            .unwrap_or_else(|| Self::Event(action.target_event_id.trim().to_string()))
    }

    fn event_tag(&self, event_id: &str) -> (&'static str, String) {
        match self {
            Self::Event(value) => ("e", value.clone()),
            Self::Address(_) => ("e", event_id.to_string()),
        }
    }

    fn root_comment_tag(&self) -> (&'static str, String) {
        match self {
            Self::Event(value) => ("E", value.clone()),
            Self::Address(value) => ("A", value.clone()),
        }
    }

    fn wrapper_tag(&self) -> (&'static str, String) {
        match self {
            Self::Event(value) => ("e", value.clone()),
            Self::Address(value) => ("a", value.clone()),
        }
    }
}

fn relation_lanes(action: &VisibleNoteRelationsAction, target: &TargetRef) -> Vec<RelationLane> {
    let target_kind = action.target_kind;
    let (reaction_tag, reaction_value) = target.event_tag(action.target_event_id.trim());
    let (comment_tag, comment_value) = target.root_comment_tag();
    let (wrapper_tag, wrapper_value) = target.wrapper_tag();
    let is_note = target_kind == KIND_SHORT_TEXT_NOTE;

    let mut lanes = Vec::new();
    lanes.push(RelationLane {
        name: "replies",
        kind: if is_note {
            KIND_SHORT_TEXT_NOTE
        } else {
            nmp_nip22::KIND_NIP22_COMMENT
        },
        tag: if is_note { "e" } else { comment_tag },
        value: if is_note {
            action.target_event_id.trim().to_string()
        } else {
            comment_value.clone()
        },
    });
    lanes.push(RelationLane {
        name: "reactions",
        kind: KIND_REACTION,
        tag: reaction_tag,
        value: reaction_value,
    });
    lanes.push(RelationLane {
        name: "reposts",
        kind: if is_note {
            nmp_nip18::KIND_REPOST
        } else {
            nmp_nip18::KIND_GENERIC_REPOST
        },
        tag: wrapper_tag,
        value: wrapper_value.clone(),
    });
    lanes.push(RelationLane {
        name: "zaps",
        kind: nmp_nip57::KIND_ZAP_RECEIPT,
        tag: wrapper_tag,
        value: wrapper_value,
    });
    lanes.push(RelationLane {
        name: "comments",
        kind: nmp_nip22::KIND_NIP22_COMMENT,
        tag: comment_tag,
        value: comment_value,
    });
    lanes
}

fn visible_note_relation_interest(
    action: &VisibleNoteRelationsAction,
    lane: &RelationLane,
) -> LogicalInterest {
    ViewDependencies {
        kinds: vec![lane.kind],
        tag_refs: vec![(lane.tag.to_string(), lane.value.clone())],
        ..Default::default()
    }
    .into_logical_interest(
        visible_note_relation_interest_id(action, lane),
        InterestScope::ActiveAccount,
        InterestLifecycle::Tailing,
    )
}

fn visible_note_relation_identity(
    action: &VisibleNoteRelationsAction,
    lane: &RelationLane,
) -> SubIdentity {
    let target = target_key(action);
    SubIdentity::new(
        SubOwnerKey::new((
            VISIBLE_NOTE_RELATIONS_NAMESPACE,
            "owner",
            target.as_str(),
            action.consumer_id.trim(),
            lane.name,
        )),
        SubKey::builder(VISIBLE_NOTE_RELATIONS_NAMESPACE)
            .with(target.as_str())
            .with(action.target_kind)
            .with(lane.name)
            .finish(),
        SubScope::Global,
    )
}

fn visible_note_relation_interest_id(
    action: &VisibleNoteRelationsAction,
    lane: &RelationLane,
) -> InterestId {
    let target = target_key(action);
    InterestId(stable_hash64((
        VISIBLE_NOTE_RELATIONS_NAMESPACE,
        target.as_str(),
        action.target_kind,
        lane.name,
    )))
}

fn target_key(action: &VisibleNoteRelationsAction) -> String {
    action
        .target_address
        .as_deref()
        .map(str::trim)
        .filter(|address| !address.is_empty())
        .unwrap_or_else(|| action.target_event_id.trim())
        .to_string()
}

fn is_hex64(s: &str) -> bool {
    s.len() == 64 && s.as_bytes().iter().all(u8::is_ascii_hexdigit)
}

#[cfg(test)]
#[path = "visible_relations_tests.rs"]
mod tests;
