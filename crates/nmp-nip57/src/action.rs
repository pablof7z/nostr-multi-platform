//! `nmp.nip57.zap` — the NIP-57 lightning zap [`ActionModule`].
//!
//! Validates a zap request in [`ZapAction::start`], builds an unsigned
//! kind:9734 via [`ZapRequest`] (`crate::build`), and dispatches
//! [`ActorCommand::FetchLnurlInvoice`] (the ADR-0024 LNURL-pay round-trip).
//! The actor signs the kind:9734, fetches the receiver's LNURL callback, and
//! surfaces the resulting bolt11 invoice as a `ShowToast` follow-up.
//!
//! # Wire routing
//!
//! NIP-57 § "Appendix C": the signed kind:9734 goes to the LN provider's
//! LNURL **callback URL** as `nostr=<urlencoded>` — NOT to Nostr relays.
//! The kind:9735 receipt is what relays receive; the LN provider mints it
//! after the invoice settles.
//!
//! # Signing constraint (ADR-0026 Phase 1)
//!
//! The actor reads `IdentityRuntime::active_local_keys` to sign the
//! kind:9734. Bunker (NIP-46) accounts return `None`; the actor fails closed
//! with a toast and records `ActionFailure` against the correlation_id.
//! Remote-signer signing is the ADR-0026 Phase 2 follow-up.

use nmp_core::substrate::{ActionContext, ActionModule, ActionRejection};
use nmp_core::ActorCommand;
use serde::{Deserialize, Serialize};

use crate::build::ZapRequest;

/// Wire shape for `nmp.nip57.zap` — the JSON body a host passes to
/// `nmp_app_dispatch_action`.
///
/// ```json
/// {
///   "recipient_pubkey": "<hex>",
///   "amount_msats": 21000,
///   "lnurl": "alice@walletofsatoshi.com",
///   "relays": [],
///   "target_event_id": "<hex>",
///   "comment": "🤙"
/// }
/// ```
///
/// `relays` is optional (`[]` or omitted) — the actor injects from
/// `kernel.author_write_relays(recipient_pubkey)` before signing (V-07).
///
/// `lnurl` carries the receiver's LNURL-pay endpoint in any of three
/// shapes: a lightning address (`user@domain`), a bech32 LNURL
/// (`lnurl1…`), or a bare `https://` URL — `nmp-core::actor::commands::zap`
/// decodes all three per LUD-01/06/16.
///
/// `target_event_id` and `comment` are optional. A zap to a profile (no
/// target event) omits `target_event_id`. `relays` may be empty, in which
/// case the actor selects from the recipient's kind:10002 (NIP-65) write
/// relays before signing — that's the only D0-correct answer (V-07).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ZapInput {
    /// Recipient's Nostr public key, lowercase hex.
    pub recipient_pubkey: String,
    /// Amount in millisatoshis. Must be > 0.
    pub amount_msats: u64,
    /// Receiver's LNURL-pay endpoint — lightning address, bech32 LNURL, or
    /// bare https URL. Required by NIP-57: a zap intent without the LN
    /// destination cannot fetch the bolt11.
    pub lnurl: String,
    /// Relay URLs for the kind:9734 `relays` tag. When empty the actor
    /// auto-selects from the recipient's kind:10002 (NIP-65) write/both
    /// relays — relay selection is policy that lives in the kernel, never
    /// the shell (V-07).
    #[serde(default)]
    pub relays: Vec<String>,
    /// Optional zapped event id (hex). When set, the kind:9734 carries an `e`
    /// tag pointing at the target note.
    #[serde(default)]
    pub target_event_id: Option<String>,
    /// Optional free-form comment — becomes the kind:9734 `content`.
    #[serde(default)]
    pub comment: Option<String>,
}

/// The `nmp.nip57.zap` [`ActionModule`].
///
/// `start` validates the zap input. `execute` builds the unsigned
/// kind:9734 zap request via [`ZapRequestBuilder`] and enqueues
/// [`ActorCommand::FetchLnurlInvoice`] — the actor handles signing
/// (D7 — kernel owns key access) and the off-thread LNURL-pay HTTP
/// round-trip (D8 — no blocking on the actor thread).
pub struct ZapAction;

impl ActionModule for ZapAction {
    const NAMESPACE: &'static str = "nmp.nip57.zap";
    type Action = ZapInput;

    /// Validate a zap request. Rejects:
    /// - empty `recipient_pubkey`
    /// - `amount_msats == 0`
    /// - empty `lnurl` (receiver LN destination is required)
    ///
    /// `relays` may be empty: the actor auto-selects from the recipient's
    /// kind:10002 (NIP-65) write list before signing (V-07). Relay choice
    /// is policy that lives in the kernel, not the shell.
    fn start(
        _ctx: &mut ActionContext,
        action: Self::Action,
    ) -> Result<(), ActionRejection> {
        if action.recipient_pubkey.trim().is_empty() {
            return Err(ActionRejection::Invalid(
                "zap requires a recipient pubkey".into(),
            ));
        }
        if action.amount_msats == 0 {
            return Err(ActionRejection::Invalid(
                "zap amount must be greater than 0 msats".into(),
            ));
        }
        if action.lnurl.trim().is_empty() {
            return Err(ActionRejection::Invalid(
                "zap requires the receiver's LNURL-pay endpoint (lightning address, bech32 LNURL, or https URL)".into(),
            ));
        }
        Ok(())
    }

