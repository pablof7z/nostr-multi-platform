use nmp_core::substrate::{KernelEvent, ViewDependencies};
use nmp_kinds::{KIND_NIP22_COMMENT, KIND_SHORT_TEXT_NOTE};
use nmp_nip01::try_from_kernel_event as note_from_kernel_event;
use nmp_nip22::try_from_kernel_event as comment_from_kernel_event;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

use crate::target::ReplyTarget;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ReplyProtocol {
    Nip10,
    Nip22,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ReplyReadMode {
    Direct,
    Thread,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ReplyReadPlanError {
    CommentEventRequiresRecord,
}

impl core::fmt::Display for ReplyReadPlanError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::CommentEventRequiresRecord => {
                write!(f, "kind:1111 reply reads require a decoded CommentRecord")
            }
        }
    }
}

impl std::error::Error for ReplyReadPlanError {}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReplyReadPlan {
    pub protocol: ReplyProtocol,
    pub mode: ReplyReadMode,
    pub target: ReplyTarget,
    pub dependencies: ViewDependencies,
}

impl ReplyReadPlan {
    pub fn direct(target: ReplyTarget) -> Result<Self, ReplyReadPlanError> {
        Self::new(target, ReplyReadMode::Direct)
    }

    pub fn thread(target: ReplyTarget) -> Result<Self, ReplyReadPlanError> {
        Self::new(target, ReplyReadMode::Thread)
    }

    fn new(target: ReplyTarget, mode: ReplyReadMode) -> Result<Self, ReplyReadPlanError> {
        let protocol = if target.is_nip10() {
            ReplyProtocol::Nip10
        } else {
            ReplyProtocol::Nip22
        };
        let dependencies = dependencies_for(&target, mode)?;
        Ok(Self {
            protocol,
            mode,
            target,
            dependencies,
        })
    }

    #[must_use]
    pub fn filter_json(&self) -> String {
        // BTreeMap serializes keys in sorted order regardless of serde_json's
        // `preserve_order` feature, so the filter string is deterministic across
        // workspace feature-unification.
        let mut map: BTreeMap<String, Value> = BTreeMap::new();
        if !self.dependencies.kinds.is_empty() {
            map.insert(
                "kinds".to_string(),
                Value::Array(
                    self.dependencies
                        .kinds
                        .iter()
                        .map(|kind| Value::from(*kind))
                        .collect(),
                ),
            );
        }
        for (tag, value) in &self.dependencies.tag_refs {
            map.insert(
                format!("#{tag}"),
                Value::Array(vec![Value::String(value.clone())]),
            );
        }
        if let Some(limit) = self.dependencies.limit {
            map.insert("limit".to_string(), Value::from(limit));
        }
        serde_json::to_string(&map).expect("filter map serializes")
    }

    #[must_use]
    pub fn accepts(&self, event: &KernelEvent) -> bool {
        match self.protocol {
            ReplyProtocol::Nip10 => self.accepts_nip10(event),
            ReplyProtocol::Nip22 => self.accepts_nip22(event),
        }
    }

    fn accepts_nip10(&self, event: &KernelEvent) -> bool {
        let Some(note) = note_from_kernel_event(event) else {
            return false;
        };
        let target_id = match &self.target {
            ReplyTarget::Note(note) => note.event_id.as_str(),
            ReplyTarget::Event(event) => event.event_id.as_str(),
            _ => return false,
        };
        match self.mode {
            ReplyReadMode::Direct => note
                .refs
                .reply
                .as_ref()
                .is_some_and(|reply| reply.id == target_id),
            ReplyReadMode::Thread => {
                event.id == target_id
                    || note
                        .refs
                        .reply
                        .as_ref()
                        .is_some_and(|reply| reply.id == target_id)
                    || note
                        .refs
                        .root
                        .as_ref()
                        .is_some_and(|root| root.id == target_id)
            }
        }
    }

    fn accepts_nip22(&self, event: &KernelEvent) -> bool {
        let Some(comment) = comment_from_kernel_event(event) else {
            return false;
        };
        let anchor = match nip22_anchor(&self.target) {
            Ok(anchor) => anchor,
            Err(_) => return false,
        };
        match self.mode {
            ReplyReadMode::Direct => {
                comment.parent_tag_name == anchor.direct_parent_tag
                    && comment.parent_tag_value == anchor.direct_parent_value
            }
            ReplyReadMode::Thread => {
                comment.root_tag_name == anchor.root_tag
                    && comment.root_tag_value == anchor.root_value
            }
        }
    }
}

fn dependencies_for(
    target: &ReplyTarget,
    mode: ReplyReadMode,
) -> Result<ViewDependencies, ReplyReadPlanError> {
    if target.is_nip10() {
        let target_id = match target {
            ReplyTarget::Note(note) => note.event_id.clone(),
            ReplyTarget::Event(event) => event.event_id.clone(),
            _ => unreachable!("is_nip10 only covers note/event"),
        };
        return Ok(ViewDependencies {
            kinds: vec![KIND_SHORT_TEXT_NOTE],
            tag_refs: vec![("e".to_string(), target_id)],
            ..Default::default()
        });
    }

    let anchor = nip22_anchor(target)?;
    let (tag, value) = match mode {
        ReplyReadMode::Direct => (anchor.query_tag, anchor.query_value),
        ReplyReadMode::Thread => (anchor.root_tag, anchor.root_value),
    };
    Ok(ViewDependencies {
        kinds: vec![KIND_NIP22_COMMENT],
        tag_refs: vec![(tag, value)],
        ..Default::default()
    })
}

struct Nip22Anchor {
    root_tag: String,
    root_value: String,
    direct_parent_tag: String,
    direct_parent_value: String,
    query_tag: String,
    query_value: String,
}

fn nip22_anchor(target: &ReplyTarget) -> Result<Nip22Anchor, ReplyReadPlanError> {
    match target {
        ReplyTarget::Comment(comment) => Ok(Nip22Anchor {
            root_tag: comment.root_tag_name.clone(),
            root_value: comment.root_tag_value.clone(),
            direct_parent_tag: "e".to_string(),
            direct_parent_value: comment.event_id.clone(),
            query_tag: "e".to_string(),
            query_value: comment.event_id.clone(),
        }),
        ReplyTarget::Event(event) if event.kind == KIND_NIP22_COMMENT => {
            Err(ReplyReadPlanError::CommentEventRequiresRecord)
        }
        ReplyTarget::Event(event) => Ok(Nip22Anchor {
            root_tag: "E".to_string(),
            root_value: event.event_id.clone(),
            direct_parent_tag: "e".to_string(),
            direct_parent_value: event.event_id.clone(),
            query_tag: "E".to_string(),
            query_value: event.event_id.clone(),
        }),
        ReplyTarget::Address(address) => Ok(Nip22Anchor {
            root_tag: "A".to_string(),
            root_value: address.coordinate.clone(),
            direct_parent_tag: "a".to_string(),
            direct_parent_value: address.coordinate.clone(),
            query_tag: "A".to_string(),
            query_value: address.coordinate.clone(),
        }),
        ReplyTarget::External(external) => Ok(Nip22Anchor {
            root_tag: "I".to_string(),
            root_value: external.uri.clone(),
            direct_parent_tag: "i".to_string(),
            direct_parent_value: external.uri.clone(),
            query_tag: "I".to_string(),
            query_value: external.uri.clone(),
        }),
        ReplyTarget::Note(_) => unreachable!("NIP-10 handled before nip22_anchor"),
    }
}

#[cfg(test)]
#[path = "read_tests.rs"]
mod tests;
