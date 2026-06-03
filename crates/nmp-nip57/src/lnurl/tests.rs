//! Unit tests for the LNURL fetcher (`FetchLnurlInvoiceCommand`) — the
//! V-41 migration of the legacy `nmp-core::actor::commands::zap::tests`.
//!
//! HTTP I/O is not exercised here (it needs a live LN provider; the iOS
//! integration shell drives that end-to-end). The tests below pin three
//! observable contracts:
//!
//! 1. V-07 recipient-relay injection — the kind:9734 `relays` tag is
//!    populated from the substrate
//!    [`RecipientRelayLookup`](nmp_core::substrate::RecipientRelayLookup)
//!    capability (kernel-side adapter routes via `outbox_router`); a
//!    pre-existing non-empty `relays` row is preserved.
//! 2. The kind:9734 signer (`sign_zap_request`) round-trips through
//!    `EventBuilder` and rejects out-of-range kinds.
//! 3. The sync-path fail branches in `FetchLnurlInvoiceCommand::run` (no
//!    local keys, sign error) emit the expected `ShowToast` +
//!    `RecordActionFailure` follow-ups through the context's `send`
//!    closure.

use super::*;
use nmp_core::substrate::{
    ActionStageTracker, KernelClock, LocalSignerAccess, NoopErrorSurface,
    NoopRecipientRelayLookup, ProtocolCommandContextParts, RecipientRelayLookup, SignedEvent,
    SignerError, SignerOp,
};

const RECIPIENT_HEX: &str =
    "bb11223344556677889900aabbccddeeff00112233445566778899aabbccddff";

fn unsigned_for(tags: Vec<Vec<String>>) -> UnsignedEvent {
    UnsignedEvent {
        pubkey: String::new(),
        kind: 9734,
        tags,
        content: String::new(),
        created_at: 0,
    }
}

// ── Capability adapters used by the LNURL test harness ──

struct FixedClock(u64);
impl KernelClock for FixedClock {
    fn now_secs(&self) -> u64 {
        self.0
    }
}

/// Test [`LocalSignerAccess`] modelling the three account states the V-78
/// zap sign path must distinguish:
///
/// * [`Self::None`] — no account at all. `active_local_keys` → `None`,
///   `sign_active_nonblocking` → `Err` (genuine fail-closed).
/// * [`Self::Local`] — a local nsec. `active_local_keys` → `Some(keys)`,
///   `sign_active_nonblocking` → `SignerOp::Ready` (signed on the spot).
/// * [`Self::Bunker`] — a NIP-46 bunker. `active_local_keys` → `None` (the
///   V-78 bug surface), but `sign_active_nonblocking` → `SignerOp::Pending`
///   resolved off-actor — so the zap is NOT fail-closed.
enum LocalSigner {
    None,
    Local(Keys),
    /// Carries the keys used to mint the signed event the parked `Pending`
    /// op resolves to (modelling the broker turning the request around),
    /// plus a sender kept alive so the channel does not disconnect early.
    Bunker(Keys, std::sync::Mutex<Vec<std::sync::mpsc::Sender<Result<SignedEvent, SignerError>>>>),
}

impl LocalSigner {
    /// Back-compat constructor for the existing relay-injection tests that
    /// wrote `LocalSigner::none()` (they never sign). Maps to [`Self::None`].
    fn none() -> Self {
        Self::None
    }

    fn local(keys: Keys) -> Self {
        Self::Local(keys)
    }

    /// A bunker whose broker resolves the parked sign immediately with a
    /// valid kind:9734 signed by `keys` (so the worker's `op.wait` succeeds).
    fn bunker(keys: Keys) -> Self {
        Self::Bunker(keys, std::sync::Mutex::new(Vec::new()))
    }
}

