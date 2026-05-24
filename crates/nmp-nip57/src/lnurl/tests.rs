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
    NoopRecipientRelayLookup, RecipientRelayLookup,
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

struct LocalSigner(Option<Keys>);
impl LocalSignerAccess for LocalSigner {
    fn active_local_keys(&self) -> Option<Keys> {
        self.0.clone()
    }
    fn signer_for_seal(
        &self,
    ) -> Option<std::sync::Arc<dyn nmp_core::substrate::SignerForSeal>> {
        None
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
    ProtocolCommandContext::new(
        send,
        tx,
        clock,
        signers,
        &EMPTY_DM,
        &ERRORS,
        stages,
        recipients,
    )
}

// ────────────────────────────────────────────────────────────────────
// V-07 — recipient relay injection through the protocol context.
// ────────────────────────────────────────────────────────────────────

#[test]
fn inject_recipient_relays_preserves_existing_relays_tag() {
    let send = |_: ActorCommand| {};
    let clock = FixedClock(1_700_000_000);
    let signers = LocalSigner(None);
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
    let signers = LocalSigner(None);
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
    let signers = LocalSigner(None);
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
    let signers = LocalSigner(None);
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
    let signers = LocalSigner(None);
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
// `FetchLnurlInvoiceCommand::run` — sync-path fail branches.
//
// The HTTP-success leg requires a live LN provider; the iOS shell drives
// that end-to-end. The sync-path branches below are what we can pin from
// the unit-test level: bunker (no local keys), sign-step failure, and
// stage tracking against `correlation_id`.
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

fn run_with_no_local_keys(sink: &Sink, correlation_id: Option<String>) {
    let send = |c: ActorCommand| sink.sends.lock().unwrap().push(c);
    let clock = FixedClock(1_700_000_000);
    let signers = LocalSigner(None);
    // Bridge the sink's stages mutex through a RecordingStages adapter.
    let stages = RecordingStages(Mutex::new(Vec::new()));
    let recipients = NoopRecipientRelayLookup;
    let mut ctx = ctx_with(&send, &clock, &signers, &stages, &recipients);

    let cmd = Box::new(FetchLnurlInvoiceCommand {
        unsigned: unsigned_for(vec![
            vec!["p".to_string(), RECIPIENT_HEX.to_string()],
        ]),
        lnurl_or_address: "alice@example.com".to_string(),
        amount_msats: 21_000,
        correlation_id,
    });
    cmd.run(&mut ctx).expect("run returns Ok on fail-closed branch");
    // Forward the captured stages into the shared sink so the asserts in
    // the parent test can read them without restructuring.
    *sink.stages.lock().unwrap() = stages.0.into_inner().unwrap();
}

#[test]
fn no_local_keys_emits_toast_and_failure_when_correlation_present() {
    let sink = Sink::new();
    run_with_no_local_keys(&sink, Some("cid-bunker".to_string()));

    let sends = sink.sends.lock().unwrap();
    assert_eq!(sends.len(), 2, "expected ShowToast + RecordActionFailure: {sends:?}");
    match &sends[0] {
        ActorCommand::ShowToast { message } => {
            assert!(
                message.contains("bunker") || message.contains("local-keys"),
                "toast must explain the bunker fail-closed reason: {message}"
            );
        }
        other => panic!("expected ShowToast, got {other:?}"),
    }
    match &sends[1] {
        ActorCommand::RecordActionFailure { correlation_id, .. } => {
            assert_eq!(correlation_id, "cid-bunker");
        }
        other => panic!("expected RecordActionFailure, got {other:?}"),
    }

    let stages = sink.stages.lock().unwrap();
    assert_eq!(*stages, vec!["cid-bunker".to_string()], "Requested stage must record once");
}

#[test]
fn no_local_keys_emits_only_toast_when_no_correlation_id() {
    let sink = Sink::new();
    run_with_no_local_keys(&sink, None);

    let sends = sink.sends.lock().unwrap();
    assert_eq!(sends.len(), 1, "expected only ShowToast: {sends:?}");
    assert!(matches!(&sends[0], ActorCommand::ShowToast { .. }));
    let stages = sink.stages.lock().unwrap();
    assert!(stages.is_empty(), "no correlation_id → no Requested stage");
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
    let signers = LocalSigner(None);
    let stages = RecordingStages(Mutex::new(Vec::new()));
    let recipients = NoopRecipientRelayLookup;
    let mut ctx = ctx_with(&send, &clock, &signers, &stages, &recipients);

    let cmd = Box::new(FetchLnurlInvoiceCommand {
        unsigned: unsigned_for(vec![
            vec!["p".to_string(), RECIPIENT_HEX.to_string()],
        ]),
        lnurl_or_address: "alice@example.com".to_string(),
        amount_msats: 21_000,
        correlation_id: None,
    });
    cmd.run(&mut ctx).expect("run returns Ok on fail-closed branch");
    assert!(
        clock.0.load(Ordering::SeqCst) >= 1,
        "now_secs must be invoked when created_at sentinel is 0"
    );
}
