#![cfg(test)]
//! Tests for signer registration, handshake progress, sign routing, and
//! the `ctx.active_account_pubkey()` accessor.

use std::sync::atomic::Ordering;

use super::{super::*, fresh, stub_signer};
use crate::actor::commands::identity::sign_active_nonblocking;
use crate::remote_signer::RemoteSignerHandle;
use crate::substrate::UnsignedEvent;

// ──────────────────────────────────────────────────────────────────────────
// Command-handler tests (the dispatch arms forward straight into these).
// ──────────────────────────────────────────────────────────────────────────

#[test]
fn add_remote_signer_projects_nip46_account_summary() {
    let (mut id, mut kernel) = fresh();
    let (handle, _count) = stub_signer();
    let expected_pk = handle.pubkey_hex();
    add_signer(
        &mut id,
        &mut kernel,
        crate::actor::SignerSource::RemoteHandle(handle),
        false,
        false,
    );

    let (accounts, active) = kernel.account_snapshot();
    assert!(
        accounts.iter().any(|a| a.signer_kind == "nip46"),
        "expected a nip46 account row, got {accounts:?}"
    );
    let row = accounts
        .iter()
        .find(|a| a.id == expected_pk)
        .expect("row by pubkey hex");
    assert_eq!(row.signer_kind, "nip46");
    assert_eq!(row.status, "active");
    assert!(row.npub.starts_with("npub1"));
    assert_eq!(active, Some(&expected_pk));
    // aim.md §2 #4 / §4.5: pre-classified semantic flags the UI binds directly.
    // The human-readable signer label is derived shell-side from `signer_kind`
    // (#1712, D7/D27 — no pre-rendered label on the wire).
    assert!(
        row.signer_is_remote,
        "nip46 row must be flagged as a remote signer"
    );
    assert!(row.is_active, "first remote signer becomes active");
}

#[test]
fn bunker_handshake_progress_writes_then_clears() {
    let (id, mut kernel) = fresh();
    bunker_handshake_progress(
        &id,
        &mut kernel,
        "awaiting_pubkey".to_string(),
        None,
        Some("connected, waiting for get_public_key".to_string()),
    );
    // D0: handshake state is an app noun — it is written to the identity
    // runtime's shared slot (read by the `"bunker_handshake"` projection),
    // not a typed kernel field.
    let progress = id.bunker_handshake_for_test().expect("set");
    assert_eq!(progress.stage, "awaiting_pubkey");
    assert!(progress.message.is_some());

    // `"idle"` collapses to `None`.
    bunker_handshake_progress(&id, &mut kernel, "idle".to_string(), None, None);
    assert!(id.bunker_handshake_for_test().is_none());
}

/// Pins the doctrine §6 anti-pattern #1 fix: `BunkerHandshakeDto` carries
/// pre-computed boolean flags so `AccountsView.swift` branches on a single flag
/// instead of switching on the raw `stage` string. Per #1493 P9 (labels-to-
/// shells) the English `stage_label` is no longer on the wire — the shell
/// derives it from the raw `stage` token (asserted here) — so this test pins the
/// flag transitions plus the stable `stage` token the shell renders from.
#[test]
fn bunker_handshake_dto_pre_computes_view_flags_and_stage() {
    let (id, mut kernel) = fresh();

    // ── `"connecting"` — handshake in flight ──────────────────────────────
    bunker_handshake_progress(
        &id,
        &mut kernel,
        "connecting".to_string(),
        None,
        Some("dialing wss://r.example".to_string()),
    );
    let dto = id.bunker_handshake_for_test().expect("connecting set");
    assert!(!dto.is_idle, "connecting is not idle");
    assert!(dto.is_in_flight, "connecting is in flight");
    assert!(!dto.is_failed, "connecting has not failed");
    assert!(!dto.is_terminal_success, "connecting is not terminal");
    assert!(dto.can_cancel, "cancel is available while connecting");
    assert_eq!(dto.stage, "connecting");

    // ── `"awaiting_pubkey"` — also in flight ──────────────────────────────
    bunker_handshake_progress(&id, &mut kernel, "awaiting_pubkey".to_string(), None, None);
    let dto = id.bunker_handshake_for_test().expect("awaiting set");
    assert!(!dto.is_idle);
    assert!(dto.is_in_flight, "awaiting_pubkey is in flight");
    assert!(!dto.is_failed);
    assert!(!dto.is_terminal_success);
    assert!(dto.can_cancel, "cancel still available awaiting pubkey");
    assert_eq!(dto.stage, "awaiting_pubkey");

    // ── `"ready"` — terminal success ──────────────────────────────────────
    bunker_handshake_progress(&id, &mut kernel, "ready".to_string(), None, None);
    let dto = id.bunker_handshake_for_test().expect("ready set");
    assert!(!dto.is_idle);
    assert!(!dto.is_in_flight, "ready is not in flight");
    assert!(!dto.is_failed);
    assert!(
        dto.is_terminal_success,
        "ready is the terminal-success flag"
    );
    assert!(!dto.can_cancel, "no cancel once terminal");
    assert_eq!(dto.stage, "ready");

    // ── `"failed"` — terminal failure ─────────────────────────────────────
    bunker_handshake_progress(
        &id,
        &mut kernel,
        "failed".to_string(),
        None,
        Some("relay handshake failed".to_string()),
    );
    let dto = id.bunker_handshake_for_test().expect("failed set");
    assert!(!dto.is_idle);
    assert!(!dto.is_in_flight, "failed is not in flight");
    assert!(dto.is_failed, "failed flag tracks terminal failure");
    assert!(!dto.is_terminal_success);
    assert!(!dto.can_cancel, "no cancel once terminal");
    assert_eq!(dto.stage, "failed");
}