impl LocalSignerAccess for LocalSigner {
    fn active_local_keys(&self) -> Option<Keys> {
        match self {
            Self::Local(keys) => Some(keys.clone()),
            // V-78: a bunker has NO local keys — this is exactly the accessor
            // the buggy path used to gate on.
            Self::None | Self::Bunker(..) => None,
        }
    }
    fn signer_for_seal(
        &self,
    ) -> Option<std::sync::Arc<dyn nmp_core::substrate::SignerForSeal>> {
        None
    }
    fn sign_active_nonblocking(
        &self,
        unsigned: &UnsignedEvent,
    ) -> Result<SignerOp<SignedEvent>, String> {
        match self {
            Self::None => Err("no active account — sign in first".to_string()),
            Self::Local(keys) => {
                // Sign synchronously → Ready, mirroring the production
                // local-nsec path.
                let json = sign_zap_request(keys, unsigned)
                    .map_err(|e| format!("local sign failed: {e}"))?;
                let event: nostr::Event = serde_json::from_str(&json)
                    .map_err(|e| format!("reparse signed: {e}"))?;
                Ok(SignerOp::ok(nostr_event_to_signed(&event)))
            }
            Self::Bunker(keys, senders) => {
                // Model the broker: a Pending op whose channel is resolved on
                // a spawned thread (off-actor, like the real RemoteSignerHandle).
                let (tx, rx) = std::sync::mpsc::channel();
                let json = sign_zap_request(keys, unsigned)
                    .map_err(|e| format!("bunker sign failed: {e}"))?;
                let event: nostr::Event = serde_json::from_str(&json)
                    .map_err(|e| format!("reparse signed: {e}"))?;
                let signed = nostr_event_to_signed(&event);
                let tx_clone = tx.clone();
                senders.lock().unwrap().push(tx);
                std::thread::spawn(move || {
                    let _ = tx_clone.send(Ok(signed));
                });
                Ok(SignerOp::Pending(rx))
            }
        }
    }
}

/// Flatten a signed `nostr::Event` into the substrate [`SignedEvent`] the
/// signer seam produces — the inverse of `signed_event_to_nostr_json`.
fn nostr_event_to_signed(event: &nostr::Event) -> SignedEvent {
    SignedEvent {
        id: event.id.to_hex(),
        sig: event.sig.to_string(),
        unsigned: UnsignedEvent {
            pubkey: event.pubkey.to_hex(),
            kind: u32::from(event.kind.as_u16()),
            tags: event.tags.iter().map(|t| t.as_slice().to_vec()).collect(),
            content: event.content.clone(),
            created_at: event.created_at.as_secs(),
        },
    }
}

struct RecordingStages(std::sync::Mutex<Vec<String>>);
impl ActionStageTracker for RecordingStages {
    fn record_requested(&self, correlation_id: &str) {
        self.0.lock().unwrap().push(correlation_id.to_string());
    }
}

/// Test-only [`RecipientRelayLookup`] returning a fixed URL list for
/// every recipient. Records every `(recipient, kind)` it was asked
/// about so tests can assert on the routing call shape.
struct FixedRecipientLookup {
    urls: Vec<String>,
    seen: std::sync::Mutex<Vec<(String, u32)>>,
}

impl FixedRecipientLookup {
    fn with_urls(urls: Vec<&'static str>) -> Self {
        Self {
            urls: urls.into_iter().map(str::to_string).collect(),
            seen: std::sync::Mutex::new(Vec::new()),
        }
    }
}

impl RecipientRelayLookup for FixedRecipientLookup {
    fn recipient_publish_relays(&self, recipient: &str, kind: u32) -> Vec<String> {
        self.seen
            .lock()
            .unwrap()
            .push((recipient.to_string(), kind));
        self.urls.clone()
    }
}

