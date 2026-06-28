//! Tests for [`super`] — the `nmp.nip57.zap` [`ActionModule`].
//!
//! Split out of `action.rs` to keep that file under the hard LOC cap; wired
//! back in via `#[path = "action_tests.rs"] mod tests;` (the same pattern
//! `decode.rs` → `decode_tests.rs` uses). `use super::*` resolves to the
//! `action` module, so the tests reach `ZapAction` / `ZapInput` unchanged.

use super::*;
use nmp_core::substrate::{ActionContext, PaymentIntent, PaymentPort};
use std::cell::RefCell;
use std::sync::Arc;

/// A no-op [`PaymentPort`] test double. These tests only assert the emitted
/// `FetchLnurlInvoiceCommand` shape (the port is captured, never invoked here),
/// so a stub is enough to exercise the `Some(_)` path. The real adapter
/// (`nmp_nip47::WalletPaymentPort`) lives in `nmp-nip47`, which NIP-57 no
/// longer depends on (#1728).
#[derive(Debug)]
struct StubPaymentPort;

impl PaymentPort for StubPaymentPort {
    fn pay_invoice(&self, intent: PaymentIntent) -> ActorCommand {
        // Test stub: return an error token (these tests assert on the emitted
        // FetchLnurlInvoiceCommand, not on the payment command).
        ActorCommand::ShowErrorToken {
            token: nmp_core::ui_token::UiToken::error(
                "stub_payment_intent",
                format!("stub: pay {}", intent.bolt11),
            ),
        }
    }
}

/// A `ZapAction` bound to a stub payment port for unit tests (ADR-0052 rung 5.2
/// — no process-global). These tests only assert the emitted
/// `FetchLnurlInvoiceCommand` shape, so the stub exercises the `Some(_)` path.
fn zap_action() -> ZapAction {
    ZapAction::with_payment_port(Arc::new(StubPaymentPort) as Arc<dyn PaymentPort>)
}

/// Run the typed executor and capture every `ActorCommand` it sends, in order.
fn run_execute(input: ZapInput) -> Result<Vec<ActorCommand>, String> {
    let captured: RefCell<Vec<ActorCommand>> = RefCell::new(Vec::new());
    zap_action().execute(
        &nmp_core::substrate::ActionContext::default(),
        input,
        "cid-deadbeef",
        &|cmd| {
            captured.borrow_mut().push(cmd);
        },
    )?;
    Ok(captured.into_inner())
}

const RECIPIENT: &str = "bb11223344556677889900aabbccddeeff00112233445566778899aabbccddff";
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
    assert!(zap_action().start(&mut ctx(), well_formed_input()).is_ok());
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
    assert!(zap_action().start(&mut ctx(), input).is_ok());
}

#[test]
fn start_rejects_empty_recipient() {
    let input = ZapInput {
        recipient_pubkey: "   ".to_string(),
        ..well_formed_input()
    };
    assert!(matches!(
        zap_action().start(&mut ctx(), input),
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
        zap_action().start(&mut ctx(), input),
        Err(ActionRejection::Invalid(_))
    ));
}

#[test]
fn start_accepts_no_lnurl_kernel_resolves() {
    // Shells that know only the pubkey and amount pass `lnurl: None`.
    // The kernel resolves the address from the cached kind:0 profile at
    // execute time — `start` must not reject it.
    assert!(
        zap_action()
            .start(&mut ctx(), well_formed_input_no_lnurl())
            .is_ok()
    );
}

#[test]
fn start_rejects_empty_lnurl_when_provided() {
    let input = ZapInput {
        lnurl: Some("   ".to_string()),
        ..well_formed_input()
    };
    assert!(matches!(
        zap_action().start(&mut ctx(), input),
        Err(ActionRejection::Invalid(_))
    ));
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
    assert!(zap_action().start(&mut ctx(), input).is_ok());
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
    assert!(zap_action().start(&mut ctx(), input).is_ok());
}

/// The executor must emit a `Protocol(FetchLnurlInvoiceCommand)`
/// carrying the full validated zap intent — NOT the previous
/// `FetchLnurlInvoice` closed-enum variant. V-41 contract: LNURL
/// fetch routes through the open `ProtocolCommand` seam; `nmp-core`
/// has no zap nouns.
#[test]
fn execute_emits_protocol_lnurl_command_with_zap_request() {
    let cmds =
        run_execute(well_formed_input()).expect("execute must succeed for well-formed input");
    assert_eq!(
        cmds.len(),
        1,
        "executor must emit exactly one command, got {cmds:?}"
    );
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
    assert!(
        dbg.contains(LNURL),
        "lnurl must surface in command Debug: {dbg}"
    );
    assert!(dbg.contains("21000"), "amount must surface: {dbg}");
    assert!(
        dbg.contains("cid-deadbeef"),
        "correlation_id must surface: {dbg}"
    );
    // kind:9734 + builder tags surface through the embedded
    // UnsignedEvent's Debug.
    assert!(dbg.contains("kind: 9734"), "kind 9734 must surface: {dbg}");
    assert!(
        dbg.contains("\"relays\""),
        "relays tag key must surface: {dbg}"
    );
    assert!(
        dbg.contains("\"amount\""),
        "amount tag key must surface: {dbg}"
    );
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
    let ActorCommand::Protocol(boxed) = cmds
        .into_iter()
        .next()
        .expect("executor must emit a command")
    else {
        panic!("expected ActorCommand::Protocol(...)");
    };
    let dbg = format!("{boxed:?}");
    assert!(
        dbg.contains("\"e\""),
        "expected `e` tag for targeted zap: {dbg}"
    );
}

/// Comment lands in the kind:9734 `content` per NIP-57.
#[test]
fn execute_routes_comment_into_zap_request_content() {
    let input = ZapInput {
        comment: Some("nice post 🤙".to_string()),
        ..well_formed_input()
    };
    let cmds = run_execute(input).unwrap();
    let ActorCommand::Protocol(boxed) = cmds
        .into_iter()
        .next()
        .expect("executor must emit a command")
    else {
        panic!("expected ActorCommand::Protocol(...)");
    };
    let dbg = format!("{boxed:?}");
    assert!(
        dbg.contains("nice post"),
        "expected comment content in: {dbg}"
    );
}

#[test]
fn execute_records_failure_when_zap_request_build_fails() {
    let input = ZapInput {
        recipient_pubkey: String::new(),
        ..well_formed_input()
    };
    let cmds = run_execute(input).expect("build failure should settle through action failure");
    assert_eq!(cmds.len(), 1, "expected one terminal command, got {cmds:?}");
    let ActorCommand::ActionLedger(ActionLedgerCommand::RecordFailure {
        correlation_id,
        reason,
    }) = &cmds[0]
    else {
        panic!("expected RecordActionFailure, got {:?}", cmds[0]);
    };
    assert_eq!(correlation_id, "cid-deadbeef");
    assert!(
        reason.contains("build kind:9734 zap request"),
        "failure should explain build error: {reason}"
    );
}
