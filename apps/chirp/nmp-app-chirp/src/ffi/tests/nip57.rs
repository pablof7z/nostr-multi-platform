//! NIP-57 zap dispatch proof.

use nmp_core::{nmp_app_free, nmp_app_new};

use super::super::{nmp_app_chirp_register, nmp_app_chirp_unregister};
use super::helpers::dispatch;

/// `nmp.nip57.zap` action — `ZapAction`, an `ActionModule` living in the
/// `nmp-nip57` protocol crate — is reachable through the generic
/// `dispatch_action` path. A well-formed `ZapInput` yields a 32-hex
/// `correlation_id` (both the typed module validator AND the executor are
/// wired); a malformed body is rejected with `error`.
///
/// This is the migration proof that ADR-0024's minimum-viable LNURL path
/// (no `HttpCapability` substrate) is live end-to-end: dispatch reaches
/// `ZapAction::execute`, which builds the unsigned kind:9734 and enqueues
/// `ActorCommand::Protocol(FetchLnurlInvoiceCommand{...})` (V-41) for the
/// actor's `Protocol(...)` arm to drive. The protocol command signs on
/// the actor thread and spawns a worker for the HTTP round-trip. The
/// test asserts only the dispatch half (correlation_id minted, executor
/// returned `Ok`); the HTTP round-trip itself requires a live LN provider
/// and is exercised end-to-end through the iOS shell.
#[test]
fn nip57_zap_dispatches_through_action_registry() {
    let app = nmp_app_new();
    let handle = nmp_app_chirp_register(app, std::ptr::null());
    assert!(!handle.is_null());

    let recipient = "bb11223344556677889900aabbccddeeff00112233445566778899aabbccddff";
    let body = format!(
        r#"{{"recipient_pubkey":"{recipient}","amount_msats":21000,"lnurl":"alice@walletofsatoshi.com","relays":["wss://relay.damus.io"]}}"#
    );
    let parsed = dispatch(app, "nmp.nip57.zap", &body);
    let id = parsed
        .get("correlation_id")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("expected correlation_id, got {parsed}"));
    assert_eq!(id.len(), 32, "correlation id should be 32 hex");

    // A zap to a profile (no target_event_id) is well-formed.
    let body_profile = format!(
        r#"{{"recipient_pubkey":"{recipient}","amount_msats":1000,"lnurl":"https://example.com/.well-known/lnurlp/bob","relays":["wss://relay.damus.io"]}}"#
    );
    let parsed = dispatch(app, "nmp.nip57.zap", &body_profile);
    assert!(
        parsed.get("correlation_id").is_some(),
        "profile-zap (no target) must dispatch cleanly: {parsed}"
    );

    // Zero amount is rejected by the typed validator (D6).
    let bad = format!(
        r#"{{"recipient_pubkey":"{recipient}","amount_msats":0,"lnurl":"alice@walletofsatoshi.com","relays":["wss://relay.damus.io"]}}"#
    );
    let parsed = dispatch(app, "nmp.nip57.zap", &bad);
    assert!(
        parsed.get("error").is_some(),
        "zero-amount zap must be rejected: {parsed}"
    );

    // Empty lnurl is rejected — NIP-57 has no destination without it.
    let no_lnurl = format!(
        r#"{{"recipient_pubkey":"{recipient}","amount_msats":21000,"lnurl":"","relays":["wss://relay.damus.io"]}}"#
    );
    let parsed = dispatch(app, "nmp.nip57.zap", &no_lnurl);
    assert!(
        parsed.get("error").is_some(),
        "empty-lnurl zap must be rejected: {parsed}"
    );

    nmp_app_chirp_unregister(handle);
    nmp_app_free(app);
}
