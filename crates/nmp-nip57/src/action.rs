//! `ZapAction` + `ZapModule` — the NIP-57 zap-request `ActionModule`.
//!
//! This is a protocol-crate `ActionModule` alongside `nmp-nip29`'s group-chat
//! actions. It wires the `start()` validation half of the
//! `nmp.zap` namespace into the kernel's `ActionRegistry`; the executor half
//! ([`zap_request_command`]) builds the kind:9734 zap-request `UnsignedEvent`
//! and maps it to an [`nmp_core::ActorCommand`].
//!
//! # Scope — what this does and does NOT do
//!
//! NIP-57 zaps have two legs:
//!
//! 1. **Zap request** (kind:9734) — a Nostr event the client builds and the
//!    LN provider embeds in the receipt. This crate owns it: [`ZapModule`]
//!    validates the request and [`zap_request_command`] builds the unsigned
//!    event ready for the actor to sign + publish.
//! 2. **LNURL-pay callback** — the signed kind:9734 must actually be POSTed
//!    to the recipient's `lnurl` callback endpoint over HTTP to obtain a
//!    bolt11 invoice; the wallet then pays that invoice and the LN provider
//!    publishes the kind:9735 receipt.
//!
//! Leg 2 (the LNURL HTTP round-trip) has **no kernel transport seam**. An
//! earlier `HttpCapability` `CapabilityModule` scaffold was built for this leg
//! but stayed inert across several direction reviews and was deleted once its
//! only prospective consumer — this module's executor — never wired up. A
//! future ADR will need to design the host HTTP seam afresh.
//!
//! So this module remains a *scaffold* for the LNURL leg: [`ZapAction::Zap`]
//! carries the `lnurl` field through validation, and [`zap_request_command`]
//! publishes the kind:9734 to Nostr relays (the `relays` tag's relays).
//! Issuing the actual HTTP `GET`/`POST` round-trip to the `lnurl` endpoint is
//! a deferred follow-up that depends on that not-yet-designed transport seam.

use nmp_core::substrate::{ActionContext, ActionModule, ActionRejection};
use nmp_core::ActorCommand;
use serde::{Deserialize, Serialize};

use crate::build::ZapRequest;

/// A NIP-57 zap action.
///
/// Modelled as a single-variant enum (mirroring `PublishAction`) so future
/// zap verbs (e.g. an explicit `Cancel`) slot in without a breaking change to
/// the serde wire shape — the externally-tagged `{"Zap":{…}}` envelope is
/// forward-compatible.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum ZapAction {
    Zap {
        /// The event id or `naddr`/`a`-coordinate to zap (hex or bech32).
        zapped_event_id: String,
        /// Recipient's pubkey (64 lowercase hex chars).
        recipient_pubkey: String,
        /// Satoshis to zap. Converted to msats for the kind:9734 `amount`
        /// tag (`amount_sats * 1000`).
        amount_sats: u64,
        /// LNURL-pay callback endpoint from the recipient's kind:0
        /// `lud16`/`lud06`. Carried through validation for the HTTP executor
        /// that POSTs the signed kind:9734 here — see the module docs: the
        /// HTTP transport seam is not yet designed, so executor wiring through
        /// it is a deferred follow-up.
        lnurl: String,
        /// Relays the recipient should watch for the kind:9735 receipt. The
        /// kind:9734 `relays` tag is mandatory per NIP-57; it is ALSO the
        /// relay set the executor publishes the request to.
        ///
        /// Not in the original task spec — added because
        /// `ZapRequestBuilder::build()` returns `MissingRelays` without it
        /// and the request cannot be constructed otherwise.
        relays: Vec<String>,
        /// Optional free-form comment attached to the zap request.
        #[serde(default)]
        comment: Option<String>,
    },
}

/// `ActionModule` impl for the `nmp.zap` namespace.
///
/// `start` is a pure validator (no `ActionPlan`, no `type Step` — both were
/// removed at the `dispatch_action` boundary). It rejects a zap request that
/// could never produce a valid kind:9734.
pub struct ZapModule;

impl ActionModule for ZapModule {
    const NAMESPACE: &'static str = "nmp.zap";

    type Action = ZapAction;

    fn start(
        _ctx: &mut ActionContext,
        action: Self::Action,
    ) -> Result<(), ActionRejection> {
        let ZapAction::Zap {
            zapped_event_id,
            recipient_pubkey,
            amount_sats,
            lnurl,
            relays,
            ..
        } = action;

        if zapped_event_id.trim().is_empty() {
            return Err(ActionRejection::Invalid(
                "zap action requires a non-empty zapped_event_id".to_string(),
            ));
        }
        if !is_hex64(&recipient_pubkey) {
            return Err(ActionRejection::Invalid(
                "zap action requires recipient_pubkey to be 64 hex chars".to_string(),
            ));
        }
        if amount_sats == 0 {
            return Err(ActionRejection::Invalid(
                "zap action requires amount_sats greater than zero".to_string(),
            ));
        }
        if lnurl.trim().is_empty() {
            return Err(ActionRejection::Invalid(
                "zap action requires a non-empty lnurl endpoint".to_string(),
            ));
        }
        // `ZapRequestBuilder::build()` rejects an empty relay set with
        // `MissingRelays`; reject it here so the failure surfaces as a clean
        // `start()` rejection rather than an executor-time build error.
        if relays.iter().all(|r| r.trim().is_empty()) {
            return Err(ActionRejection::Invalid(
                "zap action requires at least one non-empty relay".to_string(),
            ));
        }
        Ok(())
    }
}

