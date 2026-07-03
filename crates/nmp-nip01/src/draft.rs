//! NIP-01 draft-builder registration.

use std::sync::Arc;

use nmp_core::substrate::{
    DraftBuildContext, DraftBuildError, DraftBuilder, DraftBuilderRegistrar, DraftIntent,
    DraftIntentKind,
};
use nmp_signer_iface::UnsignedEvent;

use crate::decode::NoteRecord;
use crate::kinds::KIND_SHORT_TEXT_NOTE;
use crate::nip10::{parse_nip10, reply_tags};

const KIND_PROFILE_METADATA: u32 = 0;

/// Register NIP-01 draft builders for kind:1 replies and kind:0 profiles.
pub fn register_draft_builders(app: &impl DraftBuilderRegistrar) {
    app.register_draft_builder(DraftIntentKind::Reply, Arc::new(ReplyDraftBuilder));
    app.register_draft_builder(DraftIntentKind::Profile, Arc::new(ProfileDraftBuilder));
}

struct ReplyDraftBuilder;

impl DraftBuilder for ReplyDraftBuilder {
    fn build(
        &self,
        intent: &DraftIntent,
        ctx: DraftBuildContext<'_>,
    ) -> Result<UnsignedEvent, DraftBuildError> {
        let DraftIntent::Reply {
            content,
            reply_to_event_id,
        } = intent
        else {
            return Err(DraftBuildError::new(
                "nip01 reply builder received wrong intent",
            ));
        };
        let id = hex32(reply_to_event_id)
            .ok_or_else(|| DraftBuildError::new("reply_target_invalid_hex"))?;
        let stored = ctx
            .event_store
            .get_by_id(&id)
            .map_err(|err| DraftBuildError::new(format!("reply_target_lookup: {err}")))?
            .ok_or_else(|| {
                DraftBuildError::new(format!("reply_target_unknown: {reply_to_event_id}"))
            })?;
        if stored.raw.kind != KIND_SHORT_TEXT_NOTE {
            return Err(DraftBuildError::new("reply_target_not_kind1"));
        }
        let parent = NoteRecord {
            event_id: stored.raw.id.clone(),
            author: stored.raw.pubkey.clone(),
            created_at: stored.raw.created_at,
            content: stored.raw.content.clone(),
            refs: parse_nip10(&stored.raw.tags),
        };
        Ok(UnsignedEvent {
            pubkey: ctx.author_pubkey.to_string(),
            kind: KIND_SHORT_TEXT_NOTE,
            tags: reply_tags(&parent.event_id, &parent.author, &parent.refs, None),
            content: content.clone(),
            created_at: ctx.created_at,
        })
    }
}

struct ProfileDraftBuilder;

impl DraftBuilder for ProfileDraftBuilder {
    fn build(
        &self,
        intent: &DraftIntent,
        ctx: DraftBuildContext<'_>,
    ) -> Result<UnsignedEvent, DraftBuildError> {
        let DraftIntent::Profile { fields } = intent else {
            return Err(DraftBuildError::new(
                "nip01 profile builder received wrong intent",
            ));
        };
        let mut merged = current_profile_fields(&ctx)?;
        for (key, value) in fields {
            merged.insert(key.clone(), value.clone());
        }
        let content = serde_json::to_string(&merged)
            .map_err(|err| DraftBuildError::new(format!("profile_serialisation: {err}")))?;
        Ok(UnsignedEvent {
            pubkey: ctx.author_pubkey.to_string(),
            kind: KIND_PROFILE_METADATA,
            tags: Vec::new(),
            content,
            created_at: ctx.created_at,
        })
    }
}

