//! NIP-22 post-comment action (`nmp.nip22.post_comment`).
//!
//! Builds an unsigned kind:1111 event with the correct two-scope tag set:
//! UPPERCASE root scope (`A`/`E`/`I` + `K`) and lowercase parent scope
//! (`a`/`e`/`i` + `k`). Mirrors the `nmp-nip25` `react` action module shape.

use nmp_core::actor::ActorCommand;
use nmp_core::actor::PublishCommand;
use nmp_core::substrate::{
    ActionContext, ActionModule, ActionPayload, ActionPayloadDecodeError, ActionRegistrar,
    ActionRejection, ProtocolCommand, ProtocolCommandContext, ProtocolCommandError,
};
use nmp_kinds::KIND_NIP22_COMMENT;
use nmp_signer_iface::UnsignedEvent;
use serde::{Deserialize, Serialize};

pub use nmp_kinds::KIND_NIP22_COMMENT as KIND_COMMENT;

/// Input for `nmp.nip22.post_comment`.
///
/// `root_tag_name` selects the uppercase root scope: `A` for an addressable
/// artifact (`30023:<pubkey>:<d>`), `E` for an event id, `I` for external
/// content (`url:…`, `podcast:…`, `isbn:…`). Case is normalised.
///
/// `parent_event_id` is `None` for a top-level comment (its parent mirrors the
/// root) and `Some(comment_id)` for a reply to a specific kind:1111 comment.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PostCommentAction {
    pub root_tag_name: String,
    pub root_tag_value: String,
    /// Root kind for the uppercase `K` tag. `0` for purely external roots.
    pub root_kind: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_event_id: Option<String>,
    /// Root author pubkey, emitted as the uppercase `P` tag when known
    /// (NIP-22: notify the root author). Omitted when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_author_pubkey: Option<String>,
    /// Parent comment author pubkey, emitted as the lowercase `p` tag on a
    /// reply when known (NIP-22: notify the parent author). Omitted when
    /// absent or on a top-level comment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_author_pubkey: Option<String>,
    pub content: String,
}

#[derive(Debug)]
pub struct PostCommentCommand {
    action: PostCommentAction,
    correlation_id: String,
}

pub struct PostCommentModule;

impl ActionModule for PostCommentModule {
    const NAMESPACE: &'static str = "nmp.nip22.post_comment";
    type Action = PostCommentAction;

    /// ADR-0064 / S9: opt into the typed FlatBuffers payload doorway; the
    /// fail-closed `schema_version` gate runs in `decode` (BEFORE `start`).
    fn decode_payload(bytes: &[u8]) -> Option<Result<Self::Action, ActionPayloadDecodeError>> {
        Some(<PostCommentAction as ActionPayload>::decode(bytes))
    }

    fn start(&self, _ctx: &mut ActionContext, action: Self::Action) -> Result<(), ActionRejection> {
        validate(&action).map_err(ActionRejection::Invalid)
    }

    fn execute(
        &self,
        _ctx: &ActionContext,
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        send(ActorCommand::Protocol(Box::new(PostCommentCommand {
            action,
            correlation_id: correlation_id.to_string(),
        })));
        Ok(())
    }
}

impl ProtocolCommand for PostCommentCommand {
    fn run(
        self: Box<Self>,
        ctx: &mut ProtocolCommandContext<'_>,
    ) -> Result<(), ProtocolCommandError> {
        let (tags, content) = comment_event(&self.action)
            .map_err(|err| ProtocolCommandError::new(format!("post_comment: {err}")))?;
        ctx.send(ActorCommand::Publish(PublishCommand::UnsignedEvent {
            event: UnsignedEvent {
                pubkey: String::new(),
                kind: KIND_NIP22_COMMENT,
                tags,
                content,
                created_at: 0,
            },
            correlation_id: Some(self.correlation_id),
            signer_pubkey: None,
        }));
        Ok(())
    }
}

/// Register the default post-comment action.
pub fn register_actions(app: &mut impl ActionRegistrar) {
    app.register_default_action(PostCommentModule);
}

fn validate(action: &PostCommentAction) -> Result<(), String> {
    if action.content.trim().is_empty() {
        return Err("comment content must not be empty".to_string());
    }
    let root_value = action.root_tag_value.trim();
    if root_value.is_empty() {
        return Err("root_tag_value must not be empty".to_string());
    }
    let upper = root_scope(&action.root_tag_name)?;
    if upper == 'E' && !is_hex64(root_value) {
        return Err("root_tag_value must be a 64-hex event id for an E root".to_string());
    }
    if let Some(parent) = action.parent_event_id.as_deref().map(str::trim) {
        if !is_hex64(parent) {
            return Err("parent_event_id must be a 64-hex event id when provided".to_string());
        }
    }
    if action
        .root_author_pubkey
        .as_deref()
        .map(str::trim)
        .is_some_and(|pubkey| !is_hex64(pubkey))
    {
        return Err("root_author_pubkey must be 64-hex when provided".to_string());
    }
    if action
        .parent_author_pubkey
        .as_deref()
        .map(str::trim)
        .is_some_and(|pubkey| !is_hex64(pubkey))
    {
        return Err("parent_author_pubkey must be 64-hex when provided".to_string());
    }
    Ok(())
}

/// Build the `(tags, content)` for a kind:1111 comment from a validated action.
fn comment_event(action: &PostCommentAction) -> Result<(Vec<Vec<String>>, String), String> {
    validate(action)?;
    let upper = root_scope(&action.root_tag_name)?;
    let lower = upper.to_ascii_lowercase();
    let root_value = action.root_tag_value.trim().to_string();

    let mut tags: Vec<Vec<String>> = Vec::with_capacity(6);

    // Uppercase root scope + root kind, and the root author `P` tag when known.
    tags.push(vec![upper.to_string(), root_value.clone()]);
    tags.push(vec!["K".to_string(), action.root_kind.to_string()]);
    if let Some(pubkey) = author_pubkey(&action.root_author_pubkey) {
        tags.push(vec!["P".to_string(), pubkey]);
    }

    // Lowercase parent scope + parent kind. Replies reference the parent
    // comment as a kind:1111 event; top-level comments mirror the root.
    let (parent_name, parent_value, parent_kind) = match action.parent_event_id.as_deref() {
        Some(parent) => (
            'e'.to_string(),
            parent.trim().to_string(),
            KIND_NIP22_COMMENT,
        ),
        None => (lower.to_string(), root_value, action.root_kind),
    };
    tags.push(vec![parent_name, parent_value]);
    tags.push(vec!["k".to_string(), parent_kind.to_string()]);
    // Parent author `p` tag when known (a reply notifies the parent author).
    if let Some(pubkey) = author_pubkey(&action.parent_author_pubkey) {
        tags.push(vec!["p".to_string(), pubkey]);
    }

    Ok((tags, action.content.trim().to_string()))
}

fn root_scope(tag_name: &str) -> Result<char, String> {
    match tag_name
        .trim()
        .chars()
        .next()
        .map(|c| c.to_ascii_uppercase())
    {
        Some(upper @ ('A' | 'E' | 'I')) => Ok(upper),
        Some(other) => Err(format!("root_tag_name must be A/E/I, got {other}")),
        None => Err("root_tag_name must not be empty".to_string()),
    }
}

fn author_pubkey(value: &Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|pubkey| !pubkey.is_empty())
        .map(str::to_string)
}

fn is_hex64(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

#[cfg(test)]
#[path = "action_tests.rs"]
mod tests;