/// Build a `ProtocolCommandContext` whose kernel accessors are wired to
/// fixed capability adapters. The LNURL tests never spawn a worker, so
/// the sender is unused; the DM-inbox / toast / failure surfaces use the
/// `Empty` / `Noop` defaults.
fn ctx_with<'a>(
    send: &'a dyn Fn(ActorCommand),
    clock: &'a dyn KernelClock,
    signers: &'a LocalSigner,
    stages: &'a RecordingStages,
    recipients: &'a dyn RecipientRelayLookup,
) -> ProtocolCommandContext<'a> {
    let (tx, _rx) = std::sync::mpsc::channel::<ActorCommand>();
    static EMPTY_DM: nmp_core::substrate::EmptyDmInboxRelayLookup =
        nmp_core::substrate::EmptyDmInboxRelayLookup;
    static ERRORS: NoopErrorSurface = NoopErrorSurface;
    ProtocolCommandContext::new(ProtocolCommandContextParts {
        send,
        command_sender: tx,
        clock,
        signers,
        dms: &EMPTY_DM,
        errors: &ERRORS,
        stages,
        recipients,
    })
}

// ────────────────────────────────────────────────────────────────────
// V-07 — recipient relay injection through the protocol context.
// ────────────────────────────────────────────────────────────────────

#[test]
fn inject_recipient_relays_preserves_existing_relays_tag() {
    let send = |_: ActorCommand| {};
    let clock = FixedClock(1_700_000_000);
    let signers = LocalSigner::none();
    let stages = RecordingStages(std::sync::Mutex::new(Vec::new()));
    let recipients =
        FixedRecipientLookup::with_urls(vec!["wss://from-router.example"]);
    let ctx = ctx_with(&send, &clock, &signers, &stages, &recipients);

    let mut unsigned = unsigned_for(vec![
        vec!["relays".to_string(), "wss://chosen.example".to_string()],
        vec!["p".to_string(), RECIPIENT_HEX.to_string()],
    ]);
    inject_recipient_relays(&ctx, &mut unsigned);
    let relays_tag = unsigned
        .tags
        .iter()
        .find(|t| t.first().map(String::as_str) == Some("relays"))
        .expect("relays tag must be present");
    assert_eq!(
        relays_tag,
        &vec!["relays".to_string(), "wss://chosen.example".to_string()],
        "an explicit non-empty relays tag must be left untouched"
    );
    let relays_count = unsigned
        .tags
        .iter()
        .filter(|t| t.first().map(String::as_str) == Some("relays"))
        .count();
    assert_eq!(relays_count, 1, "must not duplicate the relays tag");
    // And the router must NOT have been consulted — the caller's tag wins.
    assert!(
        recipients.seen.lock().unwrap().is_empty(),
        "router must not be consulted when a filled relays row is present"
    );
}

#[test]
fn inject_recipient_relays_injects_when_tag_absent() {
    let send = |_: ActorCommand| {};
    let clock = FixedClock(1_700_000_000);
    let signers = LocalSigner::none();
    let stages = RecordingStages(std::sync::Mutex::new(Vec::new()));
    let recipients = FixedRecipientLookup::with_urls(vec![
        "wss://write-a.example",
        "wss://write-b.example",
    ]);
    let ctx = ctx_with(&send, &clock, &signers, &stages, &recipients);

    let mut unsigned =
        unsigned_for(vec![vec!["p".to_string(), RECIPIENT_HEX.to_string()]]);
    inject_recipient_relays(&ctx, &mut unsigned);
    let relays_tag = unsigned
        .tags
        .iter()
        .find(|t| t.first().map(String::as_str) == Some("relays"))
        .expect("V-07: actor must inject a relays tag when caller omitted it");
    assert_eq!(
        relays_tag,
        &vec![
            "relays".to_string(),
            "wss://write-a.example".to_string(),
            "wss://write-b.example".to_string(),
        ],
        "must inject every router-resolved URL into the relays row"
    );
    // The router was asked once, for kind:9735 (the zap receipt the LN
    // provider will mint — that's the kind whose publish-direction routes
    // to the recipient's NIP-65 write set).
    assert_eq!(
        *recipients.seen.lock().unwrap(),
        vec![(RECIPIENT_HEX.to_string(), 9735u32)],
        "router must be asked for kind:9735 against the p-tag recipient"
    );
}

