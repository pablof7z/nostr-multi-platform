//! Stage 3 of NIP-46 wiring: actor-side `RemoteSignerHandle` plumbing.
//!
//! These tests drive the new command handlers + dispatch arms with a stub
//! `RemoteSignerHandle` impl — Stage 4 (broker) ships real NIP-46 transport,
//! but the actor MUST treat the trait as a first-class signer regardless of
//! the impl behind it. D0 stays clean: the stub lives in `nmp-core`'s test
//! tree, NOT in `nmp-signers`.

use std::sync::{
    atomic::{AtomicU32, Ordering},
    Arc,
};

use nmp_signer_iface::SignerOp;
use nostr::nips::nip19::FromBech32;
use nostr::{EventBuilder, Keys, SecretKey, Timestamp};

use super::*;
use crate::actor::commands::identity::{sign_active_nonblocking, IdentityRuntime};
use crate::kernel::Kernel;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::remote_signer::RemoteSignerHandle;
use crate::substrate::{SignedEvent, UnsignedEvent};

/// nsec from `commands::tests` — known-good test key.
const TEST_NSEC: &str = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";

/// Stub `RemoteSignerHandle` for Stage 3 plumbing tests. Holds a `Keys` and
/// signs synchronously via `SignerOp::ok(...)`. Production NIP-46 signers
/// live in `nmp-signers`; D0 still forbids that import here so we cannot
/// reach for the real impl — a stub is the correct shape for actor-side
/// plumbing tests.
#[derive(Debug)]
struct StubRemoteSigner {
    keys: Keys,
    pk: String,
    sign_count: Arc<AtomicU32>,
}

impl StubRemoteSigner {
    fn new(keys: Keys) -> Self {
        let pk = keys.public_key().to_hex();
        Self {
            keys,
            pk,
            sign_count: Arc::new(AtomicU32::new(0)),
        }
    }

    fn sign_count_handle(&self) -> Arc<AtomicU32> {
        Arc::clone(&self.sign_count)
    }
}

impl RemoteSignerHandle for StubRemoteSigner {
    fn pubkey_hex(&self) -> String {
        self.pk.clone()
    }

    fn signer_kind(&self) -> &'static str {
        "nip46"
    }

    fn sign(&self, unsigned: &UnsignedEvent) -> SignerOp<SignedEvent> {
        self.sign_count.fetch_add(1, Ordering::Relaxed);
        let kind = nostr::Kind::from_u16(unsigned.kind as u16);
        let tags = unsigned
            .tags
            .iter()
            .filter_map(|t| nostr::Tag::parse(t).ok())
            .collect::<Vec<_>>();
        let built = EventBuilder::new(kind, &unsigned.content)
            .tags(tags)
            .custom_created_at(Timestamp::from(unsigned.created_at))
            .sign_with_keys(&self.keys);
        match built {
            Ok(event) => SignerOp::ok(SignedEvent {
                id: event.id.to_hex(),
                sig: event.sig.to_string(),
                unsigned: UnsignedEvent {
                    pubkey: event.pubkey.to_hex(),
                    kind: event.kind.as_u16() as u32,
                    tags: event.tags.iter().map(|t| t.as_slice().to_vec()).collect(),
                    content: event.content.clone(),
                    created_at: event.created_at.as_secs(),
                },
            }),
            Err(e) => SignerOp::err(nmp_signer_iface::SignerError::Backend(format!(
                "stub sign failed: {e}"
            ))),
        }
    }

    fn nip44_encrypt(&self, recipient_pubkey: &str, plaintext: &str) -> SignerOp<String> {
        // Real NIP-44 v2 against the stub's own keys (ADR-0026). The stub must
        // behave like a production signer for actor-side plumbing tests; an
        // error stub would be a landmine for any future test exercising the
        // seal path. D0 still holds — `nostr::nips::nip44` is a leaf crypto
        // crate, not `nmp-signers`.
        let recipient = match nostr::PublicKey::from_hex(recipient_pubkey) {
            Ok(pk) => pk,
            Err(e) => {
                return SignerOp::err(nmp_signer_iface::SignerError::Backend(format!(
                    "stub: invalid recipient pubkey: {e}"
                )))
            }
        };
        SignerOp::Ready(
            nostr::nips::nip44::encrypt(
                self.keys.secret_key(),
                &recipient,
                plaintext,
                nostr::nips::nip44::Version::V2,
            )
            .map_err(|e| {
                nmp_signer_iface::SignerError::Backend(format!("stub nip44 encrypt: {e}"))
            }),
        )
    }

    fn nip44_decrypt(&self, sender_pubkey: &str, ciphertext: &str) -> SignerOp<String> {
        let sender = match nostr::PublicKey::from_hex(sender_pubkey) {
            Ok(pk) => pk,
            Err(e) => {
                return SignerOp::err(nmp_signer_iface::SignerError::Backend(format!(
                    "stub: invalid sender pubkey: {e}"
                )))
            }
        };
        SignerOp::Ready(
            nostr::nips::nip44::decrypt(self.keys.secret_key(), &sender, ciphertext).map_err(|e| {
                nmp_signer_iface::SignerError::Backend(format!("stub nip44 decrypt: {e}"))
            }),
        )
    }

    fn deliver_response(&self, _response_json: &str) {
        // Stub: no-op. NIP-46 inbound routing is the broker's job (Stage 4).
    }
}

