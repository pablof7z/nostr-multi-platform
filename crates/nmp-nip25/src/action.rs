use nmp_core::actor::ActorCommand;
use nmp_core::actor::PublishCommand;
use nmp_core::slots::{ReactionDraft, ReactionDraftBuilder};
use nmp_core::substrate::{
    ActionContext, ActionModule, ActionPayload, ActionPayloadDecodeError, ActionRegistrar,
    ActionRejection, ProtocolCommand, ProtocolCommandContext, ProtocolCommandError,
};
use nmp_nip09::DeletionRequest;
use nmp_signer_iface::UnsignedEvent;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub const KIND_REACTION: u32 = 7;
/// Kind integer for NIP-09 deletion events (kind:5). Kept here because the
/// NIP-25 aggregate and projection projections filter on this kind. The
/// canonical builder and ownership for kind:5 artifacts live in `nmp-nip09`.
pub const KIND_REACTION_DELETE: u32 = 5;

type ReactionEventDraft = nmp_ownership::OwnedEventDraft<UnsignedEvent>;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReactAction {
    pub target_event_id: String,
    #[serde(default = "default_reaction")]
    pub reaction: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_author_pubkey: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UnreactAction {
    pub reaction_event_id: String,
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug)]
pub struct PublishReactionCommand {
    action: ReactAction,
    correlation_id: String,
}

#[derive(Debug)]
pub struct UnreactReactionCommand {
    action: UnreactAction,
    correlation_id: String,
}

pub struct ReactModule;
pub struct UnreactModule;
#[derive(Debug, Default)]
pub struct Nip25ReactionDraftBuilder;

impl ReactionDraftBuilder for Nip25ReactionDraftBuilder {
    fn build_reaction_draft(
        &self,
        target_event_id: &str,
        target_author_pubkey: Option<&str>,
        reaction: &str,
    ) -> Option<ReactionDraft> {
        build_reaction_draft(target_event_id, target_author_pubkey, reaction).ok()
    }
}

impl ActionModule for ReactModule {
    const NAMESPACE: nmp_core::substrate::DeclaredActionNamespace =
        nmp_core::substrate::DeclaredActionNamespace::framework(
            "nmp.nip25.react",
            "action.nmp.nip25.react",
        );
    type Action = ReactAction;

    /// ADR-0071 / S3: opt into the typed FlatBuffers payload doorway; the
    /// fail-closed `schema_version` gate runs in `decode` (BEFORE `start`).
    fn decode_payload(bytes: &[u8]) -> Option<Result<Self::Action, ActionPayloadDecodeError>> {
        Some(<ReactAction as ActionPayload>::decode(bytes))
    }

    fn start(&self, _ctx: &mut ActionContext, action: Self::Action) -> Result<(), ActionRejection> {
        validate_react(&action)
    }

    fn execute(
        &self,
        ctx: &ActionContext,
        mut action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        action.target_author_pubkey = resolve_target_author_pubkey(ctx, &action);
        send(ActorCommand::Protocol(Box::new(PublishReactionCommand {
            action,
            correlation_id: correlation_id.to_string(),
        })));
        Ok(())
    }
}

impl ActionModule for UnreactModule {
    const NAMESPACE: nmp_core::substrate::DeclaredActionNamespace =
        nmp_core::substrate::DeclaredActionNamespace::framework(
            "nmp.nip25.unreact",
            "action.nmp.nip25.unreact",
        );
    type Action = UnreactAction;

    /// ADR-0071 / S3: opt into the typed FlatBuffers payload doorway; the
    /// fail-closed `schema_version` gate runs in `decode` (BEFORE `start`).
    fn decode_payload(bytes: &[u8]) -> Option<Result<Self::Action, ActionPayloadDecodeError>> {
        Some(<UnreactAction as ActionPayload>::decode(bytes))
    }

    fn start(&self, _ctx: &mut ActionContext, action: Self::Action) -> Result<(), ActionRejection> {
        if !is_hex64(&action.reaction_event_id) {
            return Err(ActionRejection::Invalid(
                "unreact requires a 64-hex reaction_event_id".to_string(),
            ));
        }
        Ok(())
    }

    fn execute(
        &self,
        _ctx: &ActionContext,
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        send(ActorCommand::Protocol(Box::new(UnreactReactionCommand {
            action,
            correlation_id: correlation_id.to_string(),
        })));
        Ok(())
    }
}

impl ProtocolCommand for PublishReactionCommand {
    fn run(
        self: Box<Self>,
        ctx: &mut ProtocolCommandContext<'_>,
    ) -> Result<(), ProtocolCommandError> {
        let event = build_reaction_event(&self.action)
            .map_err(|err| ProtocolCommandError::new(format!("react: {err}")))?;
        let draft = ReactionEventDraft::new(event, crate::ownership::REACTION_EVENT_PROVENANCE);
        ctx.send(ActorCommand::Publish(PublishCommand::owned_draft(
            draft,
            Some(self.correlation_id),
            None,
        )));
        Ok(())
    }
}

