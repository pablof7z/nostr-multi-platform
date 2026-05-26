//! `nmp.nip57.zap` — the NIP-57 lightning zap [`ActionModule`].
//!
//! Validates a zap request in [`ZapAction::start`], builds an unsigned
//! kind:9734 via [`ZapRequest`] (`crate::build`), and dispatches
//! [`ActorCommand::Protocol`] carrying a
//! [`crate::lnurl::FetchLnurlInvoiceCommand`] (V-41 — the LNURL-pay
//! round-trip is now a `ProtocolCommand`; the legacy `FetchLnurlInvoice`
//! `ActorCommand` variant has been deleted along with the
//! `nmp-core::actor::commands::zap` module). The protocol command signs
//! the kind:9734 on the actor thread, fetches the receiver's LNURL
//! callback off-thread, and surfaces the resulting bolt11 invoice as a
//! `ShowToast` follow-up.
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
//! The protocol command reads
//! [`nmp_core::substrate::ProtocolCommandContext::active_local_keys`] to
//! sign the kind:9734. Bunker (NIP-46) accounts return `None`; the command
//! fails closed with a toast and records `ActionFailure` against the
//! `correlation_id`. Remote-signer signing is the ADR-0026 Phase 2
//! follow-up.

use nmp_core::substrate::{ActionContext, ActionModule, ActionRejection};
use nmp_core::ActorCommand;
use serde::{Deserialize, Serialize};

use crate::build::ZapRequest;
use crate::lnurl::FetchLnurlInvoiceCommand;

/// Wire shape for `nmp.nip57.zap` — the JSON body a host passes to
/// `nmp_app_dispatch_action`.
///
/// ```json
/// {
///   "recipient_pubkey": "<hex>",
///   "amount_msats": 21000,
///   "target_event_id": "<hex>",
///   "comment": "🤙"
/// }
/// ```
///
/// `lnurl` is optional. When omitted the kernel resolves the recipient's
/// lightning address from its cached kind:0 profile and fails with a clear
/// toast if none is found. Shells SHOULD omit `lnurl` — it is a protocol
/// detail the kernel owns, not the app. When provided (e.g. by the `:zap`
/// power-user command that lets the caller override the destination) it is
/// used verbatim.
///
/// `relays` is optional (`[]` or omitted) — the actor injects via the
/// substrate `RecipientRelayLookup` capability (kernel-side adapter
/// routes through `outbox_router` with a synthetic kind:9735 publish to
/// resolve the recipient's NIP-65 write set) before signing (V-07).
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
    /// bare https URL. When `None` the kernel resolves it from the
    /// recipient's cached kind:0 profile (`lud16` / `lud06`). Shells
    /// SHOULD omit this field; it is a protocol detail.
    #[serde(default)]
    pub lnurl: Option<String>,
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
/// [`ActorCommand::Protocol`] carrying a
/// [`FetchLnurlInvoiceCommand`] (V-41) — the protocol command handles
/// signing (D7 — kernel owns key access) and the off-thread LNURL-pay
/// HTTP round-trip (D8 — no blocking on the actor thread).
pub struct ZapAction;

impl ActionModule for ZapAction {
    const NAMESPACE: &'static str = "nmp.nip57.zap";
    type Action = ZapInput;