#[test]
fn sign_active_nonblocking_routes_through_remote_signer_when_active() {
    let (mut id, mut kernel) = fresh();
    let (handle, count) = stub_signer();
    let expected_pk = handle.pubkey_hex();
    add_signer(
        &mut id,
        &mut kernel,
        crate::actor::SignerSource::RemoteHandle(handle),
        false,
        false,
    );
    assert_eq!(count.load(Ordering::Relaxed), 0);

    // Drive a publish through the actor path: it must call
    // `sign_active_nonblocking`, which the stub records via `sign_count`.
    let unsigned = UnsignedEvent {
        pubkey: "ignored-by-signer".into(),
        kind: 1,
        tags: Vec::new(),
        content: "stage-3 hello".into(),
        created_at: 1_700_000_000,
    };
    let signed = sign_active_nonblocking(&id, &unsigned)
        .expect("sign_active_nonblocking ok")
        .poll()
        .expect("stub signer resolves Ready immediately")
        .expect("stub sign ok");
    assert_eq!(count.load(Ordering::Relaxed), 1);
    assert_eq!(signed.unsigned.pubkey, expected_pk);
    assert_eq!(signed.unsigned.kind, 1);
    assert_eq!(signed.unsigned.content, "stage-3 hello");
}

#[test]
fn publish_unsigned_event_with_active_remote_uses_stub_signer() {
    // End-to-end: AddRemoteSigner → PublishUnsignedEvent goes through the
    // stub. Mirrors `publish_unsigned_event_signs_and_publishes_arbitrary_kind`
    // from `commands::tests` but with a remote handle behind the active slot.
    //
    // T-publish-resolver-indexer (codex f81f735): seed kind:10002 for the
    // remote signer's pubkey so the resolver has NIP-65 write relays.
    let (mut id, mut kernel) = fresh();
    let (handle, count) = stub_signer();
    let expected_pk = handle.pubkey_hex();
    add_signer(
        &mut id,
        &mut kernel,
        crate::actor::SignerSource::RemoteHandle(handle),
        false,
        false,
    );
    // Seed kind:10002 so the fail-closed resolver finds write relays.
    kernel.seed_kind10002_for_test(
        &expected_pk,
        &["wss://remote-write-r1.test", "wss://remote-write-r2.test"],
    );

    let unsigned = UnsignedEvent {
        pubkey: "ignored-by-signer".into(),
        kind: 30023,
        tags: vec![vec!["d".into(), "stage-3-article".into()]],
        content: "# hello bunker".into(),
        created_at: 1_700_000_000,
    };
    let outbound = publish_unsigned_event(
        &id,
        &mut kernel,
        unsigned,
        None,
        None,
        &mut crate::actor::pending_sign::ParkedSignerOps::new(),
    );
    assert_eq!(
        count.load(Ordering::Relaxed),
        1,
        "remote signer was invoked"
    );
    assert!(!outbound.is_empty(), "publish produced outbound frames");
    assert!(outbound[0].text.contains("\"kind\":30023"));
    assert!(outbound[0]
        .text
        .contains(&format!("\"pubkey\":\"{expected_pk}\"")));
    let q = kernel.publish_queue_snapshot();
    assert_eq!(q.last().unwrap().status, "accepted_locally");
}