fn fresh() -> (IdentityRuntime, Kernel) {
    (
        IdentityRuntime::new(
            new_bunker_handshake_slot(),
            crate::actor::new_signer_state_slot(),
        ),
        Kernel::new(DEFAULT_VISIBLE_LIMIT),
    )
}

fn stub_signer() -> (Box<StubRemoteSigner>, Arc<AtomicU32>) {
    let sk = SecretKey::from_bech32(TEST_NSEC).expect("valid nsec");
    let keys = Keys::new(sk);
    let stub = StubRemoteSigner::new(keys);
    let count = stub.sign_count_handle();
    (Box::new(stub), count)
}

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
    // aim.md §4.4 / §4.5: pre-classified fields the UI binds directly.
    assert_eq!(row.signer_label, "NIP-46");
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
        Some("connected, waiting for get_public_key".to_string()),
    );
    // D0: handshake state is an app noun — it is written to the identity
    // runtime's shared slot (read by the `"bunker_handshake"` projection),
    // not a typed kernel field.
    let progress = id.bunker_handshake_for_test().expect("set");
    assert_eq!(progress.stage, "awaiting_pubkey");
    assert!(progress.message.is_some());

    // `"idle"` collapses to `None`.
    bunker_handshake_progress(&id, &mut kernel, "idle".to_string(), None);
    assert!(id.bunker_handshake_for_test().is_none());
}

/// Pins the doctrine §6 anti-pattern #1 fix: `BunkerHandshakeDto` carries
/// pre-computed boolean flags + a pre-formatted English `stage_label` so
/// `AccountsView.swift` can render fields directly instead of switching on
/// the raw `stage` string. One assertion block per stage covers every flag
/// transition the shell branches on (visibility guard, cancel-button gate,
/// terminal-icon swap, retry-button label, English subtitle).
#[test]
fn bunker_handshake_dto_pre_computes_view_flags_and_label() {
    let (id, mut kernel) = fresh();

    // ── `"connecting"` — handshake in flight ──────────────────────────────
    bunker_handshake_progress(
        &id,
        &mut kernel,
        "connecting".to_string(),
        Some("dialing wss://r.example".to_string()),
    );
    let dto = id.bunker_handshake_for_test().expect("connecting set");
    assert!(!dto.is_idle, "connecting is not idle");
    assert!(dto.is_in_flight, "connecting is in flight");
    assert!(!dto.is_failed, "connecting has not failed");
    assert!(!dto.is_terminal_success, "connecting is not terminal");
    assert!(dto.can_cancel, "cancel is available while connecting");
    assert_eq!(dto.stage_label, "Connecting to bunker relays…");

    // ── `"awaiting_pubkey"` — also in flight ──────────────────────────────
    bunker_handshake_progress(&id, &mut kernel, "awaiting_pubkey".to_string(), None);
    let dto = id.bunker_handshake_for_test().expect("awaiting set");
    assert!(!dto.is_idle);
    assert!(dto.is_in_flight, "awaiting_pubkey is in flight");
    assert!(!dto.is_failed);
    assert!(!dto.is_terminal_success);
    assert!(dto.can_cancel, "cancel still available awaiting pubkey");
    assert_eq!(dto.stage_label, "Awaiting bunker approval…");

    // ── `"ready"` — terminal success ──────────────────────────────────────
    bunker_handshake_progress(&id, &mut kernel, "ready".to_string(), None);
    let dto = id.bunker_handshake_for_test().expect("ready set");
    assert!(!dto.is_idle);
    assert!(!dto.is_in_flight, "ready is not in flight");
    assert!(!dto.is_failed);
    assert!(
        dto.is_terminal_success,
        "ready is the terminal-success flag"
    );
    assert!(!dto.can_cancel, "no cancel once terminal");
    assert_eq!(dto.stage_label, "Connected");

    // ── `"failed"` — terminal failure ─────────────────────────────────────
    bunker_handshake_progress(
        &id,
        &mut kernel,
        "failed".to_string(),
        Some("relay handshake failed".to_string()),
    );
    let dto = id.bunker_handshake_for_test().expect("failed set");
    assert!(!dto.is_idle);
    assert!(!dto.is_in_flight, "failed is not in flight");
    assert!(dto.is_failed, "failed flag tracks terminal failure");
    assert!(!dto.is_terminal_success);
    assert!(!dto.can_cancel, "no cancel once terminal");
    assert_eq!(dto.stage_label, "Bunker handshake failed");
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
    let outbound = publish_unsigned_event(&id, &mut kernel, unsigned, None, None, &mut Vec::new());
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
    struct IdentityLocalSignerAccess<'a>(&'a super::IdentityRuntime);
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

