//! Rust-owned action dispatch specs for Chirp shells.
//!
//! Native shells pass user intent into this module and receive the exact
//! `(namespace, body_json)` pair to feed through `nmp_app_dispatch_action`.
//! Protocol envelopes stay here; Swift/Kotlin only carry raw user input.

use nmp_core::tags::{e_tag, p_tag, EventRef, Nip10Refs};
use nmp_nip01::{Note, NoteRecord};
use nmp_nip02::{PubkeyAction, ReactAction};
use nmp_nip17::SendDmInput;
use nmp_nip57::ZapInput;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[cfg(test)]
#[path = "action_specs_tests.rs"]
mod tests;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TypedActionSpec {
    pub namespace: String,
    pub body_json: String,
}

impl TypedActionSpec {
    #[must_use]
    pub fn new(namespace: impl Into<String>, body_json: impl Into<String>) -> Self {
        Self {
            namespace: namespace.into(),
            body_json: body_json.into(),
        }
    }

    #[must_use]
    pub fn into_tuple(self) -> (String, String) {
        (self.namespace, self.body_json)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChirpActionIntent {
    PublishNote {
        content: String,
        #[serde(default)]
        reply_to: Option<ReplyTargetInput>,
        #[serde(default)]
        reply_to_event_id: Option<String>,
    },
    PublishProfile {
        name: String,
        #[serde(default)]
        about: Option<String>,
        #[serde(default)]
        picture: Option<String>,
    },
    Repost {
        event_id: String,
        author_pubkey: String,
    },
    React {
        event_id: String,
        reaction: String,
    },
    Follow {
        pubkey: String,
    },
    Unfollow {
        pubkey: String,
    },
    Zap {
        target_event_id: String,
        recipient_pubkey: String,
        amount_msats: u64,
        #[serde(default)]
        lnurl: Option<String>,
        #[serde(default)]
        comment: Option<String>,
    },
    SendDm {
        recipient_pubkey: String,
        content: String,
        #[serde(default)]
        reply_to: Option<String>,
    },
    /// Block a relay: add `url` to the active account's kind:10006 blocked-relay
    /// list. `account_pubkey` is the active signer's hex pubkey — the module uses
    /// it to read the current blocked set for idempotency. Mapped to the
    /// `nmp.nip51.block_relay` router-owned ActionModule.
    BlockRelay {
        url: String,
        account_pubkey: String,
    },
    /// Unblock a relay: remove `url` from the active account's kind:10006 list.
    /// Symmetric to [`BlockRelay`]. Mapped to `nmp.nip51.unblock_relay`.
    UnblockRelay {
        url: String,
        account_pubkey: String,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct ReplyTargetInput {
    pub event_id: String,
    pub author_pubkey: String,
    #[serde(default)]
    pub created_at: u64,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub root_event_id: Option<String>,
    #[serde(default)]
    pub root_relay: Option<String>,
    #[serde(default)]
    pub mentioned_pubkeys: Vec<String>,
}

pub fn action_spec_for_intent_json(intent_json: &str) -> Result<TypedActionSpec, String> {
    let intent: ChirpActionIntent = serde_json::from_str(intent_json)
        .map_err(|e| format!("invalid Chirp action intent JSON: {e}"))?;
    action_spec_for_intent(intent)
}

#[must_use]
pub fn action_spec_json_for_intent(intent_json: &str) -> String {
    match action_spec_for_intent_json(intent_json) {
        Ok(spec) => serialize_or_error(&spec),
        Err(error) => json!({ "error": error }).to_string(),
    }
}

pub fn action_spec_for_intent(intent: ChirpActionIntent) -> Result<TypedActionSpec, String> {
    match intent {
        ChirpActionIntent::PublishNote {
            content,
            reply_to,
            reply_to_event_id,
        } => match reply_to {
            Some(parent) => {
                let parent = parent.into_note_record();
                publish_note_spec(&content, Some(&parent))
            }
            None => publish_note_minimal_reply_spec(&content, reply_to_event_id.as_deref()),
        },
        ChirpActionIntent::PublishProfile {
            name,
            about,
            picture,
        } => Ok(publish_profile_spec(
            &name,
            about.as_deref().unwrap_or(""),
            picture.as_deref().unwrap_or(""),
        )),
        ChirpActionIntent::Repost {
            event_id,
            author_pubkey,
        } => Ok(repost_spec(&event_id, &author_pubkey)),
        ChirpActionIntent::React { event_id, reaction } => Ok(react_spec(&event_id, &reaction)),
        ChirpActionIntent::Follow { pubkey } => Ok(follow_spec(&pubkey)),
        ChirpActionIntent::Unfollow { pubkey } => Ok(unfollow_spec(&pubkey)),
        ChirpActionIntent::Zap {
            target_event_id,
            recipient_pubkey,
            amount_msats,
            lnurl,
            comment,
        } => Ok(zap_spec(
            &recipient_pubkey,
            amount_msats,
            Some(&target_event_id),
            comment.as_deref(),
            lnurl.as_deref(),
            Vec::new(),
        )),
        ChirpActionIntent::SendDm {
            recipient_pubkey,
            content,
            reply_to,
        } => Ok(send_dm_spec(
            &recipient_pubkey,
            &content,
            reply_to.as_deref(),
        )),
        ChirpActionIntent::BlockRelay {
            url,
            account_pubkey,
        } => Ok(block_relay_spec(&url, &account_pubkey)),
        ChirpActionIntent::UnblockRelay {
            url,
            account_pubkey,
        } => Ok(unblock_relay_spec(&url, &account_pubkey)),
    }
}

pub fn publish_note_spec(
    content: &str,
    reply_to: Option<&NoteRecord>,
) -> Result<TypedActionSpec, String> {
    let mut builder = Note::new(content);
    if let Some(parent) = reply_to {
        builder = builder.reply_to(parent);
    }
    let unsigned = builder.build("", 0).map_err(|e| e.to_string())?;
    Ok(publish_raw_spec(1, unsigned.tags, content))
}

pub fn publish_note_minimal_reply_spec(
    content: &str,
    reply_to_event_id: Option<&str>,
) -> Result<TypedActionSpec, String> {
    Note::new(content).build("", 0).map_err(|e| e.to_string())?;
    let tags = reply_to_event_id
        .filter(|id| !id.trim().is_empty())
        .map(|id| {
            vec![
                e_tag(id, None, Some("root")),
                e_tag(id, None, Some("reply")),
            ]
        })
        .unwrap_or_default();
    Ok(publish_raw_spec(1, tags, content))
}

#[must_use]
pub fn publish_profile_spec(name: &str, about: &str, picture: &str) -> TypedActionSpec {
    let mut fields = serde_json::Map::new();
    insert_non_empty(&mut fields, "name", name);
    insert_non_empty(&mut fields, "about", about);
    insert_non_empty(&mut fields, "picture", picture);
    TypedActionSpec::new(
        "nmp.publish",
        json!({ "PublishProfile": { "fields": Value::Object(fields) } }).to_string(),
    )
}

#[must_use]
pub fn repost_spec(event_id: &str, author_pubkey: &str) -> TypedActionSpec {
    publish_raw_spec(
        6,
        vec![e_tag(event_id, None, None), p_tag(author_pubkey, None)],
        "",
    )
}

#[must_use]
pub fn react_spec(event_id: &str, reaction: &str) -> TypedActionSpec {
    let input = ReactAction {
        target_event_id: event_id.to_string(),
        reaction: reaction.to_string(),
        target_author_pubkey: None,
    };
    typed_spec("nmp.nip25.react", &input)
}

#[must_use]
pub fn follow_spec(pubkey: &str) -> TypedActionSpec {
    typed_spec(
        "nmp.follow",
        &PubkeyAction {
            pubkey: pubkey.into(),
        },
    )
}

#[must_use]
pub fn unfollow_spec(pubkey: &str) -> TypedActionSpec {
    typed_spec(
        "nmp.unfollow",
        &PubkeyAction {
            pubkey: pubkey.into(),
        },
    )
}

#[must_use]
pub fn send_dm_spec(
    recipient_pubkey: &str,
    content: &str,
    reply_to: Option<&str>,
) -> TypedActionSpec {
    typed_spec(
        "nmp.nip17.send",
        &SendDmInput {
            recipient_pubkey: recipient_pubkey.to_string(),
            content: content.to_string(),
            reply_to: reply_to.map(str::to_string),
        },
    )
}

#[must_use]
pub fn zap_spec(
    recipient_pubkey: &str,
    amount_msats: u64,
    target_event_id: Option<&str>,
    comment: Option<&str>,
    lnurl: Option<&str>,
    relays: Vec<String>,
) -> TypedActionSpec {
    typed_spec(
        "nmp.nip57.zap",
        &ZapInput {
            recipient_pubkey: recipient_pubkey.to_string(),
            amount_msats,
            lnurl: non_empty(lnurl),
            relays,
            target_event_id: non_empty(target_event_id),
            comment: non_empty(comment),
        },
    )
}

/// Build the `nmp.nip51.block_relay` action body.
///
/// Wire shape: `{"url":"…","account_pubkey":"…"}`, matching the
/// `nmp_router::block_relay::BlockRelayInput` serde shape. The router-owned
/// ActionModule validates the URL scheme and applies the edit idempotently
/// against the active account's kind:10006 blocked-relay list.
#[must_use]
pub fn block_relay_spec(url: &str, account_pubkey: &str) -> TypedActionSpec {
    TypedActionSpec::new(
        "nmp.nip51.block_relay",
        json!({ "url": url, "account_pubkey": account_pubkey }).to_string(),
    )
}

/// Build the `nmp.nip51.unblock_relay` action body.
///
/// Symmetric to [`block_relay_spec`]: removes `url` from the active account's
/// kind:10006 blocked-relay list. Rejects with `ActionRejection::Conflict` when
/// the relay is not currently blocked (no publish, no spinner).
#[must_use]
pub fn unblock_relay_spec(url: &str, account_pubkey: &str) -> TypedActionSpec {
    TypedActionSpec::new(
        "nmp.nip51.unblock_relay",
        json!({ "url": url, "account_pubkey": account_pubkey }).to_string(),
    )
}

/// Build the `nmp.nip65.publish_relay_list` action body from the host's
/// configured relay set.
///
/// `relays` is a list of `(url, role)` pairs — the same shape the relay-config
/// projection exposes to shells. The on-wire JSON is `{"relays":[{"url","role"}…]}`;
/// `role` is the accepted alias for the router's `RelayListEntry::marker`, and
/// URL canonicalisation / `wss://` gating happens kernel-side in the action
/// module, not in the shell.
#[must_use]
pub fn publish_relay_list_spec(relays: &[(&str, &str)]) -> TypedActionSpec {
    let entries: Vec<Value> = relays
        .iter()
        .map(|(url, role)| json!({ "url": url, "role": role }))
        .collect();
    TypedActionSpec::new(
        "nmp.nip65.publish_relay_list",
        json!({ "relays": entries }).to_string(),
    )
}

impl ReplyTargetInput {
    fn into_note_record(self) -> NoteRecord {
        let refs = Nip10Refs {
            root: self.root_event_id.map(|id| EventRef {
                id,
                relay: self.root_relay,
                marker: Some("root".to_string()),
            }),
            reply: None,
            mentions: Vec::new(),
            mentioned_pubkeys: self.mentioned_pubkeys,
        };
        NoteRecord {
            event_id: self.event_id,
            author: self.author_pubkey,
            created_at: self.created_at,
            content: self.content,
            refs,
        }
    }
}

fn publish_raw_spec(kind: u32, tags: Vec<Vec<String>>, content: &str) -> TypedActionSpec {
    TypedActionSpec::new(
        "nmp.publish",
        json!({
            "PublishRaw": {
                "kind": kind,
                "tags": tags,
                "content": content,
                "target": "Auto"
            }
        })
        .to_string(),
    )
}

fn typed_spec(namespace: &str, input: &impl Serialize) -> TypedActionSpec {
    TypedActionSpec::new(namespace, serialize_action_body(input))
}

fn serialize_action_body(input: &impl Serialize) -> String {
    let mut value = serde_json::to_value(input).unwrap_or(Value::Null);
    drop_null_object_fields(&mut value);
    value.to_string()
}

fn serialize_or_error(spec: &TypedActionSpec) -> String {
    serde_json::to_string(spec).unwrap_or_else(|e| {
        json!({ "error": format!("failed to encode action spec: {e}") }).to_string()
    })
}

fn drop_null_object_fields(value: &mut Value) {
    if let Value::Object(map) = value {
        map.retain(|_, v| !v.is_null());
    }
}

fn non_empty(value: Option<&str>) -> Option<String> {
    value.filter(|v| !v.trim().is_empty()).map(str::to_string)
}

fn insert_non_empty(map: &mut serde_json::Map<String, Value>, key: &str, value: &str) {
    if !value.is_empty() {
        map.insert(key.to_string(), Value::String(value.to_string()));
    }
}