#[test]
fn ctx_active_account_pubkey_resolves_the_bunker_pubkey() {
    // ADR-0050 §D5 — the gift-wrap DM chain pins its originating account by
    // resolving `ctx.active_account_pubkey()` ONCE at step 1, then passing
    // `signer_pubkey: Some(hex)` to every port step (so a mid-chain account
    // switch signs the seal with the originating account). This replaces the
    // deleted `signer_for_seal` slot + `RemoteSignerForSeal` adapter.
    //
    // The accessor must be backend-transparent: with an active NIP-46 bunker
    // (no local keys), it resolves the BUNKER's user pubkey — not `None`, and
    // not a phantom local-keys branch. `active_local_keys()` stays `None` for a
    // bunker (D13 — the chain never holds raw keys; it signs through the port).
    use crate::substrate::{
        EmptyDmInboxRelayLookup, LocalSignerAccess, NoopActionStageTracker, NoopErrorSurface,
        NoopHostOpHandlerAccess, NoopKernelClock, NoopRecipientRelayLookup, ProtocolCommandContext,
        ProtocolCommandContextParts,
    };

    let (mut id, mut kernel) = fresh();
    let (handle, _sign_count) = stub_signer();
    let bunker_hex = handle.pubkey_hex();
    add_signer(
        &mut id,
        &mut kernel,
        crate::actor::SignerSource::RemoteHandle(handle),
        true, // make_active — this is the active account.
        false,
    );

    // Debt C — wrap the identity reference in a `LocalSignerAccess` adapter so
    // the test exercises the same capability surface the dispatch arm wires.
    struct IdentityLocalSignerAccess<'a>(&'a crate::actor::commands::identity::IdentityRuntime);
    impl<'a> LocalSignerAccess for IdentityLocalSignerAccess<'a> {
        fn active_local_keys(&self) -> Option<nostr::Keys> {
            self.0.active_local_keys().cloned()
        }
        fn active_account_pubkey(&self) -> Option<String> {
            self.0.active_pubkey()
        }
    }
    // SAFETY: single-threaded test scope; the `&IdentityRuntime` borrow never
    // crosses a thread boundary. The trait carries the bound.
    unsafe impl<'a> Send for IdentityLocalSignerAccess<'a> {}
    unsafe impl<'a> Sync for IdentityLocalSignerAccess<'a> {}
    let signers = IdentityLocalSignerAccess(&id);
    static CLOCK: NoopKernelClock = NoopKernelClock;
    static DMS: EmptyDmInboxRelayLookup = EmptyDmInboxRelayLookup;
    static ERRORS: NoopErrorSurface = NoopErrorSurface;
    static STAGES: NoopActionStageTracker = NoopActionStageTracker;
    static RECIPIENTS: NoopRecipientRelayLookup = NoopRecipientRelayLookup;
    static HOST_OP: NoopHostOpHandlerAccess = NoopHostOpHandlerAccess;
    static WALLET: crate::substrate::NoopWalletKernelAccess =
        crate::substrate::NoopWalletKernelAccess;
    static ZAP: crate::substrate::NoopZapProfileLookup = crate::substrate::NoopZapProfileLookup;
    let send = |_: crate::actor::ActorCommand| {};
    let (tx, _rx) = std::sync::mpsc::channel::<crate::actor::ActorMail>();
    let ctx = ProtocolCommandContext::new(ProtocolCommandContextParts {
        send: &send,
        command_sender: crate::actor::CommandSender::new(tx),
        clock: &CLOCK,
        signers: &signers,
        dms: &DMS,
        errors: &ERRORS,
        stages: &STAGES,
        recipients: &RECIPIENTS,
        host_op_handler: &HOST_OP,
        wallet_kernel: &WALLET,
        zap_profiles: &ZAP,
    });

    // Backend-transparent: the active bunker's pubkey resolves through the
    // accessor — the pin source for the whole chain.
    assert_eq!(
        ctx.active_account_pubkey().as_deref(),
        Some(bunker_hex.as_str()),
        "active_account_pubkey must resolve the active bunker's user pubkey"
    );
    // A bunker exposes NO local keys — the chain signs the seal through the
    // port (`SignEventForAccount`), never by holding raw `Keys` (D13).
    assert!(
        ctx.active_local_keys().is_none(),
        "a NIP-46 bunker account must not expose local keys"
    );
}