#[test]
fn snapshot_carries_bunker_handshake_value() {
    // D0: NIP-46 bunker handshake is an app noun surfaced via the built-in
    // `"bunker_handshake"` snapshot projection (registered in `nmp_app_new`),
    // NOT a typed `KernelSnapshot` field. This test reproduces that wiring at
    // the kernel level: a projection closure reads the identity runtime's
    // shared slot and the kernel collects it into `projections` on emit.
    let bunker_slot = new_bunker_handshake_slot();
    let id = IdentityRuntime::new(
        Arc::clone(&bunker_slot),
        crate::actor::new_signer_state_slot(),
    );
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // Register the `"bunker_handshake"` projection exactly as `nmp_app_new`
    // does — a closure reading the shared slot — and bind it onto the kernel.
    let projections = crate::kernel::new_snapshot_projection_slot();
    {
        let projection_slot = Arc::clone(&bunker_slot);
        projections
            .lock()
            .expect("registry lock")
            .register("bunker_handshake", move || {
                let slot = projection_slot.lock().unwrap_or_else(|e| e.into_inner());
                slot.as_ref()
                    .map(|dto| serde_json::to_value(dto).unwrap_or(serde_json::Value::Null))
                    .unwrap_or(serde_json::Value::Null)
            });
    }
    kernel.set_snapshot_projection_handle(projections);

    bunker_handshake_progress(
        &id,
        &mut kernel,
        "connecting".to_string(),
        Some("dialing wss://r.example".to_string()),
    );
    let snapshot = kernel.make_update_value_for_test(true);
    assert!(
        snapshot
            .get("projections")
            .and_then(|projections| projections.get("bunker_handshake"))
            .is_some(),
        "snapshot must carry the bunker_handshake projection key: {snapshot}"
    );
    assert_eq!(
        snapshot["projections"]["bunker_handshake"]["stage"],
        serde_json::json!("connecting")
    );
}

#[test]
fn frame_carries_bunker_handshake_typed_sidecar_only_when_some() {
    // Full-frame integration proof (ADR-0037): register BOTH the generic and
    // the typed `"bunker_handshake"` projections exactly as the actor does
    // (`run_actor_with_observers`), then decode the SnapshotFrame `make_update`
    // actually emits. The typed sidecar entry must be ABSENT while the slot is
    // idle (mirroring JSON `null`) and PRESENT — decoding back to the same
    // value — once a handshake is in flight.
    let bunker_slot = new_bunker_handshake_slot();
    let id = IdentityRuntime::new(
        Arc::clone(&bunker_slot),
        crate::actor::new_signer_state_slot(),
    );
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let projections = crate::kernel::new_snapshot_projection_slot();
    {
        let generic_slot = Arc::clone(&bunker_slot);
        let typed_slot = Arc::clone(&bunker_slot);
        let mut registry = projections.lock().expect("registry lock");
        registry.register("bunker_handshake", move || {
            let slot = generic_slot.lock().unwrap_or_else(|e| e.into_inner());
            slot.as_ref()
                .map(|dto| serde_json::to_value(dto).unwrap_or(serde_json::Value::Null))
                .unwrap_or(serde_json::Value::Null)
        });
        registry.register_typed("bunker_handshake", move || {
            crate::actor::typed_projections::bunker_handshake_typed(&typed_slot)
        });
    }
    kernel.set_snapshot_projection_handle(projections);

    // Idle: typed sidecar must NOT carry the key.
    let (_value, typed) = kernel.make_update_typed_for_test(true);
    assert!(
        !typed.iter().any(|t| t.key == "bunker_handshake"),
        "no bunker_handshake typed sidecar while the slot is idle: {typed:?}"
    );

    // In flight: typed sidecar carries the key, decodes back to the live state.
    bunker_handshake_progress(
        &id,
        &mut kernel,
        "connecting".to_string(),
        Some("dialing wss://r.example".to_string()),
    );
    let (_value, typed) = kernel.make_update_typed_for_test(true);
    let entry = typed
        .iter()
        .find(|t| t.key == "bunker_handshake")
        .expect("bunker_handshake typed sidecar present once a handshake is in flight");
    assert_eq!(entry.file_identifier, "KBHS");
    let decoded = crate::actor::typed_projections::decode_bunker_handshake(&entry.payload)
        .expect("typed sidecar decodes");
    assert_eq!(decoded.stage, "connecting");
    assert_eq!(decoded.message.as_deref(), Some("dialing wss://r.example"));
    assert!(decoded.is_in_flight);
}

