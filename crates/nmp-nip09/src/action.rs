//! `nmp.nip09.delete` — generic NIP-09 deletion action module.
//!
//! Apps that want to delete their own events without hand-building a kind:5
//! wire event dispatch this action. The module validates the request, builds
//! the deletion draft through the owned [`crate::build_deletion_draft`] seam,
//! and sends it through the one-door publish path with `nmp-nip09` provenance.
//!
//! This is a **yielding default**: an app that pre-registers its own deletion
//! handler pre-empts this regardless of call order.

use nmp_core::actor::ActorCommand;
use nmp_core::actor::PublishCommand;
use nmp_core::substrate::{
    ActionContext, ActionModule, ActionRegistrar, ActionRejection, ProtocolCommand,
    ProtocolCommandContext, ProtocolCommandError,
};
use serde::{Deserialize, Serialize};

use crate::builder::{build_deletion_draft, is_hex64, DeletionRequest};

/// A user intent to delete one or more of their own events via NIP-09.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DeleteAction {
    /// Hex-64 event ids to request deletion for.
    pub event_ids: Vec<String>,
    /// Optional kind integers to emit as `k` tags (may be empty).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kinds: Vec<u32>,
    /// Human-readable deletion reason (event content; may be empty).
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug)]
pub struct DeleteCommand {
    request: DeletionRequest,
    correlation_id: String,
}

/// `nmp.nip09.delete` action module: generic NIP-09 event deletion.
pub struct DeleteModule;

impl ActionModule for DeleteModule {
    const NAMESPACE: nmp_core::substrate::DeclaredActionNamespace =
        nmp_core::substrate::DeclaredActionNamespace::framework(
            "nmp.nip09.delete",
            "action.nmp.nip09.delete",
        );
    type Action = DeleteAction;

    fn start(&self, _ctx: &mut ActionContext, action: Self::Action) -> Result<(), ActionRejection> {
        if action.event_ids.is_empty() {
            return Err(ActionRejection::Invalid(
                "delete requires at least one event_id".to_string(),
            ));
        }
        for id in &action.event_ids {
            if !is_hex64(id) {
                return Err(ActionRejection::Invalid(format!(
                    "delete event_ids must be 64-hex, got {:?}",
                    id
                )));
            }
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
        send(ActorCommand::Protocol(Box::new(DeleteCommand {
            request: DeletionRequest {
                event_ids: action.event_ids,
                kinds: action.kinds,
                reason: action.reason,
            },
            correlation_id: correlation_id.to_string(),
        })));
        Ok(())
    }
}

impl ProtocolCommand for DeleteCommand {
    fn run(
        self: Box<Self>,
        ctx: &mut ProtocolCommandContext<'_>,
    ) -> Result<(), ProtocolCommandError> {
        let draft = build_deletion_draft(&self.request)
            .map_err(|err| ProtocolCommandError::new(format!("nip09.delete: {err}")))?;
        ctx.send(ActorCommand::Publish(PublishCommand::owned_draft(
            draft,
            Some(self.correlation_id),
            None,
        )));
        Ok(())
    }
}

pub(crate) fn register_actions(app: &mut impl ActionRegistrar) {
    app.register_default_action(DeleteModule);
}
