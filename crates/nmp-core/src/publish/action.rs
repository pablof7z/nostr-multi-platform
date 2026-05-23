//! `PublishAction` + `PublishModule` (the `ActionModule` impl).
//!
//! `start` is wired to the actor mailbox (M6): `ffi::action::execute_action`
//! validates a `PublishAction` through `ActionRegistry`, then converts a
//! `Publish` variant into `ActorCommand::PublishSignedEvent` for the actor
//! to publish. The publish engine drives per-relay transitions in-process;
//! its terminal verdict is surfaced as a [`PublishOutcome`] on the snapshot.

use serde::{Deserialize, Serialize};

use crate::actor::ActorCommand;
use crate::relay::CanonicalRelayUrl;
use crate::substrate::{ActionContext, ActionModule, ActionRejection, SignedEvent};

/// Stable handle returned to the caller of `Publish`. Used to key snapshot
/// entries and to address the action in the ledger when M6 wires the ledger.
pub type PublishHandle = String;

/// Relay URL — grep-able alias so the `RelayDispatcher` shim can be swapped
/// for `nmp-nip01::RelayManager` from M8 without changing call sites. Single
/// crate-wide definition lives in `crate::relay`; re-exported here so
/// `publish` import paths are unchanged.
pub use crate::relay::RelayUrl;

/// Where a publish should go.
///
/// `Auto` defers to the `OutboxResolver` (NIP-65 + indexer fallback per D3).
/// `Explicit` is the named opt-out (D3: "manual relay selection is the
/// opt-out").
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum PublishTarget {
    Auto,
    Explicit { relays: Vec<RelayUrl> },
}

/// `Auto` is the unambiguous default — the kernel resolves via NIP-65 (D3).
/// `Explicit` requires deliberate caller intent (a relay set), so it would
/// never make sense as a default. Needed by `#[serde(default)]` on
/// `PublishAction::PublishRaw::target` so a host JSON payload that omits
/// the field gets outbox routing rather than a deserialize error.
impl Default for PublishTarget {
    fn default() -> Self {
        Self::Auto
    }
}

/// Validate a publish target before it can cross the action/actor boundary.
///
/// `Auto` is always valid: it deliberately asks the kernel to resolve via
/// NIP-65. `Explicit` is fail-closed: an empty or malformed relay set is a
/// caller bug, not a request to silently widen to `Auto`.
pub(crate) fn validate_publish_target(target: &PublishTarget) -> Result<(), String> {
    match target {
        PublishTarget::Auto => Ok(()),
        PublishTarget::Explicit { relays } => validate_explicit_relays(relays),
    }
}

pub(crate) fn validate_explicit_relays(relays: &[RelayUrl]) -> Result<(), String> {
    if relays.is_empty() {
        return Err("explicit publish target requires at least one relay".to_string());
    }
    for relay in relays {
        if CanonicalRelayUrl::parse(relay).is_none() {
            return Err(format!(
                "explicit publish target relay '{relay}' must be a ws:// or wss:// relay URL"
            ));
        }
    }
    Ok(())
}