#[test]
fn frame_carries_nip46_onboarding_typed_sidecar_always() {
    // Full-frame integration proof (ADR-0037): unlike `bunker_handshake`, the
    // `"nip46_onboarding"` typed sidecar is ALWAYS present (the static
    // signer-app table is emitted even when idle), mirroring the JSON
    // projection's never-`null` contract.
    let bunker_slot = new_bunker_handshake_slot();
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let projections = crate::kernel::new_snapshot_projection_slot();
    {
        let generic_slot = Arc::clone(&bunker_slot);
        let typed_slot = Arc::clone(&bunker_slot);
        let mut registry = projections.lock().expect("registry lock");
        registry.register("nip46_onboarding", move || {
            let dto = crate::actor::commands::build_nip46_onboarding_dto(&generic_slot);
            serde_json::to_value(&dto).unwrap_or(serde_json::Value::Null)
        });
        registry.register_typed("nip46_onboarding", move || {
            crate::actor::typed_projections::nip46_onboarding_typed(&typed_slot)
        });
    }
    kernel.set_snapshot_projection_handle(projections);

    // Even on an idle slot the typed sidecar is present.
    let (_value, typed) = kernel.make_update_typed_for_test(true);
    let entry = typed
        .iter()
        .find(|t| t.key == "nip46_onboarding")
        .expect("nip46_onboarding typed sidecar present even when idle");
    assert_eq!(entry.file_identifier, "KN46");
    let decoded = crate::actor::typed_projections::decode_nip46_onboarding(&entry.payload)
        .expect("typed sidecar decodes");
    assert!(
        !decoded.signer_apps.is_empty(),
        "static signer-app table is always present"
    );
    assert_eq!(decoded.stage_kind, None);
}

// ──────────────────────────────────────────────────────────────────────────
// ADR-0048 D6: `signer_state` projection tests (generalised from the V-14
// step b `bunker_connection_state` projection).
//
// These tests prove the `"signer_state"` projection is driven by REAL
// transitions through the identity-runtime setter and that the snapshot
// reflects them correctly. No live socket required — the command handlers
// (`bunker_connection_state_changed` for the NIP-46 broker path,
// `nip55_signer_state_changed` for the NIP-55 capability path) are called
// directly.
// ──────────────────────────────────────────────────────────────────────────

#[test]
fn signer_state_projection_reflects_transitions() {
    use crate::actor::commands::{bunker_connection_state_changed, new_signer_state_slot};
    use crate::actor::new_bunker_handshake_slot;

    // Wire up a signer-state slot + identity runtime, register the
    // `"signer_state"` projection closure, bind it onto a kernel.
    let signer_state_slot = new_signer_state_slot();
    let id = IdentityRuntime::new(new_bunker_handshake_slot(), Arc::clone(&signer_state_slot));
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let projections = crate::kernel::new_snapshot_projection_slot();
    {
        let slot = Arc::clone(&signer_state_slot);
        projections
            .lock()
            .expect("registry lock")
            .register("signer_state", move || {
                let s = slot.lock().unwrap_or_else(|e| e.into_inner());
                s.as_ref()
                    .map(|dto| serde_json::to_value(dto).unwrap_or(serde_json::Value::Null))
                    .unwrap_or(serde_json::Value::Null)
            });
    }
    kernel.set_snapshot_projection_handle(projections);

    // 1. Initial state: projection key is null (no active remote-signer session).
    let snapshot = kernel.make_update_value_for_test(true);
    assert_eq!(
        snapshot["projections"]["signer_state"],
        serde_json::Value::Null,
        "idle slot must project null: {snapshot}"
    );

    // 2. Simulate the broker reporting "connected" after handshake completes.
    // ADR-0048 D6: "connected" is mapped to "ready" in the unified SignerStateDto.
    bunker_connection_state_changed(&id, &mut kernel, "connected".to_string(), None);
    let snapshot = kernel.make_update_value_for_test(true);
    assert_eq!(
        snapshot["projections"]["signer_state"]["state"],
        serde_json::json!("ready"),
        "connected transition must surface as 'ready' in projection: {snapshot}"
    );
    assert_eq!(
        snapshot["projections"]["signer_state"]["signer_kind"],
        serde_json::json!("nip46"),
        "the NIP-46 broker path must stamp signer_kind=nip46"
    );
    assert_eq!(
        snapshot["projections"]["signer_state"]["is_ready"],
        serde_json::json!(true)
    );
    assert_eq!(
        snapshot["projections"]["signer_state"]["is_reconnecting"],
        serde_json::json!(false)
    );
    assert_eq!(
        snapshot["projections"]["signer_state"]["is_failed"],
        serde_json::json!(false)
    );

    // 3. Simulate a relay flap → "reconnecting".
    bunker_connection_state_changed(
        &id,
        &mut kernel,
        "reconnecting".to_string(),
        Some("connection reset by peer".to_string()),
    );
    let snapshot = kernel.make_update_value_for_test(true);
    assert_eq!(
        snapshot["projections"]["signer_state"]["state"],
        serde_json::json!("reconnecting"),
        "relay flap must project reconnecting: {snapshot}"
    );
    assert_eq!(
        snapshot["projections"]["signer_state"]["is_reconnecting"],
        serde_json::json!(true)
    );
    assert_eq!(
        snapshot["projections"]["signer_state"]["reason"],
        serde_json::json!("connection reset by peer")
    );

    // 4. Simulate a permanent failure → "failed".
    bunker_connection_state_changed(
        &id,
        &mut kernel,
        "failed".to_string(),
        Some("403 Forbidden".to_string()),
    );
    let snapshot = kernel.make_update_value_for_test(true);
    assert_eq!(
        snapshot["projections"]["signer_state"]["state"],
        serde_json::json!("failed"),
        "permanent failure must project failed: {snapshot}"
    );
    assert_eq!(
        snapshot["projections"]["signer_state"]["is_failed"],
        serde_json::json!(true)
    );
    assert_eq!(
        snapshot["projections"]["signer_state"]["reason"],
        serde_json::json!("403 Forbidden")
    );
}