    /// Validate a zap request. Rejects:
    /// - empty `recipient_pubkey`
    /// - `amount_msats == 0`
    ///
    /// `lnurl` may be omitted — the kernel resolves it from the recipient's
    /// cached kind:0 profile at execute time. `relays` may be empty: the
    /// actor auto-selects from the recipient's kind:10002 (NIP-65) write
    /// list before signing (V-07). Relay choice is policy that lives in the
    /// kernel, not the shell.
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
        Ok(())
    }

    /// Settles asynchronously: `execute` enqueues
    /// `Protocol(FetchLnurlInvoiceCommand{...})` and returns immediately;
    /// the HTTP worker spawned in `FetchLnurlInvoiceCommand::run` surfaces
    /// the bolt11 (or failure) via `ShowToast` + `RecordActionSuccess` /
    /// `RecordActionFailure`. Recording sites: `lnurl::mod`
    /// (`Requested` via `ctx.record_action_stage_requested`; `Failed` on
    /// pre-payment errors).
    fn is_async_completing() -> bool { // doctrine-allow: D12 — recording sites are cross-file (crate::lnurl records Requested via ProtocolCommandContext and Failed on pre-payment errors)
        true
    }

    /// Build the unsigned kind:9734 and enqueue an
    /// [`ActorCommand::Protocol`] carrying a
    /// [`FetchLnurlInvoiceCommand`] (V-41).
    ///
    /// # D7 — kernel owns the wall clock
    ///
    /// `created_at` is passed as `0`; the protocol command re-stamps from
    /// `ProtocolCommandContext::now_secs` before signing. Matches the
    /// `PublishUnsignedEventToRelays` precedent.
    ///
    /// # D8 — no blocking
    ///
    /// The closure neither HTTPs nor signs; the LNURL command's `run`
    /// does both: the kind:9734 signature on the actor thread (D7), the
    /// LNURL-pay HTTP round-trip on a spawned `std::thread::spawn`
    /// worker (D8).
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
        // the protocol command overrides this when it builds the signed
        // event. Pass an empty placeholder; the substrate `UnsignedEvent`
        // carries it through unchanged but `sign_zap_request` re-signs
        // from the active `Keys` (its pubkey is what `EventBuilder` stamps).
        // `created_at = 0` is the D7 sentinel — re-stamped in `run()`.
        let unsigned = builder
            .build(String::new(), 0)
            .map_err(|e| format!("build kind:9734 zap request: {e}"))?;
        send(ActorCommand::Protocol(Box::new(FetchLnurlInvoiceCommand {
            unsigned,
            recipient_pubkey: action.recipient_pubkey,
            lnurl_or_address: action.lnurl,
            amount_msats: action.amount_msats,
            correlation_id: Some(correlation_id.to_string()),
        })));
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
            lnurl: Some(LNURL.to_string()),
            relays: vec![RELAY.to_string()],
            target_event_id: None,
            comment: None,
        }
    }

    fn well_formed_input_no_lnurl() -> ZapInput {
        ZapInput {
            recipient_pubkey: RECIPIENT.to_string(),
            amount_msats: 21_000,
            lnurl: None,
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
    fn start_accepts_no_lnurl_kernel_resolves() {
        // Shells that know only the pubkey and amount pass `lnurl: None`.
        // The kernel resolves the address from the cached kind:0 profile at
        // execute time — `start` must not reject it.
        assert!(ZapAction::start(&mut ctx(), well_formed_input_no_lnurl()).is_ok());
    }

    /// V-07: empty relays is VALID — the actor injects the recipient's
    /// NIP-65 write list before signing. The executor still emits
    /// `Protocol(FetchLnurlInvoiceCommand{...})`; the resulting kind:9734
    /// has no `relays` tag at this point (`FetchLnurlInvoiceCommand::run`
    /// adds it via `ProtocolCommandContext::recipient_publish_relays`).
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

    /// The executor must emit a `Protocol(FetchLnurlInvoiceCommand)`
    /// carrying the full validated zap intent — NOT the previous
    /// `FetchLnurlInvoice` closed-enum variant. V-41 contract: LNURL
    /// fetch routes through the open `ProtocolCommand` seam; `nmp-core`
    /// has no zap nouns.

    #[test]
    fn execute_emits_protocol_lnurl_command_with_zap_request() {
        let cmds = run_execute(well_formed_input()).expect("execute must succeed for well-formed input");
        assert_eq!(cmds.len(), 1, "executor must emit exactly one command, got {cmds:?}");
        let cmd = cmds.into_iter().next().unwrap();
        let ActorCommand::Protocol(boxed) = cmd else {
            panic!("expected ActorCommand::Protocol(...), got something else");
        };
        // Debug-format the boxed protocol command and assert the LNURL
        // command type appears — the trait object hides the concrete
        // type, but Debug derive on FetchLnurlInvoiceCommand surfaces
        // the struct name + fields.
        let dbg = format!("{boxed:?}");
        assert!(
            dbg.contains("FetchLnurlInvoiceCommand"),
            "expected FetchLnurlInvoiceCommand, got: {dbg}"
        );
        assert!(dbg.contains(LNURL), "lnurl must surface in command Debug: {dbg}");
        assert!(dbg.contains("21000"), "amount must surface: {dbg}");
        assert!(dbg.contains("cid-deadbeef"), "correlation_id must surface: {dbg}");
        // kind:9734 + builder tags surface through the embedded
        // UnsignedEvent's Debug.
        assert!(dbg.contains("kind: 9734"), "kind 9734 must surface: {dbg}");
        assert!(dbg.contains("\"relays\""), "relays tag key must surface: {dbg}");
        assert!(dbg.contains("\"amount\""), "amount tag key must surface: {dbg}");
        assert!(dbg.contains("\"p\""), "p tag key must surface: {dbg}");
        // The D7 sentinel: executor must pass created_at=0 (the protocol
        // command re-stamps from `ctx.now_secs()` in its `run`).
        assert!(dbg.contains("created_at: 0"), "created_at sentinel: {dbg}");
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
        let ActorCommand::Protocol(boxed) =
            cmds.into_iter().next().expect("executor must emit a command")
        else {
            panic!("expected ActorCommand::Protocol(...)");
        };
        let dbg = format!("{boxed:?}");
        assert!(dbg.contains("\"e\""), "expected `e` tag for targeted zap: {dbg}");
    }

    /// Comment lands in the kind:9734 `content` per NIP-57.

    #[test]
    fn execute_routes_comment_into_zap_request_content() {
        let input = ZapInput {
            comment: Some("nice post 🤙".to_string()),
            ..well_formed_input()
        };
        let cmds = run_execute(input).unwrap();
        let ActorCommand::Protocol(boxed) =
            cmds.into_iter().next().expect("executor must emit a command")
        else {
            panic!("expected ActorCommand::Protocol(...)");
        };
        let dbg = format!("{boxed:?}");
        assert!(dbg.contains("nice post"), "expected comment content in: {dbg}");
    }
}
