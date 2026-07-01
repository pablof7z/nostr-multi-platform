//! `PublishAction` + `PublishModule` (the `ActionModule` impl).
//!
//! `start` is wired to the actor mailbox (M6): `ffi::action::execute_action`
//! validates a `PublishAction` through `ActionRegistry`, then converts
//! unsigned app-facing variants into actor publish commands. Pre-signed publish
//! is internal/protocol-only and is not dispatchable through this module.

use serde::{Deserialize, Serialize};

use crate::actor::ActorCommand;
use crate::actor::PublishCommand;
use crate::publish::policy::{classify_publish_behavior, validate_publish_routing};
use crate::substrate::{
    ActionContext, ActionModule, ActionPayload, ActionPayloadDecodeError, ActionRejection,
};
use nmp_signer_iface::SignedEvent;

mod signer;
mod target;

pub use signer::{PublishSigner, PublishSignerProvenance};
pub(crate) use target::{
    validate_explicit_relays, validate_presigned_publish_target, validate_publish_target,
};
pub use target::{PublishRouteClass, PublishTarget};

/// Stable handle used by the internal pre-signed publish engine path.
pub type PublishHandle = String;

/// Relay URL — grep-able alias so the `RelayDispatcher` shim can be swapped
/// for `nmp-nip01::RelayManager` from M8 without changing call sites. Single
/// crate-wide definition lives in `crate::relay`; re-exported here so
/// `publish` import paths are unchanged.
pub use crate::relay::RelayUrl;

/// The publish action shape.
///
/// App-facing dispatch accepts unsigned draft/build variants only. The
/// `Publish` variant remains for the internal engine/verbatim path but is
/// rejected by [`PublishModule`] and omitted from the `nmp.publish` wire schema.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum PublishAction {
    /// Internal/protocol-only externally signed publish. Not app-dispatchable.
    Publish {
        handle: PublishHandle,
        event: SignedEvent,
        target: PublishTarget,
    },
    /// Publish a kind:0 profile metadata event for the active account.
    /// `fields` is a flat JSON object with string-valued keys such as
    /// `"name"`, `"about"`, `"picture"` — the actor serializes it into the
    /// kind:0 `content` field, signs with the active signer, and routes
    /// through the NIP-65 outbox. Like `PublishRaw`, the event is
    /// *not* pre-signed: the actor stamps `created_at` and signs. This is the
    /// `ActionModule`-native path for hosts that need to publish kind:0
    /// metadata events; the one-door rule deleted the prior bespoke
    /// `nmp_app_publish_unsigned_event` FFI symbol, so this `PublishAction`
    /// variant + `nmp_app_dispatch_action("nmp.publish", ...)` is the only
    /// door for it.
    PublishProfile {
        fields: serde_json::Map<String, serde_json::Value>,
    },
    /// Sign-and-publish an arbitrary event kind for the active account.
    ///
    /// `kind`, `tags`, and `content` map directly to Nostr event fields.
    /// The actor fills `pubkey` from the active signer, stamps `created_at`
    /// (D7 — kernel owns the wall clock), signs, and routes through the
    /// NIP-65 outbox per `target`. This is the generic publish path for
    /// second apps and custom event kinds that don't warrant a dedicated
    /// `ActionModule`.
    ///
    /// # Why this exists
    ///
    /// `nmp_app_publish_unsigned_event` was deleted to enforce the
    /// `dispatch_action` seam. Without `PublishRaw`, every new event kind
    /// requires a Rust `ActionModule` impl — a 2-week barrier for app
    /// developers. `PublishRaw` restores the generic publish capability
    /// while keeping it routed through the action lifecycle (`correlation_id`,
    /// `action_stages`, NIP-65 outbox).
    ///
    /// # Restrictions
    ///
    /// Reserved replaceable lists such as kind:0 (profile), kind:3
    /// (contacts), and kind:10003 (NIP-51 bookmarks) have dedicated variants
    /// or per-NIP action modules that apply protocol-specific validation and
    /// read-modify-write merging. `PublishRaw` rejects these kinds to prevent
    /// accidental data loss from bypassing that processing.
    ///
    /// # Signer selection
    ///
    /// `signer` is typed app-facing intent: `Active` signs with the active
    /// account; `Registered { pubkey, provenance }` signs with the registered
    /// signer whose pubkey matches while preserving why this path is allowed
    /// (for example, an app-managed agent key). The active account is never
    /// changed. Whether the selected key is local (nsec, signs inline) or
    /// remote (NIP-46 bunker, parks on the kernel's `ParkedOp` path) is
    /// transparent to the caller. An unknown pubkey is **not** validated at
    /// dispatch time — it surfaces as a sign-time error toast through
    /// `sign_with_account_nonblocking`'s "no signer for account {pubkey}" path.
    PublishRaw {
        kind: u32,
        tags: Vec<Vec<String>>,
        content: String,
        #[serde(default)]
        target: PublishTarget,
        #[serde(default)]
        signer: PublishSigner,
    },
    /// Sign-and-publish a kind:1 reply. Hosts provide only the direct parent
    /// event id and content; the reducer resolves the parent from the kernel
    /// store and builds NIP-10 marked tags in Rust.
    PublishReply {
        content: String,
        reply_to_event_id: String,
        #[serde(default)]
        target: PublishTarget,
        #[serde(default)]
        signer: PublishSigner,
    },
}

/// Final outcome reported to the action ledger when the engine finishes.
///
/// `Mixed` covers the common case where some relays accepted and some
/// gave up — the snapshot carries the per-relay detail; the ledger gets a
/// single coarse verdict.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum PublishOutcome {
    Accepted {
        relays: Vec<RelayUrl>,
    },
    Mixed {
        accepted: Vec<RelayUrl>,
        failed: Vec<RelayUrl>,
    },
    FailedAfterRetries {
        failed: Vec<RelayUrl>,
    },
    NoTargets,
    Cancelled,
}