/// The single public publish action.
///
/// The signed event is included pre-signed because the kernel ledger (M6) will
/// sign once via the active signer and then enqueue the publish — we never
/// re-sign on retry (per the M6 exit gate "re-publish of an event preserves
/// `id` and `sig`").
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum PublishAction {
    Publish {
        handle: PublishHandle,
        event: SignedEvent,
        target: PublishTarget,
    },
    /// Sign-and-publish a kind:1 note (optionally a reply) with the active
    /// account. Unlike `Publish`, the event is *not* pre-signed — the actor
    /// signs it via the active signer. This is the `ActionModule`-native
    /// replacement for the deleted per-verb `nmp_app_publish_note` FFI symbol;
    /// the executor routes it to the existing `ActorCommand::PublishNote`
    /// handler.
    PublishNote {
        content: String,
        reply_to_id: Option<String>,
        target: PublishTarget,
    },
    /// Publish a kind:0 profile metadata event for the active account.
    /// `fields` is a flat JSON object with string-valued keys such as
    /// `"name"`, `"about"`, `"picture"` — the actor serializes it into the
    /// kind:0 `content` field, signs with the active signer, and routes
    /// through the NIP-65 outbox. Like `PublishNote`, the event is
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
    /// kind:0 (profile) and kind:3 (contacts) have dedicated variants that
    /// apply protocol-specific processing (kind:0: field validation, kind:3:
    /// follow-list merge). `PublishRaw` rejects these kinds to prevent
    /// accidental data loss from bypassing that processing.
    PublishRaw {
        kind: u32,
        tags: Vec<Vec<String>>,
        content: String,
        #[serde(default)]
        target: PublishTarget,
    },
    /// Cancel an in-flight publish, addressed by its [`PublishHandle`].
    ///
    /// This variant is the publish *engine's* internal command shape — it is
    /// constructed by `Kernel::cancel_publish` (the handler for
    /// `ActorCommand::CancelPublish`, the FFI symbol `nmp_app_cancel_publish`)
    /// and matched by `PublishEngine::start_publish`. It is deliberately NOT
    /// dispatchable through `dispatch_action`: `PublishModule::start` rejects
    /// it so the publish lifecycle's control plane (cancel / retry) stays on
    /// the dedicated FFI symbols rather than the generic action seam.
    Cancel { handle: PublishHandle },
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
    const NAMESPACE: &'static str = "nmp.publish";

    type Action = PublishAction;

    /// For pre-signed `Publish` actions, use the event's `id` as the
    /// `correlation_id`. The publish engine's `LastTerminal.correlation_id` is
    /// already the `PublishHandle` (== `event.id`), so using the same value
    /// here means `dispatch_action`'s return and `action_results` in the
    /// snapshot share the same identifier.
    ///
    /// `PublishNote` and `PublishProfile` return `None` — the event id isn't
    /// known until the actor signs. `Cancel` is not reachable through
    /// `dispatch_action` (`start` rejects it), so it never reaches this
    /// function; it falls into the `_` arm and returns `None`.
    fn preferred_action_id(action: &Self::Action) -> Option<crate::substrate::ActionId> {
        match action {
            PublishAction::Publish { event, .. } if !event.id.is_empty() => Some(event.id.clone()),
            _ => None,
        }
    }

    /// Publish actions settle asynchronously — the actor signs, hands the
    /// event to the publish engine, and the terminal verdict arrives through
    /// `projections["action_results"]` on a later tick.  Recording sites:
    /// `actor/dispatch.rs` (Requested), `kernel/publish_engine.rs`
    /// (Publishing / Accepted), `kernel/publish_cmd.rs` (Failed).
    fn is_async_completing() -> bool { // doctrine-allow: D12 — recording sites are cross-file (actor/dispatch.rs + kernel/publish_*.rs); exercised by kernel/action_stages_tests.rs
        true
    }

    fn start(
        _ctx: &mut ActionContext,
        action: Self::Action,
    ) -> Result<(), ActionRejection> {
        match action {
            PublishAction::Publish { event, target, .. } => {
                if event.id.is_empty() || event.sig.is_empty() {
                    return Err(ActionRejection::Invalid(
                        "publish action requires a signed event with id+sig".to_string(),
                    ));
                }
                validate_publish_target(&target).map_err(ActionRejection::Invalid)?;
                Ok(())
            }
            PublishAction::PublishNote {
                content, target, ..
            } => {
                if content.is_empty() {
                    return Err(ActionRejection::Invalid(
                        "publish note requires non-empty content".to_string(),
                    ));
                }
                validate_publish_target(&target).map_err(ActionRejection::Invalid)?;
                Ok(())
            }
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
                // Guard the reserved kinds that have dedicated variants with
                // protocol-specific processing.
                if kind == 0 {
                    return Err(ActionRejection::Invalid(
                        "use PublishProfile (not PublishRaw) for kind:0 profile updates".to_string(),
                    ));
                }
                if kind == 3 {
                    return Err(ActionRejection::Invalid(
                        "kind:3 contact-list must be modified via nmp.follow / nmp.unfollow, \
                         not PublishRaw (the actor owns the follow-list state)".to_string(),
                    ));
                }
                validate_publish_target(&target).map_err(ActionRejection::Invalid)?;
                Ok(())
            }
            // Cancel is engine-internal — it is constructed by
            // `Kernel::cancel_publish` for the `nmp_app_cancel_publish` FFI
            // symbol, never dispatched through `dispatch_action`. Reject it
            // here so the publish lifecycle's control plane stays on the
            // dedicated FFI door and `dispatch_action` carries nothing for
            // cancel. Previously this arm was an accepting no-op whose
            // executor counterpart did `Ok(())` — a dead path that looked
            // alive on the action seam.
            PublishAction::Cancel { .. } => Err(ActionRejection::Invalid(
                "publish cancel is not dispatchable via dispatch_action; \
                 use the nmp_app_cancel_publish FFI symbol"
                    .to_string(),
            )),
        }
    }

    fn execute(
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        match action {
            PublishAction::Publish { event, target, .. } => {
                send(ActorCommand::PublishSignedEvent {
                    raw: publish_signed_event_to_raw(event),
                    target,
                    correlation_id: Some(correlation_id.to_string()),
                });
                Ok(())
            }
            PublishAction::PublishNote { content, reply_to_id, target } => {
                send(ActorCommand::PublishNote {
                    content,
                    reply_to_id,
                    target,
                    correlation_id: Some(correlation_id.to_string()),
                });
                Ok(())
            }
            PublishAction::PublishProfile { fields } => {
                send(ActorCommand::PublishProfile {
                    fields,
                    correlation_id: Some(correlation_id.to_string()),
                });
                Ok(())
            }
            PublishAction::PublishRaw { kind, tags, content, target } => {
                send(ActorCommand::PublishRawEvent {
                    kind,
                    tags,
                    content,
                    target,
                    correlation_id: Some(correlation_id.to_string()),
                });
                Ok(())
            }
            // Cancel is rejected by `start` before `execute` is reached.
            // This arm exists only for match exhaustiveness (D6 — no
            // `unreachable!()` on a production path).
            PublishAction::Cancel { .. } => Ok(()),
        }
    }
}