#[test]
fn inject_recipient_relays_treats_bare_relays_key_as_absent() {
    // A `["relays"]` row with no URLs is malformed — treat as absent so
    // the injection still fires, AND the malformed row must be discarded.
    let send = |_: ActorCommand| {};
    let clock = FixedClock(1_700_000_000);
    let signers = LocalSigner::none();
    let stages = RecordingStages(std::sync::Mutex::new(Vec::new()));
    let recipients =
        FixedRecipientLookup::with_urls(vec!["wss://write.example"]);
    let ctx = ctx_with(&send, &clock, &signers, &stages, &recipients);

    let mut unsigned = unsigned_for(vec![
        vec!["relays".to_string()],
        vec!["p".to_string(), RECIPIENT_HEX.to_string()],
    ]);
    inject_recipient_relays(&ctx, &mut unsigned);
    let relays_rows: Vec<&Vec<String>> = unsigned
        .tags
        .iter()
        .filter(|t| t.first().map(String::as_str) == Some("relays"))
        .collect();
    assert_eq!(
        relays_rows.len(),
        1,
        "must end up with exactly one relays row (the bare one is dropped)"
    );
    assert!(
        relays_rows[0].len() > 1,
        "the surviving relays row must carry the injected URLs: {:?}",
        relays_rows[0]
    );
}

#[test]
fn inject_recipient_relays_falls_back_to_bootstrap_when_p_tag_missing() {
    // Defensive — a builder bug that drops the `p` tag must NOT produce
    // a zap with an empty relays tag. The router resolves the empty
    // recipient against its cold-start AppRelay seed (lane 7) — the test
    // wires that resolution through the `FixedRecipientLookup` adapter
    // (which models the router's lane-7 fallback).
    let send = |_: ActorCommand| {};
    let clock = FixedClock(1_700_000_000);
    let signers = LocalSigner::none();
    let stages = RecordingStages(std::sync::Mutex::new(Vec::new()));
    let recipients = FixedRecipientLookup::with_urls(vec![
        "wss://bootstrap.example",
    ]);
    let ctx = ctx_with(&send, &clock, &signers, &stages, &recipients);

    let mut unsigned = unsigned_for(Vec::new());
    inject_recipient_relays(&ctx, &mut unsigned);
    let relays_tag = unsigned
        .tags
        .iter()
        .find(|t| t.first().map(String::as_str) == Some("relays"))
        .expect("must inject a relays tag even when p tag is absent");
    assert_eq!(
        relays_tag,
        &vec![
            "relays".to_string(),
            "wss://bootstrap.example".to_string(),
        ],
        "router-resolved URLs (router's own cold-start lane) populate the tag"
    );
    // The router was consulted with an empty recipient pubkey — the LNURL
    // fetcher does not synthesise a fake recipient when the `p` tag is
    // missing; routing decides the fallback (lane 7 in production).
    assert_eq!(
        *recipients.seen.lock().unwrap(),
        vec![(String::new(), 9735u32)],
        "router asked with empty recipient when p tag missing"
    );
}

#[test]
fn inject_recipient_relays_emits_empty_tag_when_router_returns_no_urls() {
    // Documented behaviour from the function doc comment: if the router
    // returns an empty `Vec` (e.g. `RoutingError::Unroutable` — no NIP-65
    // cache hit AND no AppRelay seed), the `relays` tag is still added,
    // empty. The LN provider then falls back to its own default; the
    // contract NIP-57 § "Appendix A" wants the tag PRESENT.
    let send = |_: ActorCommand| {};
    let clock = FixedClock(1_700_000_000);
    let signers = LocalSigner::none();
    let stages = RecordingStages(std::sync::Mutex::new(Vec::new()));
    let recipients = NoopRecipientRelayLookup;
    let ctx = ctx_with(&send, &clock, &signers, &stages, &recipients);

    let mut unsigned =
        unsigned_for(vec![vec!["p".to_string(), RECIPIENT_HEX.to_string()]]);
    inject_recipient_relays(&ctx, &mut unsigned);
    let relays_tag = unsigned
        .tags
        .iter()
        .find(|t| t.first().map(String::as_str) == Some("relays"))
        .expect("relays row must be added even with an empty URL set");
    assert_eq!(
        relays_tag,
        &vec!["relays".to_string()],
        "empty router result yields a bare relays row (LN provider falls back)"
    );
}