fn current_profile_fields(
    ctx: &DraftBuildContext<'_>,
) -> Result<serde_json::Map<String, serde_json::Value>, DraftBuildError> {
    let Some(author) = hex32(ctx.author_pubkey) else {
        return Ok(serde_json::Map::new());
    };
    let mut iter = ctx
        .event_store
        .scan_by_author_kind(&author, &[KIND_PROFILE_METADATA], None, None, 1)
        .map_err(|err| DraftBuildError::new(format!("profile_lookup: {err}")))?;
    let Some(stored) = iter
        .next()
        .transpose()
        .map_err(|err| DraftBuildError::new(format!("profile_lookup: {err}")))?
    else {
        return Ok(serde_json::Map::new());
    };
    serde_json::from_str::<serde_json::Value>(&stored.raw.content)
        .ok()
        .and_then(|value| value.as_object().cloned())
        .ok_or_else(|| DraftBuildError::new("profile_current_content_not_object"))
        .or_else(|_| Ok(serde_json::Map::new()))
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

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_core::substrate::{DraftBuilderRegistry, DraftIntent};
    use nmp_store::{EventStore, MemEventStore, RawEvent, VerifiedEvent};

    const AUTHOR: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const PARENT_AUTHOR: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const ROOT_ID: &str = "1111111111111111111111111111111111111111111111111111111111111111";
    const PARENT_ID: &str = "2222222222222222222222222222222222222222222222222222222222222222";
    const RELAY: &str = "wss://relay.example";

    fn insert(store: &MemEventStore, raw: RawEvent) {
        store
            .insert(
                VerifiedEvent::from_raw_unchecked(raw),
                &RELAY.to_string(),
                0,
            )
            .expect("insert fixture");
    }

    struct Host {
        registry: DraftBuilderRegistry,
    }

    impl Host {
        fn new() -> Self {
            let host = Self {
                registry: DraftBuilderRegistry::new(),
            };
            register_draft_builders(&host);
            host
        }
    }

    impl DraftBuilderRegistrar for Host {
        fn register_draft_builder(&self, kind: DraftIntentKind, builder: Arc<dyn DraftBuilder>) {
            self.registry.register(kind, builder);
        }
    }

    #[test]
    fn registered_reply_builder_builds_kind1_with_nip10_tags() {
        let store = MemEventStore::new();
        insert(
            &store,
            RawEvent {
                id: PARENT_ID.to_string(),
                pubkey: PARENT_AUTHOR.to_string(),
                created_at: 10,
                kind: KIND_SHORT_TEXT_NOTE,
                tags: vec![vec![
                    "e".into(),
                    ROOT_ID.into(),
                    "wss://root.example".into(),
                    "root".into(),
                ]],
                content: "parent".into(),
                sig: "f".repeat(128),
            },
        );

        let host = Host::new();
        let unsigned = host
            .registry
            .build(
                &DraftIntent::Reply {
                    content: "reply".into(),
                    reply_to_event_id: PARENT_ID.into(),
                },
                DraftBuildContext {
                    event_store: &store,
                    author_pubkey: AUTHOR,
                    created_at: 42,
                },
            )
            .expect("reply draft");

        assert_eq!(unsigned.kind, KIND_SHORT_TEXT_NOTE);
        assert_eq!(unsigned.pubkey, AUTHOR);
        assert_eq!(unsigned.created_at, 42);
        assert_eq!(
            unsigned.tags[0],
            vec!["e", ROOT_ID, "wss://root.example", "root"]
        );
        assert_eq!(unsigned.tags[1], vec!["e", PARENT_ID, "", "reply"]);
        assert_eq!(unsigned.tags[2], vec!["p", PARENT_AUTHOR]);
    }

    #[test]
    fn registered_profile_builder_merges_current_kind0_fields() {
        let store = MemEventStore::new();
        insert(
            &store,
            RawEvent {
                id: ROOT_ID.to_string(),
                pubkey: AUTHOR.to_string(),
                created_at: 10,
                kind: KIND_PROFILE_METADATA,
                tags: Vec::new(),
                content: r#"{"name":"alice","about":"old"}"#.into(),
                sig: "f".repeat(128),
            },
        );

        let mut fields = serde_json::Map::new();
        fields.insert("about".into(), serde_json::Value::String("new".into()));
        fields.insert(
            "picture".into(),
            serde_json::Value::String("https://p".into()),
        );
        let host = Host::new();
        let unsigned = host
            .registry
            .build(
                &DraftIntent::Profile { fields },
                DraftBuildContext {
                    event_store: &store,
                    author_pubkey: AUTHOR,
                    created_at: 43,
                },
            )
            .expect("profile draft");

        let content: serde_json::Value = serde_json::from_str(&unsigned.content).unwrap();
        assert_eq!(unsigned.kind, KIND_PROFILE_METADATA);
        assert_eq!(content["name"], "alice");
        assert_eq!(content["about"], "new");
        assert_eq!(content["picture"], "https://p");
    }
}