/// `ActionModule` impl. The runtime is the engine; this trait exists so the
/// ledger sees a uniform shape across actions.
pub struct PublishModule;

impl ActionModule for PublishModule {
    const NAMESPACE: crate::substrate::DeclaredActionNamespace =
        crate::substrate::DeclaredActionNamespace::framework("nmp.publish", "action.nmp.publish");

    type Action = PublishAction;

    /// Publish actions settle asynchronously — the actor signs, hands the
    /// event to the publish engine, and the terminal verdict arrives through
    /// `projections["action_results"]` on a later tick.  Recording sites:
    /// `actor/dispatch.rs` (Requested), `kernel/publish_engine.rs`
    /// (Publishing / Accepted), `kernel/publish_cmd.rs` (Failed).
    #[rustfmt::skip]
    fn is_async_completing() -> bool { // doctrine-allow: D12 — recording sites in actor/dispatch.rs + kernel/publish_*.rs
        true
    }

    /// ADR-0064 / S3: opt into the typed FlatBuffers payload doorway. The decode
    /// (including the fail-closed `schema_version` gate, run BEFORE `start()`)
    /// delegates to `<PublishAction as ActionPayload>::decode` in
    /// `publish/wire.rs` — the single typed-decode site.
    fn decode_payload(bytes: &[u8]) -> Option<Result<Self::Action, ActionPayloadDecodeError>> {
        Some(<PublishAction as ActionPayload>::decode(bytes))
    }

    fn start(&self, _ctx: &mut ActionContext, action: Self::Action) -> Result<(), ActionRejection> {
        match action {
            PublishAction::Publish { .. } => Err(ActionRejection::Invalid(
                "pre-signed PublishAction::Publish is internal/protocol-only; apps must dispatch unsigned PublishRaw/PublishProfile/PublishReply builders"
                    .to_string(),
            )),
            PublishAction::PublishProfile { fields } => {
                // A kind:0 `content` is a flat JSON object of string values
                // (NIP-01 metadata). Reject any non-string field up front so a
                // malformed profile never reaches the actor.
                for (key, value) in &fields {
                    if !value.is_string() {
                        return Err(ActionRejection::Invalid(format!(
                            "profile field '{key}' must be a string value"
                        )));
                    }
                }
                Ok(())
            }
            PublishAction::PublishRaw { kind, target, .. } => {
                // Workstream C one-door: a raw app publish may not emit a kind
                // reserved to a dedicated typed builder (kind:0 → PublishProfile,
                // kind:3 → nmp.follow/unfollow, kind:10003 → NIP-51 bookmark
                // builders), which would bypass the builder's protocol-specific
                // processing. The reserved set + the rejection wording live in the
                // publish-policy classification table, not as scattered
                // `if kind == N` literals here (D0). The guard consults the table
                // and surfaces the table's reason verbatim.
                if let Some(reserved) = classify_publish_behavior(kind).reserved_builder() {
                    return Err(ActionRejection::Invalid(reserved.raw_publish_rejection()));
                }
                validate_publish_target(&target).map_err(ActionRejection::Invalid)?;
                // Workstream C one-door (D10): a private/encrypted envelope
                // (gift-wrap kind:1059, sealed kind:14) published raw with
                // `Auto` or an empty `Explicit` target is refused — it must
                // carry an explicit non-empty recipient-inbox relay set, never
                // Auto-route to public relays.
                validate_publish_routing(kind, &target).map_err(ActionRejection::Invalid)?;
                Ok(())
            }
            PublishAction::PublishReply {
                content,
                reply_to_event_id,
                target,
                ..
            } => {
                if content.trim().is_empty() {
                    return Err(ActionRejection::Invalid(
                        "reply content must not be empty".to_string(),
                    ));
                }
                if reply_to_event_id.len() != 64
                    || !reply_to_event_id.bytes().all(|b| b.is_ascii_hexdigit())
                {
                    return Err(ActionRejection::Invalid(
                        "reply_to_event_id must be a 64-character hex event id".to_string(),
                    ));
                }
                validate_publish_target(&target).map_err(ActionRejection::Invalid)?;
                Ok(())
            }
        }
    }

    fn execute(
        &self,
        _ctx: &ActionContext,
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        match action {
            PublishAction::Publish { .. } => {
                Err("pre-signed PublishAction::Publish is internal/protocol-only".to_string())
            }
            PublishAction::PublishProfile { fields } => {
                send(ActorCommand::Publish(PublishCommand::Profile {
                    fields,
                    correlation_id: Some(correlation_id.to_string()),
                }));
                Ok(())
            }
            PublishAction::PublishRaw {
                kind,
                tags,
                content,
                target,
                signer,
            } => {
                send(ActorCommand::Publish(PublishCommand::RawEvent {
                    kind,
                    tags,
                    content,
                    target,
                    signer_pubkey: signer.signer_pubkey(),
                    correlation_id: Some(correlation_id.to_string()),
                }));
                Ok(())
            }
            PublishAction::PublishReply {
                content,
                reply_to_event_id,
                target,
                signer,
            } => {
                send(ActorCommand::Publish(PublishCommand::Reply {
                    content,
                    reply_to_event_id,
                    target,
                    signer_pubkey: signer.signer_pubkey(),
                    correlation_id: Some(correlation_id.to_string()),
                }));
                Ok(())
            }
        }
    }
}

#[cfg(test)]
#[path = "action/tests.rs"]
mod tests;