#[test]
fn signer_state_slot_reflects_direct_write() {
    // Drive `bunker_connection_state_changed` (the pub command handler) directly
    // to prove the slot writer pre-computes flags correctly without going
    // through the actor loop. Uses the test-accessor to read back the slot.
    use crate::actor::commands::{bunker_connection_state_changed, new_signer_state_slot};
    use crate::actor::new_bunker_handshake_slot;

    let signer_state_slot = new_signer_state_slot();
    let id = IdentityRuntime::new(new_bunker_handshake_slot(), Arc::clone(&signer_state_slot));
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // Idle: slot is None.
    assert!(id.signer_state_for_test().is_none());

    // Write "reconnecting" via the command handler.
    bunker_connection_state_changed(
        &id,
        &mut kernel,
        "reconnecting".to_string(),
        Some("timeout".to_string()),
    );
    let dto = id
        .signer_state_for_test()
        .expect("slot must be Some after reconnecting");
    assert_eq!(dto.state, "reconnecting");
    assert_eq!(dto.signer_kind, "nip46");
    assert!(dto.is_reconnecting);
    assert!(!dto.is_ready);
    assert!(!dto.is_failed);
    assert_eq!(dto.reason.as_deref(), Some("timeout"));

    // Overwrite with "connected" (mapped to "ready" by ADR-0048 D6).
    bunker_connection_state_changed(&id, &mut kernel, "connected".to_string(), None);
    let dto = id
        .signer_state_for_test()
        .expect("slot must be Some after connected");
    assert!(dto.is_ready, "connected maps to is_ready=true");
    assert!(!dto.is_reconnecting);
    assert!(!dto.is_failed);
    assert!(dto.reason.is_none());

    // Overwrite with "failed".
    bunker_connection_state_changed(
        &id,
        &mut kernel,
        "failed".to_string(),
        Some("403 Forbidden".to_string()),
    );
    let dto = id
        .signer_state_for_test()
        .expect("slot must be Some after failed");
    assert!(dto.is_failed);
    assert!(!dto.is_ready);
    assert!(!dto.is_reconnecting);
    assert_eq!(dto.reason.as_deref(), Some("403 Forbidden"));
}

#[test]
fn signer_state_slot_reflects_nip55_transitions() {
    // ADR-0048 D6: the NIP-55 capability path writes into the SAME slot via
    // `nip55_signer_state_changed`, stamping `signer_kind = "nip55"` and the
    // NIP-55-specific states (`awaiting_approval` / `unavailable`).
    use crate::actor::commands::{new_signer_state_slot, nip55_signer_state_changed};
    use crate::actor::new_bunker_handshake_slot;

    let signer_state_slot = new_signer_state_slot();
    let id = IdentityRuntime::new(new_bunker_handshake_slot(), Arc::clone(&signer_state_slot));
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // Intent round-trip in flight → "awaiting_approval" drives the host's
    // "Waiting for Amber…" inline affordance.
    nip55_signer_state_changed(&id, &mut kernel, "awaiting_approval".to_string(), None);
    let dto = id
        .signer_state_for_test()
        .expect("slot must be Some after awaiting_approval");
    assert_eq!(dto.signer_kind, "nip55");
    assert_eq!(dto.state, "awaiting_approval");
    assert!(dto.is_awaiting_approval);
    assert!(!dto.is_ready);
    assert!(!dto.is_unavailable);

    // Signer app uninstalled mid-session → "unavailable" prompts re-auth.
    nip55_signer_state_changed(
        &id,
        &mut kernel,
        "unavailable".to_string(),
        Some("signer app not installed".to_string()),
    );
    let dto = id
        .signer_state_for_test()
        .expect("slot must be Some after unavailable");
    assert!(dto.is_unavailable);
    assert!(!dto.is_awaiting_approval);
    assert_eq!(dto.reason.as_deref(), Some("signer app not installed"));

    // Approval granted → "ready".
    nip55_signer_state_changed(&id, &mut kernel, "ready".to_string(), None);
    let dto = id
        .signer_state_for_test()
        .expect("slot must be Some after ready");
    assert!(dto.is_ready);
    assert_eq!(dto.signer_kind, "nip55");
    assert!(dto.reason.is_none());
}

