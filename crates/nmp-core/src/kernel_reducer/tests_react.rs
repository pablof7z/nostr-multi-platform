//! Tests for [`KernelReducer::build_reaction_draft`].
//!
//! Core owns only author resolution and delegation to the registered protocol
//! builder. Protocol grammar is tested in the protocol crate.

use super::*;
use crate::slots::{ReactionDraft, ReactionDraftBuilder};
use crate::store::{RawEvent, VerifiedEvent};
use std::sync::Arc;

// ─── Synthetic event IDs (valid 64-char hex) ────────────────────────────────

const TARGET_ID: &str = "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1";
const TARGET_AUTHOR: &str = "b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2";

// ─── Tests ──────────────────────────────────────────────────────────────────

#[derive(Debug)]
struct EchoReactionBuilder;

impl ReactionDraftBuilder for EchoReactionBuilder {
    fn build_reaction_draft(
        &self,
        target_event_id: &str,
        target_author_pubkey: Option<&str>,
        reaction: &str,
    ) -> Option<ReactionDraft> {
        Some(ReactionDraft {
            tags: vec![vec![
                "builder".to_string(),
                target_event_id.to_string(),
                target_author_pubkey.unwrap_or("").to_string(),
            ]],
            content: reaction.to_string(),
        })
    }
}

#[test]
fn build_reaction_draft_returns_none_without_registered_builder() {
    let r = KernelReducer::new();
    assert!(r.build_reaction_draft(TARGET_ID, "+").is_none());
}

#[test]
fn build_reaction_draft_delegates_without_author_when_target_not_cached() {
    let mut r = KernelReducer::new();
    r.set_reaction_draft_builder(Arc::new(EchoReactionBuilder));
    let (tags, content) = r
        .build_reaction_draft(TARGET_ID, "+")
        .expect("registered builder should return Some");
    assert_eq!(tags[0][0], "builder");
    assert_eq!(tags[0][1], TARGET_ID);
    assert_eq!(tags[0][2], "");
    assert_eq!(content, "+");
}

#[test]
fn build_reaction_draft_delegates_with_resolved_author_when_cached() {
    // ingest_pre_verified_event populates BOTH the store AND self.events (the
    // HashMap read-cache that event_author reads). Store-only seeding is
    // insufficient here because event_author reads self.events, not the store.
    let mut r = KernelReducer::new();
    r.set_reaction_draft_builder(Arc::new(EchoReactionBuilder));
    let raw = RawEvent {
        id: TARGET_ID.to_string(),
        pubkey: TARGET_AUTHOR.to_string(),
        created_at: 1_700_000_000,
        kind: 1,
        tags: vec![],
        content: "hello".into(),
        sig: "0".repeat(128),
    };
    let verified = VerifiedEvent::from_raw_unchecked(raw);
    r.kernel
        .ingest_pre_verified_event(nmp_network::role::RelayRole::Content, "sub-test", verified);

    let (tags, content) = r
        .build_reaction_draft(TARGET_ID, "+")
        .expect("registered builder should return Some after ingest");
    assert_eq!(tags[0][0], "builder");
    assert_eq!(tags[0][1], TARGET_ID);
    assert_eq!(tags[0][2], TARGET_AUTHOR);
    assert_eq!(content, "+");
}