// ────────────────────────────────────────────────────────────────────
// `sign_zap_request` — round-trip + kind range.
// ────────────────────────────────────────────────────────────────────

#[test]
fn sign_zap_request_round_trips_through_event_builder() {
    let keys = Keys::generate();
    let unsigned = UnsignedEvent {
        pubkey: keys.public_key().to_hex(),
        kind: 9734,
        tags: vec![
            vec![
                "relays".to_string(),
                "wss://relay.example".to_string(),
            ],
            vec![
                "p".to_string(),
                "bb11223344556677889900aabbccddeeff00112233445566778899aabbccddff".to_string(),
            ],
        ],
        content: "great post 🤙".to_string(),
        created_at: 1_700_000_000,
    };
    let json = sign_zap_request(&keys, &unsigned).expect("sign must succeed");
    let event: nostr::Event =
        serde_json::from_str(&json).expect("signed output must be a valid nostr::Event");
    assert_eq!(event.kind.as_u16(), 9734);
    assert_eq!(event.content, "great post 🤙");
    assert!(!event.sig.to_string().is_empty());
}

#[test]
fn sign_zap_request_rejects_out_of_range_kind() {
    let keys = Keys::generate();
    let unsigned = UnsignedEvent {
        pubkey: keys.public_key().to_hex(),
        // 100_000 is outside the u16 range nostr::Kind accepts.
        kind: 100_000,
        tags: Vec::new(),
        content: String::new(),
        created_at: 0,
    };
    assert!(sign_zap_request(&keys, &unsigned).is_err());
}

// ────────────────────────────────────────────────────────────────────
// `FetchLnurlInvoiceCommand::run` — sign-path branches.
//
// The HTTP-success leg requires a live LN provider; the iOS shell drives
// that end-to-end. The branches below are what we can pin from the
// unit-test level: the genuine no-account fail-closed, the V-78 bunker
// path that NO LONGER fails closed, and stage tracking against
// `correlation_id`.
// ────────────────────────────────────────────────────────────────────

use std::sync::Mutex;

/// Drive `run()` with a captured send sink + recordable stage tracker.
struct Sink {
    sends: Mutex<Vec<ActorCommand>>,
    stages: Mutex<Vec<String>>,
}

impl Sink {
    fn new() -> Self {
        Self {
            sends: Mutex::new(Vec::new()),
            stages: Mutex::new(Vec::new()),
        }
    }
}

/// A LNURL target that fails `lnurl_to_well_known_url` PARSE — no `@`, not
/// `lnurl1…`, not `https://`. The HTTP worker errors at the very first line
/// of `fetch_lnurl_invoice_blocking` (the parse), before opening any socket,
/// so the sign-path tests below stay fully hermetic (no network egress from
/// `cargo test`). The worker's failure `send`s land on the dropped receiver
/// installed by `ctx_with` — invisible to the actor-thread `sink.sends`.
const UNREACHABLE_LNURL: &str = "not-a-valid-lnurl-target";

