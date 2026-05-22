//! `ZapAction` — INTENTIONALLY NOT REGISTERED in production.
//! The executor stubs with ShowToast (no real HTTP/LNURL round-trip).
//! Re-register when ADR-0024 (HttpCapability) lands and the executor
//! can actually complete payment. See: pending_zaps queue design.
//!
//! `nmp.nip57.zap` — the NIP-57 lightning zap [`ActionModule`].
//!
//! # What this PR does
//!
//! Wires the `nmp.nip57.zap` action namespace into the kernel's generic
//! `dispatch_action` seam so a host (Chirp, or any future NMP host) can express
//! a zap intent through the single-door action path without adding any NIP-57
//! nouns to `nmp-core` (D0).
//!
//! [`ZapAction`] is a pure **validator + intent recorder**. Its `start` method
//! rejects obviously-malformed inputs (missing recipient, zero amount, no
//! relays). Its `execute` method records the validated zap intent as a
//! `ShowToast` so it is observable in the snapshot — this is an intentional,
//! documented stub while the LNURL HTTP fetch infrastructure (ADR-0024
//! `HttpCapability`) is built.
//!
//! # What this PR does NOT do (explicit scope boundary)
//!
//! * It does NOT call any HTTP endpoint (LNURL fetch, bolt11 decode, LN pay).
//!   D8 — the actor thread is single-actor; blocking it drops all incoming
//!   events. The LNURL fetch path requires `HttpCapability` (ADR-0024), which
//!   is not yet implemented.
//! * It does NOT publish the kind:9734 zap request to relays. Zap requests are
//!   sent to the **LNURL endpoint** over HTTP, not broadcast to Nostr relays.
//!   Publishing kind:9734 to relays would be semantically wrong per NIP-57.
//!
//! # Upgrade path
//!
//! Once `HttpCapability` (ADR-0024) lands:
//!
//! 1. Add an `ActorCommand::InitiateLnurlZap { ... }` variant that carries the
//!    validated zap parameters.
//! 2. Replace the `ShowToast` stub in `execute` with that `ActorCommand`.
//! 3. The actor's LNURL handler: fetches the recipient's LN address or
//!    kind:0/9734-compatible LNURL, calls the LNURL `callback` URL, receives
//!    the bolt11 invoice, and routes the payment through `ActorCommand::WalletPayInvoice`
//!    (NIP-47 NWC) or a future LN wallet capability.
//!
//! # Namespace
//!
//! `nmp.nip57.zap` — consistent with the existing `nmp.nip57.zaps` domain
//! namespace (`domain.rs`).

use nmp_core::substrate::{ActionContext, ActionModule, ActionRejection};
use nmp_core::ActorCommand;
use serde::{Deserialize, Serialize};

/// Wire shape for `nmp.nip57.zap` — the JSON body a host passes to
/// `nmp_app_dispatch_action`.
///
/// ```json
/// {
///   "recipient_pubkey": "<hex>",
///   "amount_msats": 21000,
///   "relays": ["wss://relay.damus.io"],
///   "target_event_id": "<hex>",
///   "comment": "🤙"
/// }
/// ```
///
/// `target_event_id` and `comment` are optional. A zap to a profile (no
/// target event) omits `target_event_id`. `relays` must have at least one
/// entry: NIP-57 requires a `relays` tag so the recipient knows where to
/// look for the kind:9735 receipt.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ZapInput {
    /// Recipient's Nostr public key, lowercase hex.
    pub recipient_pubkey: String,
    /// Amount in millisatoshis. Must be > 0.
    pub amount_msats: u64,
    /// Relay URLs included in the kind:9734 `relays` tag. At least one required
    /// per NIP-57.
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
/// `start` validates the zap input. `execute` records the intent as a
/// `ShowToast` stub — the LNURL HTTP fetch and bolt11 payment are deferred
/// to `HttpCapability` (ADR-0024). See module-level docs for the upgrade path.
pub struct ZapAction;

impl ActionModule for ZapAction {
    const NAMESPACE: &'static str = "nmp.nip57.zap";
    type Action = ZapInput;