// ──────────────────────────────────────────────────────────────────────────
// End-to-end dispatch test — drives the new `ActorCommand` variants through
// the spawned `run_actor` loop so the dispatch arms are exercised (not just
// the command-handler functions they wrap).
// ──────────────────────────────────────────────────────────────────────────

/// PR-B (#991/#979): drain snapshot frames from the channel and return the typed
/// sidecar entries of the LAST snapshot frame received. The generic JSON `payload`
/// is no longer emitted on the wire after payload zeroing; callers must use the
/// typed sidecar decoders.
fn last_typed_sidecars(
    upd_rx: &std::sync::mpsc::Receiver<crate::update_envelope::UpdateFrameBytes>,
) -> Vec<crate::update_envelope::TypedProjectionData> {
    let mut last: Vec<crate::update_envelope::TypedProjectionData> = Vec::new();
    while let Ok(frame) = upd_rx.try_recv() {
        if let Ok(typed) = crate::update_envelope::decode_snapshot_typed_projections(&frame) {
            last = typed;
        }
    }
    last
}

#[test]
fn snapshot_carries_nip46_onboarding_projection() {
    // The built-in `"nip46_onboarding"` projection is wired alongside
    // `"bunker_handshake"` and produces a typed DTO with the static
    // signer-app table + pre-computed flags. This end-to-end test drives
    // a `BunkerHandshakeProgress` through the actor and asserts both
    // projections appear in the emitted snapshot.
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use crate::actor::{run_actor_with_observers, ActorCommand, ActorMail, CommandSender};
    use crate::capability_socket::new_capability_callback_slot;
    use std::sync::atomic::AtomicU64;
    use std::sync::Arc;

    let (inbox_tx, cmd_rx) = mpsc::channel::<ActorMail>();
    let cmd_tx = CommandSender::new(inbox_tx);
    let (upd_tx, upd_rx) = mpsc::channel::<crate::update_envelope::UpdateFrameBytes>();

    let snapshot_projections = crate::kernel::new_snapshot_projection_slot();
    let bunker_slot = crate::actor::new_bunker_handshake_slot();
    // Replicate the wiring `nmp_app_new` does for the two NIP-46 projections.
    {
        let slot = Arc::clone(&bunker_slot);
        snapshot_projections
            .lock()
            .expect("registry lock")
            .register("bunker_handshake", move || {
                let s = slot.lock().unwrap_or_else(|e| e.into_inner());
                s.as_ref()
                    .map(|dto| serde_json::to_value(dto).unwrap_or(serde_json::Value::Null))
                    .unwrap_or(serde_json::Value::Null)
            });
    }
    {
        let slot = Arc::clone(&bunker_slot);
        snapshot_projections
            .lock()
            .expect("registry lock")
            .register("nip46_onboarding", move || {
                let dto = crate::actor::build_nip46_onboarding_dto(&slot);
                serde_json::to_value(&dto).unwrap_or(serde_json::Value::Null)
            });
    }

    let actor_self_tx = cmd_tx.clone();
    thread::spawn(move || {
        run_actor_with_observers(
            cmd_rx,
            actor_self_tx,
            upd_tx,
            crate::actor::new_lifecycle_observer_slot(),
            crate::actor::new_event_observer_slot(),
            crate::actor::new_raw_event_observer_slot(),
            snapshot_projections,
            // V-38: substrate-generic relay-text interceptor slot.
            crate::substrate::new_relay_text_interceptor_slot(),
            // ADR-0051: throwaway relay-connected hook slot.
            crate::substrate::new_relay_connected_hook_slot(),
            bunker_slot,
            // V-14 step b: throwaway connection-state slot.
            crate::actor::new_signer_state_slot(),
            // ADR-0052 §D3: throwaway per-app signer hook slots (this
            // remote-signer test drives the broker through the bunker_slot
            // shared handshake slot, not the hook seam).
            crate::new_bunker_hook_slot(),
            crate::new_external_signer_hook_slot(),
            // Typed slot constructor.
            crate::kernel::new_app_relay_slot(),
            Arc::new(std::sync::Mutex::new(None)),
            Arc::new(std::sync::Mutex::new(None)),
            new_capability_callback_slot(),
            Arc::new(std::sync::Mutex::new(None)),
            Arc::new(AtomicU64::new(0)),
            // D2 — test wiring; no coverage hook installed.
            Arc::new(std::sync::Mutex::new(None)),
            crate::substrate::new_req_frame_interceptor_slot(),
            // Host-op handler slot — test wiring; this remote-signer test does
            // not exercise the `DispatchHostOp` path.
            crate::substrate::new_host_op_handler_slot(),
            // V-40 — test wiring; no NIP-17 cache here.
            Arc::new(std::sync::RwLock::new(
                crate::substrate::EventIngestDispatcher::new(),
            )),
            Arc::new(std::sync::Mutex::new(
                crate::substrate::empty_dm_inbox_relay_lookup(),
            )),
            // Test wiring — no blocked-relay cache installed; the kernel
            // defaults to the empty-lookup that returns an empty
            // `BlockedRelaySet` for every account.
            Arc::new(std::sync::Mutex::new(
                crate::substrate::empty_blocked_relay_lookup(),
            )),
            // Test wiring — no bootstrap self-kinds override; the kernel
            // uses its built-in `[0, 3, 10002, 10000, 10006]` default.
            Arc::new(std::sync::Mutex::new(None)),
            // V-51 phase 4 — test wiring; nothing reads the routing-trace slot here.
            Arc::new(std::sync::Mutex::new(None)),
            // V-51 phase 5 — test wiring; no per-app routing factory installed.
            Arc::new(std::sync::Mutex::new(None)),
            // Spec §271 (2026-05-25) — test wiring; no per-app
            // publish-resolver factory installed. The under-`cfg(test)`
            // auto-install on `Kernel::new()` (also via this actor's
            // `Kernel::with_storage_path`) gives the kernel a working
            // `Nip65OutboxResolver` regardless of this slot.
            Arc::new(std::sync::Mutex::new(None)),
            // Test wiring; no raw-event forwarding policy installed.
            crate::slots::new_raw_event_forward_policy_slot(),
            // V-82 — test wiring; nothing outside the actor reads the
            // active-account slot here (private throwaway).
            crate::slots::new_active_account_slot(),
            // V-83 — test wiring; nothing outside the actor reads the
            // event-store slot here (private throwaway).
            crate::slots::new_event_store_slot(),
            // Test-support kernel-clock slot — private throwaway (None).
            crate::slots::new_kernel_clock_slot(),
        );
    });

    cmd_tx
        .send(ActorCommand::Start {
            visible_limit: 50,
            emit_hz: 30,
            initial_relays: Vec::new(),
        })
        .unwrap();

    cmd_tx
        .send(ActorCommand::BunkerHandshakeProgress {
            stage: "connecting".to_string(),
            message: Some("dialing relay".to_string()),
        })
        .unwrap();

    thread::sleep(Duration::from_millis(300));
    let _ = cmd_tx.send(ActorCommand::Shutdown);

    // PR-B (#991/#979): payload is zeroed — read from the typed sidecar instead.
    let sidecars = last_typed_sidecars(&upd_rx);
    assert!(!sidecars.is_empty(), "actor produced no snapshot frames");

    // Decode the nip46_onboarding typed sidecar.
    let onboarding_entry = sidecars
        .iter()
        .find(|p| p.key == crate::actor::typed_projections::NIP46_ONBOARDING_SCHEMA_ID)
        .expect("snapshot missing nip46_onboarding typed sidecar");
    let onboarding = crate::actor::typed_projections::decode_nip46_onboarding(&onboarding_entry.payload)
        .expect("nip46_onboarding sidecar must decode");

    // The typed projection's `stage_kind` + `is_in_flight` must reflect the
    // same broker progress as the prior JSON path.
    assert_eq!(
        onboarding.stage_kind.as_deref(),
        Some("connecting"),
        "nip46_onboarding must carry stage_kind=connecting"
    );
    assert!(
        onboarding.is_in_flight,
        "nip46_onboarding must pre-compute is_in_flight=true for connecting"
    );
    assert!(
        !onboarding.signer_apps.is_empty(),
        "nip46_onboarding must carry non-empty signer_apps table"
    );
}

