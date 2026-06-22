//! NIP-17 DM send dispatch proof + DM-inbox host registration tests.

use nmp_ffi::{nmp_app_free, nmp_app_new};

use super::super::{nmp_app_chirp_register_dm_inbox, nmp_app_chirp_unregister};
use super::helpers::{dispatch, register_app};

/// THE NIP-17 SEND-VERB PROOF: after `nmp_app_chirp_register`, the
/// `nmp.nip17.send` action — `SendDmAction`, an `ActionModule` living in the
/// `nmp-nip17` protocol crate — is reachable through the typed byte doorway
/// (ADR-0064 / Cut-B, #1756). A well-formed `SendDmInput` yields an echoed
/// host-supplied `correlation_id` (both the typed module validator AND the
/// executor are wired); a malformed / empty body is rejected with `error`.
#[test]
fn nip17_dm_send_dispatches_through_action_registry() {
    let app = nmp_app_new();
    let handle = register_app(app);

    let recipient = "bb11223344556677889900aabbccddeeff00112233445566778899aabbccddff";
    let body = format!(r#"{{"recipient_pubkey":"{recipient}","content":"hello over NIP-17"}}"#);
    let parsed = dispatch(app, "nmp.nip17.send", &body);
    let id = parsed
        .get("correlation_id")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("expected correlation_id, got {parsed}"));
    // ADR-0064 / Cut-B (#1756): the byte doorway echoes the host-supplied id.
    assert!(
        !id.is_empty(),
        "byte doorway must echo a non-empty correlation id"
    );

    // Empty content is rejected by the typed `SendDmAction::start`
    // validator surfaced through the host seam (D6).
    let parsed = dispatch(
        app,
        "nmp.nip17.send",
        &format!(r#"{{"recipient_pubkey":"{recipient}","content":"  "}}"#),
    );
    assert!(
        parsed.get("error").is_some(),
        "an empty-content DM must be rejected: {parsed}"
    );

    nmp_app_chirp_unregister(handle);
    nmp_app_free(app);
}

/// THE DM-INBOX WIRING PROOF: `nmp_app_chirp_register_dm_inbox` registers
/// a `DmInboxProjection` as an `IngestParser` for kind:1059 under slot
/// `"nip17.dm_inbox"` — it runs to completion (IngestParser slot registration
/// + snapshot-projection/controller registration) without panicking across
/// the FFI boundary. Active-account interest ownership is Rust-side — the
/// FFI takes no viewer pubkey.
///
/// Idempotency is provided by `replace_ingest_parser` (slot-keyed replace):
/// re-invoking with the same slot key evicts the prior parser and installs a
/// fresh one without stacking — no `swap_dm_inbox_observer` raw-tap slot
/// needed (that slot was deleted in the raw-tap PR-4 retirement).
#[test]
fn register_dm_inbox_runs_for_app() {
    let app = nmp_app_new();
    nmp_app_chirp_register_dm_inbox(app);
    nmp_app_free(app);
}

/// D6: a null `app` is a silent no-op — the function must never
/// dereference a null pointer or panic across the FFI boundary.
#[test]
fn register_dm_inbox_null_app_is_silent_noop() {
    nmp_app_chirp_register_dm_inbox(std::ptr::null_mut());
}

/// THE IDEMPOTENCY PROOF: re-invoking `nmp_app_chirp_register_dm_inbox`
/// must NOT stack observers on every call.
///
/// Since PR-4 (raw-tap retirement ladder), the DM inbox rides
/// `replace_ingest_parser` under slot `"nip17.dm_inbox"`. The slot-keyed
/// replace is inherently idempotent — a second call evicts the first parser
/// under the same key and installs a fresh one. This test confirms the
/// function is re-callable without panicking (the observable proof that no
/// stacking occurs is in the `replace_ingest_parser` unit tests in nmp-core).
#[test]
fn register_dm_inbox_is_idempotent_on_re_invoke() {
    let app = nmp_app_new();
    // First registration — installs the DmInboxProjection under "nip17.dm_inbox".
    nmp_app_chirp_register_dm_inbox(app);
    // Second registration — evicts the first via slot-keyed replace; no panic.
    nmp_app_chirp_register_dm_inbox(app);
    nmp_app_free(app);
}
