//! Draft-builder-specific publish command tests.
//!
//! Split from `command_apply_publish_tests.rs` to keep each reducer test file
//! below the repository hard size cap.

use super::*;
use crate::actor::{ActorCommand, PublishCommand};
use crate::publish::PublishTarget;
use crate::store::RawEvent;
use crate::substrate::DraftBuilderRegistrar;
use nmp_signer_iface::UnsignedEvent;
use std::sync::Arc;

const PK: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";
const RELAY: &str = "wss://relay.example";

fn install_fixture_draft_builders(r: &KernelReducer) {
    r.register_draft_builder(
        crate::substrate::DraftIntentKind::Reply,
        Arc::new(FixtureReplyBuilder),
    );
    r.register_draft_builder(
        crate::substrate::DraftIntentKind::Profile,
        Arc::new(FixtureProfileBuilder),
    );
}

struct FixtureReplyBuilder;

impl crate::substrate::DraftBuilder for FixtureReplyBuilder {
    fn build(
        &self,
        intent: &crate::substrate::DraftIntent,
        ctx: crate::substrate::DraftBuildContext<'_>,
    ) -> Result<UnsignedEvent, crate::substrate::DraftBuildError> {
        let crate::substrate::DraftIntent::Reply {
            content,
            reply_to_event_id,
        } = intent
        else {
            return Err(crate::substrate::DraftBuildError::new("wrong intent"));
        };
        let id = hex32(reply_to_event_id)
            .ok_or_else(|| crate::substrate::DraftBuildError::new("reply_target_invalid_hex"))?;
        let parent = ctx
            .event_store
            .get_by_id(&id)
            .map_err(|err| crate::substrate::DraftBuildError::new(err.to_string()))?
            .ok_or_else(|| crate::substrate::DraftBuildError::new("reply_target_unknown"))?;
        let root = parent
            .raw
            .tags
            .iter()
            .find(|tag| tag.get(3).map(String::as_str) == Some("root"))
            .and_then(|tag| tag.get(1))
            .cloned()
            .unwrap_or_else(|| reply_to_event_id.clone());
        Ok(UnsignedEvent {
            pubkey: ctx.author_pubkey.to_string(),
            kind: 1,
            tags: vec![
                crate::tags::e_tag(&root, None, Some("root")),
                crate::tags::e_tag(reply_to_event_id, None, Some("reply")),
                crate::tags::p_tag(&parent.raw.pubkey, None),
            ],
            content: content.clone(),
            created_at: ctx.created_at,
        })
    }
}

struct FixtureProfileBuilder;

impl crate::substrate::DraftBuilder for FixtureProfileBuilder {
    fn build(
        &self,
        intent: &crate::substrate::DraftIntent,
        ctx: crate::substrate::DraftBuildContext<'_>,
    ) -> Result<UnsignedEvent, crate::substrate::DraftBuildError> {
        let crate::substrate::DraftIntent::Profile { fields } = intent else {
            return Err(crate::substrate::DraftBuildError::new("wrong intent"));
        };
        Ok(UnsignedEvent {
            pubkey: ctx.author_pubkey.to_string(),
            kind: 0,
            tags: Vec::new(),
            content: serde_json::to_string(fields).unwrap(),
            created_at: ctx.created_at,
        })
    }
}

fn hex32(hex: &str) -> Option<[u8; 32]> {
    if hex.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        let hi = hex_value(hex.as_bytes()[i * 2])?;
        let lo = hex_value(hex.as_bytes()[i * 2 + 1])?;
        *byte = (hi << 4) | lo;
    }
    Some(out)
}

