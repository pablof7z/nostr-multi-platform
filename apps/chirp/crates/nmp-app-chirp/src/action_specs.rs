//! Rust-owned `(namespace, body_json)` action builders for Chirp's in-repo Rust
//! shells.
//!
//! These pure builder functions construct the exact `(namespace, body_json)`
//! pair for each Chirp write verb; protocol tag/body construction stays here, in
//! Rust. They back the Rust-native [`crate::typed_api::ChirpClient`] used by the
//! chirp-tui / chirp-desktop shells (which dispatch the pair through the typed
//! byte doorway via [`crate::dispatch_bytes`]).
//!
//! The native iOS/Android shells no longer go through these: their social writes
//! ride the generated `GeneratedActionBuilders` FlatBuffers byte builders
//! (ADR-0064 §3, M14-1 / #2145) straight to the byte doorway. The former
//! `ChirpActionIntent` JSON intent lane has been retired.

use nmp_core::tags::{e_tag, p_tag};
use nmp_nip01::{Note, NoteRecord};
use nmp_nip02::{PubkeyAction, ReactAction};
use nmp_nip17::SendDmInput;
use nmp_nip57::ZapInput;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{ZapIdentifierInput, ZAP_IDENTIFIER_NAMESPACE};

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

#[must_use]
pub fn zap_identifier_spec(
    recipient_identifier: &str,
    amount_msats: u64,
    target_event_id: Option<&str>,
    comment: Option<&str>,
) -> TypedActionSpec {
    typed_spec(
        ZAP_IDENTIFIER_NAMESPACE,
        &ZapIdentifierInput {
            recipient_identifier: recipient_identifier.to_string(),
            amount_msats,
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