/// Convert a [`SignedEvent`] into the flat [`crate::store::RawEvent`] shape
/// the actor's publish command expects. Pure field move — no re-signing.
fn publish_signed_event_to_raw(event: SignedEvent) -> crate::store::RawEvent {
    crate::store::RawEvent {
        id: event.id,
        pubkey: event.unsigned.pubkey,
        created_at: event.unsigned.created_at,
        kind: event.unsigned.kind,
        tags: event.unsigned.tags,
        content: event.unsigned.content,
        sig: event.sig,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::substrate::UnsignedEvent;

    fn ctx() -> ActionContext {
        ActionContext::default()
    }

    fn signed_event() -> SignedEvent {
        SignedEvent {
            id: "a".repeat(64),
            sig: "b".repeat(128),
            unsigned: UnsignedEvent {
                pubkey: "c".repeat(64),
                kind: 1,
                tags: Vec::new(),
                content: "hello".to_string(),
                created_at: 1_700_000_000,
            },
        }
    }

    #[test]
    fn explicit_publish_target_requires_non_empty_relays() {
        let action = PublishAction::PublishNote {
            content: "hello".to_string(),
            reply_to_id: None,
            target: PublishTarget::Explicit { relays: Vec::new() },
        };
        let err = PublishModule::start(&mut ctx(), action)
            .expect_err("empty explicit target must fail closed");
        assert!(matches!(err, ActionRejection::Invalid(msg) if msg.contains("at least one relay")));
    }

    #[test]
    fn explicit_publish_target_rejects_malformed_relay_url() {
        let action = PublishAction::Publish {
            handle: "h".to_string(),
            event: signed_event(),
            target: PublishTarget::Explicit {
                relays: vec!["https://relay.example".to_string()],
            },
        };
        let err = PublishModule::start(&mut ctx(), action)
            .expect_err("malformed explicit relay must be rejected");
        assert!(matches!(err, ActionRejection::Invalid(msg) if msg.contains("ws:// or wss://")));
    }

    #[test]
    fn explicit_publish_target_accepts_valid_relay_url() {
        let action = PublishAction::PublishNote {
            content: "hello".to_string(),
            reply_to_id: None,
            target: PublishTarget::Explicit {
                relays: vec!["wss://relay.example".to_string()],
            },
        };
        PublishModule::start(&mut ctx(), action)
            .expect("valid explicit target should pass validation");
    }

    #[test]
    fn publish_raw_rejects_kind_0_to_protect_profile_path() {
        // kind:0 has dedicated `PublishProfile` handling (field validation +
        // string-typed-content guarantee). Routing it through `PublishRaw`
        // would bypass that, so the guard fails closed at `start`.
        let action = PublishAction::PublishRaw {
            kind: 0,
            tags: Vec::new(),
            content: "{}".to_string(),
            target: PublishTarget::Auto,
        };
        let err = PublishModule::start(&mut ctx(), action)
            .expect_err("PublishRaw must reject kind:0");
        assert!(matches!(err, ActionRejection::Invalid(msg) if msg.contains("PublishProfile")));
    }

    #[test]
    fn publish_raw_rejects_kind_3_pending_dedicated_path() {
        // kind:3 (contact list) needs a follow-list-merge step; PublishRaw
        // would publish the raw payload verbatim, silently overwriting the
        // user's existing follow set. Fail closed until a dedicated variant
        // (or contacts-aware PublishRaw branch) lands.
        let action = PublishAction::PublishRaw {
            kind: 3,
            tags: Vec::new(),
            content: String::new(),
            target: PublishTarget::Auto,
        };
        let err = PublishModule::start(&mut ctx(), action)
            .expect_err("PublishRaw must reject kind:3");
        assert!(matches!(err, ActionRejection::Invalid(msg) if msg.contains("kind:3")));
    }

    #[test]
    fn publish_raw_accepts_arbitrary_event_kind_with_auto_target() {
        // A kind:30023 (long-form article) is the canonical second-app
        // motivation. `Auto` target must pass validation — `#[serde(default)]`
        // + `Default::default() == Auto` is the host-omits-the-field path,
        // so it has to be a valid input.
        let action = PublishAction::PublishRaw {
            kind: 30023,
            tags: vec![vec!["d".to_string(), "my-article".to_string()]],
            content: "# Hello, second app".to_string(),
            target: PublishTarget::Auto,
        };
        PublishModule::start(&mut ctx(), action)
            .expect("valid PublishRaw with Auto target should pass validation");
    }

    #[test]
    fn publish_raw_propagates_explicit_target_validation_failure() {
        // The kind guard runs first, but past it the existing
        // `validate_publish_target` must still apply — an explicit empty
        // relay set fails closed exactly as for `PublishNote`.
        let action = PublishAction::PublishRaw {
            kind: 30023,
            tags: Vec::new(),
            content: "body".to_string(),
            target: PublishTarget::Explicit { relays: Vec::new() },
        };
        let err = PublishModule::start(&mut ctx(), action)
            .expect_err("empty explicit target must fail closed for PublishRaw too");
        assert!(matches!(err, ActionRejection::Invalid(msg) if msg.contains("at least one relay")));
    }

    #[test]
    fn publish_target_default_is_auto_for_serde_omitted_field() {
        // `#[serde(default)] target: PublishTarget` on PublishRaw relies
        // on Default returning Auto. Lock that in so a future contributor
        // can't quietly flip it to Explicit and silently widen routing.
        assert_eq!(PublishTarget::default(), PublishTarget::Auto);
    }

    fn run_execute(action: PublishAction) -> Result<Vec<ActorCommand>, String> {
        use std::cell::RefCell;
        let captured: RefCell<Vec<ActorCommand>> = RefCell::new(Vec::new());
        PublishModule::execute(action, "test-cid", &|cmd| {
            captured.borrow_mut().push(cmd);
        })?;
        Ok(captured.into_inner())
    }

    #[test]
    fn execute_publish_note_emits_publish_note_command() {
        let action = PublishAction::PublishNote {
            content: "hello".to_string(),
            reply_to_id: None,
            target: PublishTarget::Auto,
        };
        let cmds = run_execute(action).expect("execute must succeed");
        assert_eq!(cmds.len(), 1, "must emit exactly one command");
        match cmds.into_iter().next().unwrap() {
            ActorCommand::PublishNote { content, reply_to_id, target, correlation_id } => {
                assert_eq!(content, "hello");
                assert_eq!(reply_to_id, None);
                assert_eq!(target, PublishTarget::Auto);
                assert_eq!(correlation_id.as_deref(), Some("test-cid"));
            }
            other => panic!("expected PublishNote, got {other:?}"),
        }
    }

    #[test]
    fn execute_publish_profile_emits_publish_profile_command() {
        let mut fields = serde_json::Map::new();
        fields.insert("display_name".to_string(), serde_json::Value::String("Alice".to_string()));
        let action = PublishAction::PublishProfile { fields };
        let cmds = run_execute(action).expect("execute must succeed");
        assert_eq!(cmds.len(), 1, "must emit exactly one command");
        match cmds.into_iter().next().unwrap() {
            ActorCommand::PublishProfile { fields, correlation_id } => {
                assert_eq!(
                    fields.get("display_name").and_then(|v| v.as_str()),
                    Some("Alice"),
                );
                assert_eq!(correlation_id.as_deref(), Some("test-cid"));
            }
            other => panic!("expected PublishProfile, got {other:?}"),
        }
    }

    #[test]
    fn execute_publish_raw_emits_publish_raw_event_command() {
        let action = PublishAction::PublishRaw {
            kind: 30023,
            tags: vec![vec!["d".to_string(), "slug".to_string()]],
            content: "body".to_string(),
            target: PublishTarget::Auto,
        };
        let cmds = run_execute(action).expect("execute must succeed");
        assert_eq!(cmds.len(), 1, "must emit exactly one command");
        match cmds.into_iter().next().unwrap() {
            ActorCommand::PublishRawEvent { kind, content, target, correlation_id, .. } => {
                assert_eq!(kind, 30023);
                assert_eq!(content, "body");
                assert_eq!(target, PublishTarget::Auto);
                assert_eq!(correlation_id.as_deref(), Some("test-cid"));
            }
            other => panic!("expected PublishRawEvent, got {other:?}"),
        }
    }

    #[test]
    fn execute_publish_signed_event_emits_publish_signed_event_command() {
        let action = PublishAction::Publish {
            handle: "h".to_string(),
            event: signed_event(),
            target: PublishTarget::Auto,
        };
        let cmds = run_execute(action).expect("execute must succeed");
        assert_eq!(cmds.len(), 1, "must emit exactly one command");
        match cmds.into_iter().next().unwrap() {
            ActorCommand::PublishSignedEvent { raw, target, correlation_id } => {
                assert_eq!(raw.kind, 1);
                assert_eq!(target, PublishTarget::Auto);
                assert_eq!(correlation_id.as_deref(), Some("test-cid"));
            }
            other => panic!("expected PublishSignedEvent, got {other:?}"),
        }
    }
}