fn hex_value(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[test]
fn reply_needs_sign_with_nip10_tags_from_stored_parent() {
    let parent_id = "a1".repeat(32);
    let parent_author = "b2".repeat(32);
    let root_id = "c3".repeat(32);
    let mut r = KernelReducer::new();
    install_fixture_draft_builders(&r);
    let _ = r.set_active_account(PK.to_string());
    r.kernel
        .event_store_handle()
        .insert(
            crate::store::VerifiedEvent::from_raw_unchecked(RawEvent {
                id: parent_id.clone(),
                pubkey: parent_author.clone(),
                created_at: 1_700_000_000,
                kind: 1,
                tags: vec![vec![
                    "e".to_string(),
                    root_id.clone(),
                    "".to_string(),
                    "root".to_string(),
                ]],
                content: "parent".to_string(),
                sig: "0".repeat(128),
            }),
            &RELAY.to_string(),
            0,
        )
        .expect("seed parent");

    let cid = Some("reply-cid".to_string());
    let outcome = r.apply_actor_command(ActorCommand::Publish(PublishCommand::Reply {
        content: "reply body".to_string(),
        reply_to_event_id: parent_id.clone(),
        target: PublishTarget::Auto,
        signer_pubkey: None,
        correlation_id: cid.clone(),
    }));

    match outcome {
        CommandApplyOutcome::NeedsSign {
            request,
            target: PublishTarget::Auto,
            action_correlation_id,
        } => {
            assert_eq!(request.account_pubkey, PK);
            assert_eq!(action_correlation_id, cid);
            let unsigned: serde_json::Value =
                serde_json::from_str(&request.unsigned_json).expect("unsigned json");
            assert_eq!(unsigned["kind"], 1);
            assert_eq!(unsigned["content"], "reply body");
            let tags = unsigned["tags"].as_array().expect("tags array");
            assert!(
                tags.iter().any(|tag| tag
                    .as_array()
                    .is_some_and(|row| row.get(0) == Some(&serde_json::json!("e"))
                        && row.get(1) == Some(&serde_json::json!(root_id))
                        && row.get(3) == Some(&serde_json::json!("root")))),
                "reply must carry root e-tag: {}",
                request.unsigned_json
            );
            assert!(
                tags.iter().any(|tag| tag
                    .as_array()
                    .is_some_and(|row| row.get(0) == Some(&serde_json::json!("e"))
                        && row.get(1) == Some(&serde_json::json!(parent_id))
                        && row.get(3) == Some(&serde_json::json!("reply")))),
                "reply must carry direct reply e-tag: {}",
                request.unsigned_json
            );
            assert!(
                tags.iter().any(|tag| tag
                    .as_array()
                    .is_some_and(|row| row.get(0) == Some(&serde_json::json!("p"))
                        && row.get(1) == Some(&serde_json::json!(parent_author)))),
                "reply must p-tag the parent author: {}",
                request.unsigned_json
            );
        }
        other => panic!("expected NeedsSign(Auto), got {other:?}"),
    }
}

#[test]
fn reply_missing_parent_returns_unsupported() {
    let mut r = KernelReducer::new();
    install_fixture_draft_builders(&r);
    let _ = r.set_active_account(PK.to_string());

    let outcome = r.apply_actor_command(ActorCommand::Publish(PublishCommand::Reply {
        content: "reply body".to_string(),
        reply_to_event_id: "a1".repeat(32),
        target: PublishTarget::Auto,
        signer_pubkey: None,
        correlation_id: Some("reply-missing".to_string()),
    }));

    assert!(
        matches!(outcome, CommandApplyOutcome::Unsupported { ref reason } if reason.contains("reply_target_unknown")),
        "missing parent must fail closed; got {outcome:?}"
    );
}

#[test]
fn profile_needs_sign_with_correlation_id() {
    let mut r = KernelReducer::new();
    install_fixture_draft_builders(&r);
    let _ = r.set_active_account(PK.to_string());

    let cid = Some("profile-cid-3".to_string());
    let mut fields = serde_json::Map::new();
    fields.insert(
        "name".to_string(),
        serde_json::Value::String("Alice".to_string()),
    );

    let outcome = r.apply_actor_command(ActorCommand::Publish(PublishCommand::Profile {
        fields,
        correlation_id: cid.clone(),
    }));

    match outcome {
        CommandApplyOutcome::NeedsSign {
            request,
            target: PublishTarget::Auto,
            action_correlation_id,
        } => {
            assert_eq!(request.account_pubkey, PK);
            assert_eq!(action_correlation_id, cid);
            assert!(
                request.unsigned_json.contains("\"kind\":0"),
                "Profile must build a kind:0 unsigned event; got: {}",
                request.unsigned_json
            );
        }
        other => panic!("expected NeedsSign(Auto) for Profile, got {other:?}"),
    }
}