#[test]
fn dispatch_add_remote_signer_then_progress_surfaces_on_snapshot() {
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use crate::actor::{run_actor, ActorCommand, ActorMail, CommandSender};

    let (inbox_tx, cmd_rx) = mpsc::channel::<ActorMail>();
    let cmd_tx = CommandSender::new(inbox_tx);
    let (upd_tx, upd_rx) = mpsc::channel::<crate::update_envelope::UpdateFrameBytes>();
    let actor_self_tx = cmd_tx.clone();
    thread::spawn(move || run_actor(cmd_rx, actor_self_tx, upd_tx));

    cmd_tx
        .send(ActorCommand::Start {
            visible_limit: 50,
            emit_hz: 30,
            initial_relays: Vec::new(),
        })
        .unwrap();

    let (handle, _count) = stub_signer();
    let pk = handle.pubkey_hex();
    cmd_tx
        .send(ActorCommand::AddSigner {
            source: crate::actor::SignerSource::RemoteHandle(handle),
            make_active: true,
        })
        .unwrap();
    cmd_tx
        .send(ActorCommand::BunkerHandshakeProgress {
            stage: "ready".to_string(),
            message: None,
        })
        .unwrap();

    // Let the actor drain both commands and emit at least one snapshot.
    thread::sleep(Duration::from_millis(300));
    let _ = cmd_tx.send(ActorCommand::Shutdown);

    // PR-B (#991/#979): payload zeroed — read from the typed sidecar instead.
    let sidecars = last_typed_sidecars(&upd_rx);
    assert!(!sidecars.is_empty(), "actor produced no snapshot frames");

    // Decode the `accounts` typed sidecar and assert the remote-signer pubkey
    // is present and the signer_kind is "nip46".
    let accounts_entry = sidecars
        .iter()
        .find(|p| p.key == crate::kernel::public_typed_projections::ACCOUNTS_SCHEMA_ID)
        .expect("snapshot missing accounts typed sidecar");
    let accounts = crate::kernel::public_typed_projections::decode_accounts(&accounts_entry.payload)
        .expect("accounts sidecar must decode");
    assert!(
        accounts.accounts.iter().any(|row| row.id == pk || row.npub.contains(&pk)),
        "snapshot missing remote-signer pubkey {pk} in accounts sidecar"
    );
    assert!(
        accounts.accounts.iter().any(|row| row.signer_kind == "nip46"),
        "snapshot missing nip46 signer_kind in accounts sidecar"
    );

    // Decode the `bunker_handshake` typed sidecar and assert stage=ready.
    let bhs_entry = sidecars
        .iter()
        .find(|p| p.key == crate::actor::typed_projections::BUNKER_HANDSHAKE_SCHEMA_ID)
        .expect("snapshot missing bunker_handshake typed sidecar");
    let handshake = crate::actor::typed_projections::decode_bunker_handshake(&bhs_entry.payload)
        .expect("bunker_handshake sidecar must decode");
    assert_eq!(
        handshake.stage, "ready",
        "snapshot missing handshake stage=ready"
    );
}