impl ProtocolCommand for UnreactReactionCommand {
    fn run(
        self: Box<Self>,
        ctx: &mut ProtocolCommandContext<'_>,
    ) -> Result<(), ProtocolCommandError> {
        let draft = reaction_delete_draft(self.action.reaction_event_id, self.action.reason);
        ctx.send(ActorCommand::Publish(PublishCommand::owned_draft(
            draft,
            Some(self.correlation_id),
            None,
        )));
        Ok(())
    }
}

pub(crate) fn register_actions(app: &mut impl ActionRegistrar) {
    app.register_default_action(ReactModule);
    app.register_default_action(UnreactModule);
}

/// Install the NIP-25 reaction draft builder into a reducer-hosted composition
/// root. This keeps the reducer/browser write path on the same protocol-owned
/// grammar as the registered action command.
pub fn install_reaction_draft_builder(reducer: &mut nmp_core::KernelReducer) {
    reducer.set_reaction_draft_builder(Arc::new(Nip25ReactionDraftBuilder));
}

fn validate_react(action: &ReactAction) -> Result<(), ActionRejection> {
    if !is_hex64(&action.target_event_id) {
        return Err(ActionRejection::Invalid(
            "react requires a 64-hex target_event_id".to_string(),
        ));
    }
    if action
        .target_author_pubkey
        .as_deref()
        .is_some_and(|author| !is_hex64(author))
    {
        return Err(ActionRejection::Invalid(
            "react target_author_pubkey must be 64-hex when provided".to_string(),
        ));
    }
    Ok(())
}

/// Build the bare NIP-25 reaction (`kind:7`) event from its inputs — no
/// routing, no transport envelope. The returned [`UnsignedEvent`] carries the
/// `e`/`p` reaction tags and content; `pubkey` / `created_at` / `sig` are
/// filled at sign time.
///
/// This is the **composition seam** for routing a reaction *into* another
/// transport. To react to an event inside a NIP-29 group, build the reaction
/// here, then hand its `(kind, content, tags)` to NIP-29's generic
/// `nmp.nip29.publish_group_event` surface (`PublishGroupEventInput`), which
/// injects only the `h` / `previous` envelope. NIP-25 owns the `kind:7`
/// construction; the transport owns only its envelope — NIP-29 never names,
/// classifies, or owns `kind:7` (the #2504/#2505 kind-blind correction, #2513).
///
/// # Errors
///
/// Returns the validation message when `target_event_id` is not 64-hex or
/// `target_author_pubkey` is supplied but malformed.
pub fn build_reaction_event(action: &ReactAction) -> Result<UnsignedEvent, String> {
    match validate_react(action) {
        Ok(()) => {}
        Err(ActionRejection::Invalid(msg)) => return Err(msg),
        Err(other) => return Err(format!("{other:?}")),
    }
    let draft = build_reaction_draft(
        &action.target_event_id,
        action.target_author_pubkey.as_deref(),
        &action.reaction,
    )?;
    Ok(UnsignedEvent {
        pubkey: String::new(),
        kind: KIND_REACTION,
        tags: draft.tags,
        content: draft.content,
        created_at: 0,
    })
}

/// Build NIP-25 kind:7 reaction tags and normalized content.
///
/// The target author's `p` tag is included when supplied by the caller's read
/// cache. Missing author degrades to an `e`-only reaction draft.
pub fn build_reaction_draft(
    target_event_id: &str,
    target_author_pubkey: Option<&str>,
    reaction: &str,
) -> Result<ReactionDraft, String> {
    if !is_hex64(target_event_id) {
        return Err("react requires a 64-hex target_event_id".to_string());
    }
    if target_author_pubkey.is_some_and(|author| !is_hex64(author)) {
        return Err("react target_author_pubkey must be 64-hex when provided".to_string());
    }
    let content = if reaction.trim().is_empty() {
        "+".to_string()
    } else {
        reaction.to_string()
    };
    let mut tags = vec![vec!["e".to_string(), target_event_id.to_string()]];
    if let Some(author) = target_author_pubkey {
        tags.push(vec!["p".to_string(), author.to_string()]);
    }
    Ok(ReactionDraft { tags, content })
}

/// Build a kind:5 deletion draft for a reaction event, delegating construction
/// to `nmp-nip09` so the artifact provenance is owned by the deletion crate
/// (ADR-0074 composable-ownership doctrine). The reaction event id has already
/// been validated as 64-hex by `UnreactModule::start`, so the call always
/// succeeds; we treat errors as internal bugs and propagate via unwrap.
fn reaction_delete_draft(reaction_event_id: String, reason: String) -> ReactionEventDraft {
    nmp_nip09::build_deletion_draft(&DeletionRequest {
        event_ids: vec![reaction_event_id],
        kinds: vec![],
        reason,
    })
    .expect("reaction_event_id is pre-validated 64-hex by UnreactModule::start")
}

fn resolve_target_author_pubkey(ctx: &ActionContext, action: &ReactAction) -> Option<String> {
    if action.target_author_pubkey.is_some() {
        return action.target_author_pubkey.clone();
    }
    ctx.local_event_by_id(&action.target_event_id)
        .ok()
        .flatten()
        .map(|stored| stored.raw.pubkey.clone())
        .filter(|pubkey| is_hex64(pubkey))
}

fn default_reaction() -> String {
    "+".to_string()
}

fn is_hex64(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

#[cfg(test)]
#[path = "action_tests.rs"]
mod tests;