    /// Validate a zap request. Rejects:
    /// - empty `recipient_pubkey`
    /// - `amount_msats == 0`
    /// - empty `relays` list (NIP-57 requires at least one relay for receipt
    ///   discovery; after filtering whitespace-only entries)
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
        let non_empty_relays: Vec<_> = action
            .relays
            .iter()
            .filter(|r| !r.trim().is_empty())
            .collect();
        if non_empty_relays.is_empty() {
            return Err(ActionRejection::Invalid(
                "NIP-57 zap requires at least one relay for receipt discovery".into(),
            ));
        }
        Ok(())
    }

    /// Record the validated zap intent.
    ///
    /// # D8 — no sync I/O
    ///
    /// This executor MUST NOT call any sync HTTP (LNURL fetch, bolt11 pay).
    /// The actor thread is single-actor; blocking it drops all incoming events.
    ///
    /// # Current behaviour (stub)
    ///
    /// Emits a `ShowToast` carrying the zap parameters so the intent is
    /// observable in the snapshot. This is intentionally a stub — the LNURL
    /// HTTP fetch path requires `HttpCapability` (ADR-0024), which is not yet
    /// implemented. Replace with `ActorCommand::InitiateLnurlZap { ... }` when
    /// that infrastructure lands.
    fn execute(
        action: Self::Action,
        _correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        // TODO(ADR-0024): replace with ActorCommand::InitiateLnurlZap once
        // HttpCapability is implemented. The LNURL fetch (recipient's LN address
        // → callback URL → bolt11 invoice) must happen off the actor thread.
        let sats = action.amount_msats / 1000;
        let msats_rem = action.amount_msats % 1000;
        let amount_str = if msats_rem == 0 {
            format!("{sats} sats")
        } else {
            format!("{} msats", action.amount_msats)
        };
        let target_str = action
            .target_event_id
            .as_deref()
            .map(|id| format!(" on note {}", &id[..id.len().min(8)]))
            .unwrap_or_default();
        let message = format!(
            "Zap intent recorded: {} for {}{} — LNURL HTTP fetch not yet wired (ADR-0024)",
            amount_str,
            &action.recipient_pubkey[..action.recipient_pubkey.len().min(8)],
            target_str,
        );
        send(ActorCommand::ShowToast { message });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_core::substrate::ActionContext;
    use std::cell::RefCell;

    const RECIPIENT: &str =
        "bb11223344556677889900aabbccddeeff00112233445566778899aabbccddff";
    const RELAY: &str = "wss://relay.damus.io";

    fn ctx() -> ActionContext {
        ActionContext::default()
    }

    fn well_formed_input() -> ZapInput {
        ZapInput {
            recipient_pubkey: RECIPIENT.to_string(),
            amount_msats: 21_000,
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
    fn start_rejects_empty_relays() {
        let input = ZapInput {
            relays: vec![],
            ..well_formed_input()
        };
        assert!(matches!(
            ZapAction::start(&mut ctx(), input),
            Err(ActionRejection::Invalid(_))
        ));
    }

    #[test]
    fn start_rejects_whitespace_only_relays() {
        let input = ZapInput {
            relays: vec!["   ".to_string(), "\t".to_string()],
            ..well_formed_input()
        };
        assert!(matches!(
            ZapAction::start(&mut ctx(), input),
            Err(ActionRejection::Invalid(_))
        ));
    }

    #[test]
    fn execute_emits_show_toast_stub() {
        let captured: RefCell<Option<ActorCommand>> = RefCell::new(None);
        ZapAction::execute(well_formed_input(), "test-cid", &|cmd| {
            *captured.borrow_mut() = Some(cmd);
        })
        .expect("execute must succeed for well-formed input");
        match captured.into_inner().expect("executor must emit a command") {
            ActorCommand::ShowToast { message } => {
                assert!(
                    message.contains("ADR-0024"),
                    "toast must document the ADR-0024 gap, got: {message}"
                );
                assert!(
                    message.contains("21 sats"),
                    "toast must include the amount, got: {message}"
                );
            }
            other => panic!("expected ShowToast, got {other:?}"),
        }
    }

    #[test]
    fn execute_formats_msats_when_not_whole_sats() {
        let input = ZapInput {
            amount_msats: 1_500,
            ..well_formed_input()
        };
        let captured: RefCell<Option<ActorCommand>> = RefCell::new(None);
        ZapAction::execute(input, "test-cid", &|cmd| {
            *captured.borrow_mut() = Some(cmd);
        })
        .unwrap();
        match captured.into_inner().unwrap() {
            ActorCommand::ShowToast { message } => {
                assert!(message.contains("1500 msats"), "got: {message}");
            }
            other => panic!("expected ShowToast, got {other:?}"),
        }
    }
}