    /// Settles asynchronously: `execute` enqueues `FetchLnurlInvoice` and
    /// returns immediately; the actor's HTTP worker surfaces the bolt11 (or
    /// failure) via `ShowToast`/`RecordActionFailure`. Recording sites:
    /// `actor/dispatch.rs` (Requested), `actor/commands/zap.rs` (Failed).
    fn is_async_completing() -> bool { // doctrine-allow: D12 — recording sites are cross-file (actor/dispatch.rs FetchLnurlInvoice arm sets Requested; actor/commands/zap.rs sets Failed on pre-payment errors)
        true
    }

    /// Build the unsigned kind:9734 and enqueue
    /// [`ActorCommand::FetchLnurlInvoice`].
    ///
    /// # D7 — kernel owns the wall clock
    ///
    /// `created_at` is passed as `0`; the actor re-stamps from
    /// `kernel.now_secs()` before signing. Matches the
    /// `PublishUnsignedEventToRelays` precedent.
    ///
    /// # D8 — no blocking
    ///
    /// The closure neither HTTPs nor signs; the actor's
    /// `FetchLnurlInvoice` arm does both off-thread.
    fn execute(
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        // Filter empty/whitespace relays (already partially done in start;
        // re-filter so the builder gets the cleaned set without re-running
        // the validator).
        let relays: Vec<String> = action
            .relays
            .iter()
            .filter(|r| !r.trim().is_empty())
            .cloned()
            .collect();
        let mut builder = ZapRequest::to_pubkey(&action.recipient_pubkey)
            .amount_msats(action.amount_msats)
            .relays(relays);
        if let Some(ref id) = action.target_event_id {
            builder = builder.zapped_event(id);
        }
        if let Some(ref comment) = action.comment {
            builder = builder.comment(comment);
        }
        // `author` is the kernel-resolved active account at sign time —
        // the actor overrides this when it builds the signed event. Pass an
        // empty placeholder; the substrate `UnsignedEvent` carries it
        // through unchanged but the actor's `sign_zap_request` resigns from
        // the active `Keys` (its pubkey is what `EventBuilder` stamps).
        // `created_at = 0` is the D7 sentinel — re-stamped on the actor.
        let unsigned = builder
            .build(String::new(), 0)
            .map_err(|e| format!("build kind:9734 zap request: {e}"))?;
        send(ActorCommand::FetchLnurlInvoice {
            unsigned,
            lnurl_or_address: action.lnurl,
            amount_msats: action.amount_msats,
            correlation_id: Some(correlation_id.to_string()),
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_core::substrate::ActionContext;
    use std::cell::RefCell;

    /// Run the typed executor and capture every `ActorCommand` it sends, in order.
    fn run_execute(input: ZapInput) -> Result<Vec<ActorCommand>, String> {
        let captured: RefCell<Vec<ActorCommand>> = RefCell::new(Vec::new());
        ZapAction::execute(input, "cid-deadbeef", &|cmd| {
            captured.borrow_mut().push(cmd);
        })?;
        Ok(captured.into_inner())
    }

    const RECIPIENT: &str =
        "bb11223344556677889900aabbccddeeff00112233445566778899aabbccddff";
    const RELAY: &str = "wss://relay.damus.io";
    const LNURL: &str = "alice@walletofsatoshi.com";

    fn ctx() -> ActionContext {
        ActionContext::default()
    }

    fn well_formed_input() -> ZapInput {
        ZapInput {
            recipient_pubkey: RECIPIENT.to_string(),
            amount_msats: 21_000,
            lnurl: LNURL.to_string(),
            relays: vec![RELAY.to_string()],
            target_event_id: None,
            comment: None,
        }
    }

    #[test]
    fn namespace_is_nmp_nip57_zap() {
        assert_eq!(ZapAction::NAMESPACE, "nmp.nip57.zap");
    }

    #[test]
    fn is_async_completing_is_true() {
        // Zap settles asynchronously — host should subscribe to action_stages.
        assert!(ZapAction::is_async_completing());
    }

    #[test]
    fn start_accepts_well_formed_input() {
        assert!(ZapAction::start(&mut ctx(), well_formed_input()).is_ok());
    }

    #[test]
    fn start_accepts_input_with_target_event_and_comment() {
        let input = ZapInput {
            target_event_id: Some(
                "aabb1122334455660011223344556677889900112233445566778899aabbccdd".to_string(),
            ),
            comment: Some("great post".to_string()),
            ..well_formed_input()
        };
        assert!(ZapAction::start(&mut ctx(), input).is_ok());
    }

    #[test]
    fn start_rejects_empty_recipient() {
        let input = ZapInput {
            recipient_pubkey: "   ".to_string(),
            ..well_formed_input()
        };
        assert!(matches!(
            ZapAction::start(&mut ctx(), input),
            Err(ActionRejection::Invalid(_))
        ));
    }

    #[test]
    fn start_rejects_zero_amount() {
        let input = ZapInput {
            amount_msats: 0,
            ..well_formed_input()
        };
        assert!(matches!(
            ZapAction::start(&mut ctx(), input),
            Err(ActionRejection::Invalid(_))
        ));
    }

    #[test]
    fn start_rejects_empty_lnurl() {
        let input = ZapInput {
            lnurl: "   ".to_string(),
            ..well_formed_input()
        };
        assert!(matches!(
            ZapAction::start(&mut ctx(), input),
            Err(ActionRejection::Invalid(_))
        ));
    }

    /// V-07: empty relays is VALID — the actor injects the recipient's
    /// NIP-65 write list before signing. The executor still emits
    /// `FetchLnurlInvoice`; the resulting kind:9734 has no `relays` tag at
    /// this point (the actor adds it in `handle_fetch_lnurl_invoice`).
    #[test]
    fn start_accepts_empty_relays_actor_injects() {
        let input = ZapInput {
            relays: vec![],
            ..well_formed_input()
        };
        assert!(ZapAction::start(&mut ctx(), input).is_ok());
    }

    /// V-07 sibling: whitespace-only relays filter to empty and follow the
    /// same auto-inject path — accepted at `start`, no `relays` tag emitted
    /// by the builder (the actor injects from kind:10002 later).
    #[test]
    fn start_accepts_whitespace_only_relays_actor_injects() {
        let input = ZapInput {
            relays: vec!["   ".to_string(), "\t".to_string()],
            ..well_formed_input()
        };
        assert!(ZapAction::start(&mut ctx(), input).is_ok());
    }

    /// The executor must emit a `FetchLnurlInvoice` carrying the full
    /// validated zap intent — NOT the previous `ShowToast` stub. This pins
    /// the post-ADR-0024 contract: the LNURL fetch runs off-thread in the
    /// actor's spawned worker, not as a fabricated "intent recorded" toast.
    #[test]
    fn execute_emits_fetch_lnurl_invoice_with_zap_request() {
        let cmds = run_execute(well_formed_input()).expect("execute must succeed for well-formed input");
        assert_eq!(cmds.len(), 1, "executor must emit exactly one command, got {cmds:?}");
        match cmds.into_iter().next().unwrap() {
            ActorCommand::FetchLnurlInvoice {
                unsigned,
                lnurl_or_address,
                amount_msats,
                correlation_id,
            } => {
                assert_eq!(lnurl_or_address, LNURL);
                assert_eq!(amount_msats, 21_000);
                assert_eq!(correlation_id.as_deref(), Some("cid-deadbeef"));
                // kind:9734 zap-request — the builder must have produced
                // a NIP-57-shaped unsigned event with `relays`, `amount`,
                // and `p` tags.
                assert_eq!(unsigned.kind, 9734);
                let keys: Vec<&str> = unsigned
                    .tags
                    .iter()
                    .filter_map(|t| t.first())
                    .map(String::as_str)
                    .collect();
                assert!(keys.contains(&"relays"), "missing relays tag: {keys:?}");
                assert!(keys.contains(&"amount"), "missing amount tag: {keys:?}");
                assert!(keys.contains(&"p"), "missing p tag: {keys:?}");
                // The kernel re-stamps `created_at` from `now_secs()` —
                // the executor passes the D7 sentinel `0`.
                assert_eq!(
                    unsigned.created_at, 0,
                    "executor must pass created_at=0 sentinel; actor re-stamps"
                );
            }
            other => panic!("expected FetchLnurlInvoice, got {other:?}"),
        }
    }

    /// `e` tag must surface when `target_event_id` is set — a zap to a
    /// specific note vs. a zap to a profile.
    #[test]
    fn execute_includes_e_tag_when_target_event_id_set() {
        let input = ZapInput {
            target_event_id: Some(
                "aabb1122334455660011223344556677889900112233445566778899aabbccdd".into(),
            ),
            ..well_formed_input()
        };
        let cmds = run_execute(input).unwrap();
        let ActorCommand::FetchLnurlInvoice { unsigned, .. } =
            cmds.into_iter().next().expect("executor must emit a command")
        else {
            panic!("expected FetchLnurlInvoice");
        };
        let has_e = unsigned.tags.iter().any(|t| t.first().map(String::as_str) == Some("e"));
        assert!(has_e, "expected `e` tag for targeted zap: {:?}", unsigned.tags);
    }

    /// Comment lands in the kind:9734 `content` per NIP-57.
    #[test]
    fn execute_routes_comment_into_zap_request_content() {
        let input = ZapInput {
            comment: Some("nice post 🤙".to_string()),
            ..well_formed_input()
        };
        let cmds = run_execute(input).unwrap();
        let ActorCommand::FetchLnurlInvoice { unsigned, .. } =
            cmds.into_iter().next().expect("executor must emit a command")
        else {
            panic!("expected FetchLnurlInvoice");
        };
        assert_eq!(unsigned.content, "nice post 🤙");
    }
}