/// Run the command with a given signer and collect everything the `send`
/// closure captured + the recorded stages. `lnurl` lets a test pick a
/// parse-failing target ([`UNREACHABLE_LNURL`]) so the spawned worker never
/// touches the network.
fn run_with_signer(
    sink: &Sink,
    signers: &LocalSigner,
    lnurl: &str,
    correlation_id: Option<String>,
) {
    let send = |c: ActorCommand| sink.sends.lock().unwrap().push(c);
    let clock = FixedClock(1_700_000_000);
    // Bridge the sink's stages mutex through a RecordingStages adapter.
    let stages = RecordingStages(Mutex::new(Vec::new()));
    let recipients = NoopRecipientRelayLookup;
    let mut ctx = ctx_with(&send, &clock, signers, &stages, &recipients);

    let cmd = Box::new(FetchLnurlInvoiceCommand {
        unsigned: unsigned_for(vec![
            vec!["p".to_string(), RECIPIENT_HEX.to_string()],
        ]),
        recipient_pubkey: RECIPIENT_HEX.to_string(),
        lnurl_or_address: Some(lnurl.to_string()),
        amount_msats: 21_000,
        correlation_id,
    });
    cmd.run(&mut ctx).expect("run returns Ok");
    // Forward the captured stages into the shared sink so the asserts in
    // the parent test can read them without restructuring.
    *sink.stages.lock().unwrap() = stages.0.into_inner().unwrap();
}

#[test]
fn no_account_emits_toast_and_failure_when_correlation_present() {
    // A genuinely-absent account (no local key AND no remote signer) still
    // fails closed: the sign seam returns `Err`, the command emits the toast
    // + `RecordActionFailure` before any worker spawns.
    let sink = Sink::new();
    run_with_signer(&sink, &LocalSigner::none(), UNREACHABLE_LNURL, Some("cid-none".to_string()));

    let sends = sink.sends.lock().unwrap();
    assert_eq!(sends.len(), 2, "expected ShowToast + RecordActionFailure: {sends:?}");
    match &sends[0] {
        ActorCommand::ShowToast { message } => {
            assert!(
                message.contains("no active account"),
                "toast must explain the no-account reason: {message}"
            );
        }
        other => panic!("expected ShowToast, got {other:?}"),
    }
    match &sends[1] {
        ActorCommand::RecordActionFailure { correlation_id, .. } => {
            assert_eq!(correlation_id, "cid-none");
        }
        other => panic!("expected RecordActionFailure, got {other:?}"),
    }

    let stages = sink.stages.lock().unwrap();
    assert_eq!(*stages, vec!["cid-none".to_string()], "Requested stage must record once");
}

#[test]
fn no_account_emits_only_toast_when_no_correlation_id() {
    let sink = Sink::new();
    run_with_signer(&sink, &LocalSigner::none(), UNREACHABLE_LNURL, None);

    let sends = sink.sends.lock().unwrap();
    assert_eq!(sends.len(), 1, "expected only ShowToast: {sends:?}");
    assert!(matches!(&sends[0], ActorCommand::ShowToast { .. }));
    let stages = sink.stages.lock().unwrap();
    assert!(stages.is_empty(), "no correlation_id → no Requested stage");
}

/// V-78 regression — the core fix. A NIP-46 bunker account (no local keys)
/// must NOT fail closed at dispatch: the command resolves the sign through
/// `sign_active_nonblocking` (→ `SignerOp::Pending`), spawns the off-actor
/// worker, and returns WITHOUT emitting `ShowToast`/`RecordActionFailure`
/// on the actor thread. The buggy path emitted a "zap requires a local-keys
/// account" toast here.
#[test]
fn bunker_account_does_not_fail_closed_at_dispatch() {
    let sink = Sink::new();
    let bunker = LocalSigner::bunker(Keys::generate());
    run_with_signer(&sink, &bunker, UNREACHABLE_LNURL, Some("cid-bunker".to_string()));

    // The Requested stage is recorded (the action is in flight), but NO
    // terminal failure/toast is emitted synchronously — the worker owns the
    // outcome. (The worker will fail at the live-HTTP leg in this unit test,
    // but that happens off-thread and is not captured by `sink.sends`, which
    // only sees the actor-thread `send` closure, not the worker's
    // `command_sender` clone whose receiver is dropped.)
    let sends = sink.sends.lock().unwrap();
    assert!(
        sends.is_empty(),
        "V-78: a bunker zap must NOT emit a sync fail-closed toast/failure: {sends:?}"
    );
    let stages = sink.stages.lock().unwrap();
    assert_eq!(
        *stages,
        vec!["cid-bunker".to_string()],
        "Requested stage must still record for an in-flight bunker zap"
    );
}