/// `true` when `s` is exactly 64 lowercase-or-uppercase ASCII hex chars — the
/// shape of a Nostr pubkey / event id.
fn is_hex64(s: &str) -> bool {
    s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit())
}

/// Map a validated `nmp.zap` action JSON to the [`ActorCommand`] that
/// publishes the kind:9734 zap-request event.
///
/// Split out of the executor closure (mirroring `nmp-nip29`'s
/// `post_chat_message_command`) so the action→command mapping is unit
/// testable without the FFI / actor channel. Re-decodes its own input — the
/// executor never trusts an upstream shape it did not verify.
///
/// The `pubkey` on the built `UnsignedEvent` is a placeholder: the actor
/// derives it from the active identity at sign time and overwrites this
/// field. `created_at` is set to 0 as a sentinel; the actor re-stamps it
/// via `kernel.now_secs()` (D7 — kernel owns the wall clock).
///
/// Routes via [`ActorCommand::PublishUnsignedEventToRelays`] pinned to the
/// zap request's own `relays` set — the recipient watches exactly those
/// relays for the kind:9735 receipt, so the request must land there rather
/// than route through the author's NIP-65 outbox.
pub fn zap_request_command(action_json: &str) -> Result<ActorCommand, String> {
    let action: ZapAction =
        serde_json::from_str(action_json).map_err(|e| e.to_string())?;
    let ZapAction::Zap {
        zapped_event_id,
        recipient_pubkey,
        amount_sats,
        relays,
        comment,
        ..
    } = action;

    // Filter whitespace-only relays once so the kind:9734 `relays` tag (built
    // by `ZapRequestBuilder`, which filters internally) and the
    // `ActorCommand`'s relay set are guaranteed identical — the recipient
    // watches exactly the relays the request is published to.
    let relays: Vec<String> =
        relays.into_iter().filter(|r| !r.trim().is_empty()).collect();

    let mut builder = ZapRequest::to_pubkey(recipient_pubkey)
        // NIP-57: `amount` is msats. 1 sat = 1000 msat.
        .amount_msats(amount_sats.saturating_mul(1000))
        .relays(relays.clone())
        .zapped_event(zapped_event_id);
    if let Some(c) = comment {
        builder = builder.comment(c);
    }

    // `pubkey` placeholder — the actor overwrites it at sign time.
    // `created_at = 0` — the actor re-stamps via kernel.now_secs() (D7).
    let unsigned = builder
        .build(String::new(), 0)
        .map_err(|e| e.to_string())?;

    Ok(ActorCommand::PublishUnsignedEventToRelays {
        event: unsigned,
        relays,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> ActionContext {
        ActionContext { now_ms: 1_700_000_000_000 }
    }

    /// Build a `Zap` action with sensible defaults; each field is an arg so a
    /// test can vary exactly one (struct-update syntax does not work on enum
    /// variants, so this constructor stands in for it).
    fn zap(
        zapped_event_id: &str,
        recipient_pubkey: &str,
        amount_sats: u64,
        lnurl: &str,
        relays: Vec<&str>,
        comment: Option<&str>,
    ) -> ZapAction {
        ZapAction::Zap {
            zapped_event_id: zapped_event_id.to_string(),
            recipient_pubkey: recipient_pubkey.to_string(),
            amount_sats,
            lnurl: lnurl.to_string(),
            relays: relays.into_iter().map(String::from).collect(),
            comment: comment.map(String::from),
        }
    }

    fn valid_action() -> ZapAction {
        zap(
            &"b".repeat(64),
            &"a".repeat(64),
            21,
            "https://ln.example/.well-known/lnurlp/alice",
            vec!["wss://relay.example"],
            Some("great post"),
        )
    }

    #[test]
    fn namespace_is_nmp_zap() {
        assert_eq!(ZapModule::NAMESPACE, "nmp.zap");
    }

    #[test]
    fn start_accepts_a_valid_zap() {
        assert!(ZapModule::start(&mut ctx(), valid_action()).is_ok());
    }

    #[test]
    fn start_rejects_empty_zapped_event_id() {
        let action = zap(
            "   ",
            &"a".repeat(64),
            21,
            "https://x",
            vec!["wss://r"],
            None,
        );
        let err = ZapModule::start(&mut ctx(), action).unwrap_err();
        assert!(matches!(err, ActionRejection::Invalid(m) if m.contains("zapped_event_id")));
    }

    #[test]
    fn start_rejects_non_hex64_recipient_pubkey() {
        let action = zap(
            &"b".repeat(64),
            "not-hex",
            21,
            "https://x",
            vec!["wss://r"],
            None,
        );
        let err = ZapModule::start(&mut ctx(), action).unwrap_err();
        assert!(matches!(err, ActionRejection::Invalid(m) if m.contains("64 hex chars")));
    }

    #[test]
    fn start_rejects_short_recipient_pubkey() {
        let action = zap(
            &"b".repeat(64),
            "abc",
            21,
            "https://x",
            vec!["wss://r"],
            None,
        );
        assert!(ZapModule::start(&mut ctx(), action).is_err());
    }

    #[test]
    fn start_rejects_zero_amount() {
        let action = zap(
            &"b".repeat(64),
            &"a".repeat(64),
            0,
            "https://x",
            vec!["wss://r"],
            None,
        );
        let err = ZapModule::start(&mut ctx(), action).unwrap_err();
        assert!(matches!(err, ActionRejection::Invalid(m) if m.contains("amount_sats")));
    }

    #[test]
    fn start_rejects_empty_lnurl() {
        let action = zap(
            &"b".repeat(64),
            &"a".repeat(64),
            21,
            "  ",
            vec!["wss://r"],
            None,
        );
        let err = ZapModule::start(&mut ctx(), action).unwrap_err();
        assert!(matches!(err, ActionRejection::Invalid(m) if m.contains("lnurl")));
    }

    #[test]
    fn start_rejects_empty_relays() {
        let action = zap(
            &"b".repeat(64),
            &"a".repeat(64),
            21,
            "https://x",
            vec!["   "],
            None,
        );
        let err = ZapModule::start(&mut ctx(), action).unwrap_err();
        assert!(matches!(err, ActionRejection::Invalid(m) if m.contains("relay")));
    }

    #[test]
    fn zap_action_round_trips_through_serde() {
        let action = valid_action();
        let json = serde_json::to_string(&action).unwrap();
        let back: ZapAction = serde_json::from_str(&json).unwrap();
        assert_eq!(action, back);
    }

    #[test]
    fn comment_defaults_to_none_when_absent() {
        let json = format!(
            r#"{{"Zap":{{"zapped_event_id":"{}","recipient_pubkey":"{}","amount_sats":21,"lnurl":"https://x","relays":["wss://r"]}}}}"#,
            "b".repeat(64),
            "a".repeat(64),
        );
        let action: ZapAction = serde_json::from_str(&json).unwrap();
        let ZapAction::Zap { comment, .. } = action;
        assert_eq!(comment, None);
    }

    #[test]
    fn zap_request_command_builds_publish_unsigned_to_relays() {
        let json = serde_json::to_string(&valid_action()).unwrap();
        let cmd = zap_request_command(&json).expect("valid zap should map to a command");
        match cmd {
            ActorCommand::PublishUnsignedEventToRelays { event, relays } => {
                assert_eq!(event.kind, crate::kinds::KIND_ZAP_REQUEST);
                assert_eq!(relays, vec!["wss://relay.example".to_string()]);
                // amount tag carries msats: 21 sats -> 21000 msats.
                let amount = event
                    .tags
                    .iter()
                    .find(|t| t.first().map(String::as_str) == Some("amount"))
                    .expect("amount tag present");
                assert_eq!(amount[1], "21000");
                // recipient is the single `p` tag.
                let p = event
                    .tags
                    .iter()
                    .find(|t| t.first().map(String::as_str) == Some("p"))
                    .expect("p tag present");
                assert_eq!(p[1], "a".repeat(64));
                // zapped event lands as an `e` tag.
                let e = event
                    .tags
                    .iter()
                    .find(|t| t.first().map(String::as_str) == Some("e"))
                    .expect("e tag present");
                assert_eq!(e[1], "b".repeat(64));
                // comment lands in content.
                assert_eq!(event.content, "great post");
            }
            other => panic!("expected PublishUnsignedEventToRelays, got {other:?}"),
        }
    }

    #[test]
    fn zap_request_command_rejects_malformed_json() {
        assert!(zap_request_command("{not json").is_err());
    }

    #[test]
    fn zap_request_command_omits_comment_when_none() {
        let json = format!(
            r#"{{"Zap":{{"zapped_event_id":"{}","recipient_pubkey":"{}","amount_sats":5,"lnurl":"https://x","relays":["wss://r"]}}}}"#,
            "b".repeat(64),
            "a".repeat(64),
        );
        let cmd = zap_request_command(&json).expect("valid zap");
        match cmd {
            ActorCommand::PublishUnsignedEventToRelays { event, .. } => {
                assert_eq!(event.content, "");
            }
            other => panic!("expected PublishUnsignedEventToRelays, got {other:?}"),
        }
    }
}