// ──────────────────────────────────────────────────────────────────────────
// RemoteSignerHandle NIP-44 seam (ADR-0026): the actor reaches NIP-44
// through the same trait it uses for `sign()`. These tests pin the new
// methods on the trait object via the `StubRemoteSigner` double.
// ──────────────────────────────────────────────────────────────────────────

#[test]
fn remote_handle_nip44_round_trips_through_the_seam() {
    // ADR-0026: encrypt to a recipient, then decrypt from that recipient's
    // perspective — NIP-44 is symmetric in the shared conversation key, so a
    // ciphertext sealed by A to B decrypts with B's key against A's pubkey.
    let alice_sk = SecretKey::from_bech32(TEST_NSEC).expect("valid nsec");
    let alice = StubRemoteSigner::new(Keys::new(alice_sk));
    let bob = StubRemoteSigner::new(Keys::generate());

    let alice_pk = RemoteSignerHandle::pubkey_hex(&alice);
    let bob_pk = RemoteSignerHandle::pubkey_hex(&bob);

    let plaintext = "the kind:13 rumor body";
    let ciphertext = RemoteSignerHandle::nip44_encrypt(&alice, &bob_pk, plaintext)
        .wait(std::time::Duration::from_secs(1))
        .expect("encrypt resolves");
    assert_ne!(
        ciphertext, plaintext,
        "ciphertext must not be the plaintext"
    );

    let decrypted = RemoteSignerHandle::nip44_decrypt(&bob, &alice_pk, &ciphertext)
        .wait(std::time::Duration::from_secs(1))
        .expect("decrypt resolves");
    assert_eq!(
        decrypted, plaintext,
        "round-trip must recover the plaintext"
    );
}

#[test]
fn remote_handle_nip44_encrypt_with_malformed_pubkey_surfaces_err() {
    // D6: a bad hex pubkey through the actor-facing seam must surface as an
    // error, never a panic.
    let (signer, _count) = stub_signer();
    let err = RemoteSignerHandle::nip44_encrypt(&*signer, "not-hex", "plaintext")
        .wait(std::time::Duration::from_millis(100))
        .expect_err("malformed pubkey must surface as Err");
    match err {
        nmp_signer_iface::SignerError::Backend(m) => {
            assert!(m.contains("invalid recipient pubkey"), "got: {m}")
        }
        other => panic!("expected Backend Err, got {other:?}"),
    }
}