/// V-78 — a local-nsec account also routes through `sign_active_nonblocking`
/// (→ `SignerOp::Ready`) and likewise does not fail closed at dispatch.
#[test]
fn local_account_does_not_fail_closed_at_dispatch() {
    let sink = Sink::new();
    let local = LocalSigner::local(Keys::generate());
    run_with_signer(&sink, &local, UNREACHABLE_LNURL, Some("cid-local".to_string()));

    let sends = sink.sends.lock().unwrap();
    assert!(
        sends.is_empty(),
        "a local-keys zap must NOT emit a sync fail-closed toast/failure: {sends:?}"
    );
    let stages = sink.stages.lock().unwrap();
    assert_eq!(*stages, vec!["cid-local".to_string()]);
}

/// V-78 — `signed_event_to_nostr_json` must reproduce the EXACT flat NIP-01
/// wire bytes `sign_zap_request` emits, so a bunker-signed kind:9734 hits the
/// LN provider's callback byte-for-byte identical to a local-nsec zap. The
/// signed `nostr::Event` is flattened to `SignedEvent` and rebuilt; the two
/// serializations must be equal.
#[test]
fn signed_event_to_nostr_json_matches_sign_zap_request_bytes() {
    let keys = Keys::generate();
    let unsigned = UnsignedEvent {
        pubkey: keys.public_key().to_hex(),
        kind: 9734,
        tags: vec![
            vec!["relays".to_string(), "wss://relay.example".to_string()],
            vec!["p".to_string(), RECIPIENT_HEX.to_string()],
            vec!["amount".to_string(), "21000".to_string()],
        ],
        content: "nice post 🤙".to_string(),
        created_at: 1_700_000_000,
    };

    // The canonical local path.
    let direct = sign_zap_request(&keys, &unsigned).expect("sign must succeed");
    // Flatten that signed event into a substrate SignedEvent, then rebuild
    // the flat JSON through the V-78 helper.
    let event: nostr::Event = serde_json::from_str(&direct).expect("valid event");
    let signed = nostr_event_to_signed(&event);
    let rebuilt = signed_event_to_nostr_json(&signed).expect("rebuild must succeed");

    assert_eq!(
        direct, rebuilt,
        "the bunker-rebuilt flat NIP-01 JSON must be byte-identical to the \
         local-nsec sign output"
    );
}

#[test]
fn run_restamps_created_at_from_context_clock() {
    // Indirect: we can't observe `unsigned.created_at` after the move,
    // but we can verify the dispatch arm calls `now_secs` once when the
    // sentinel is `0`. Wire a counter through a custom clock adapter.
    use std::sync::atomic::{AtomicU64, Ordering};

    struct CountingClock(AtomicU64);
    impl KernelClock for CountingClock {
        fn now_secs(&self) -> u64 {
            self.0.fetch_add(1, Ordering::SeqCst);
            1_700_000_000
        }
    }

    let sink = Sink::new();
    let send = |c: ActorCommand| sink.sends.lock().unwrap().push(c);
    let clock = CountingClock(AtomicU64::new(0));
    let signers = LocalSigner::none();
    let stages = RecordingStages(Mutex::new(Vec::new()));
    let recipients = NoopRecipientRelayLookup;
    let mut ctx = ctx_with(&send, &clock, &signers, &stages, &recipients);

    let cmd = Box::new(FetchLnurlInvoiceCommand {
        unsigned: unsigned_for(vec![
            vec!["p".to_string(), RECIPIENT_HEX.to_string()],
        ]),
        recipient_pubkey: RECIPIENT_HEX.to_string(),
        lnurl_or_address: Some("alice@example.com".to_string()),
        amount_msats: 21_000,
        correlation_id: None,
    });
    cmd.run(&mut ctx).expect("run returns Ok on fail-closed branch");
    assert!(
        clock.0.load(Ordering::SeqCst) >= 1,
        "now_secs must be invoked when created_at sentinel is 0"
    );
}
