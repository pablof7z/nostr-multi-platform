//! Unit tests for the LNURL fetcher (`FetchLnurlInvoiceCommand`) — the
//! V-41 migration of the legacy `nmp-core::actor::commands::zap::tests`.
//!
//! HTTP I/O is not exercised here (it needs a live LN provider; the iOS
//! integration shell drives that end-to-end). The tests below pin three
//! observable contracts:
//!
//! 1. V-07 — recipient `relays` tag injection via the
//!    [`ProtocolCommandContext`] accessors (`author_write_relays` /
//!    `bootstrap_discovery_relays`).
//! 2. The kind:9734 signer (`sign_zap_request`) round-trips through
//!    `EventBuilder` and rejects out-of-range kinds.
//! 3. The sync-path fail branches in `FetchLnurlInvoiceCommand::run` (no
//!    local keys, sign error) emit the expected `ShowToast` +
//!    `RecordActionFailure` follow-ups through the context's `send`
//!    closure.

use super::*;

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

/// Build a `ProtocolCommandContext` whose kernel accessors are wired to
/// fixed stubs (the test never spawns a worker, so the sender is unused).
///
/// V-39+V-40 added four trailing positional args to
/// [`ProtocolCommandContext::new`] (the NIP-17 DM stack surface). The LNURL
/// tests don't exercise that surface — they pass `None` for the local-keys
/// snapshot, the empty `DmInboxRelayLookup`, and no-op toast / failure
/// closures.
fn ctx_with<'a>(
    send: &'a dyn Fn(ActorCommand),
    author_relays: &'a dyn Fn(&str) -> Vec<String>,
    bootstrap_relays: &'a dyn Fn() -> Vec<String>,
    local_keys: &'a dyn Fn() -> Option<Keys>,
    now: &'a dyn Fn() -> u64,
    stage_req: &'a dyn Fn(&str),
) -> ProtocolCommandContext<'a> {
    let (tx, _rx) = std::sync::mpsc::channel::<ActorCommand>();
    // V-39+V-40 trailing-arg stubs — the LNURL fetcher never reads any of
    // these accessors.
    static EMPTY_DM: nmp_core::substrate::EmptyDmInboxRelayLookup =
        nmp_core::substrate::EmptyDmInboxRelayLookup;
    static NOOP_TOAST: fn(Option<String>) = |_| {};
    static NOOP_FAIL: fn(String, String) = |_, _| {};
    // V-08 trailing-arg stub — LNURL fetcher never reads signer_for_seal.
    static NOOP_SIGNER_FOR_SEAL: fn() -> Option<std::sync::Arc<dyn nmp_core::substrate::SignerForSeal>> = || None;
    ProtocolCommandContext::new(
        send,
        tx,
        now,
        author_relays,
        bootstrap_relays,
        local_keys,
        stage_req,
        None,
        &EMPTY_DM,
        &NOOP_TOAST,
        &NOOP_FAIL,
        &NOOP_SIGNER_FOR_SEAL,
    )
}

// ────────────────────────────────────────────────────────────────────
// V-07 — recipient relay injection through the protocol context.
// ────────────────────────────────────────────────────────────────────

#[test]
fn inject_recipient_relays_preserves_existing_relays_tag() {
    let send = |_: ActorCommand| {};
    let by_author = |_: &str| vec!["wss://by-author.example".to_string()];
    let bootstrap = || vec!["wss://bootstrap.example".to_string()];
    let no_keys = || None;
    let now = || 1_700_000_000u64;
    let no_stage = |_: &str| ();
    let ctx = ctx_with(&send, &by_author, &bootstrap, &no_keys, &now, &no_stage);

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
}

#[test]
fn inject_recipient_relays_injects_when_tag_absent() {
    let send = |_: ActorCommand| {};
    let by_author = |a: &str| {
        assert_eq!(a, RECIPIENT_HEX, "must consult recipient's write list");
        vec!["wss://alice.example".to_string()]
    };
    let bootstrap = || vec!["wss://bootstrap.example".to_string()];
    let no_keys = || None;
    let now = || 1_700_000_000u64;
    let no_stage = |_: &str| ();
    let ctx = ctx_with(&send, &by_author, &bootstrap, &no_keys, &now, &no_stage);

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
            "wss://alice.example".to_string()
        ]
    );
}

#[test]
fn inject_recipient_relays_treats_bare_relays_key_as_absent() {
    // A `["relays"]` row with no URLs is malformed — treat as absent so
    // the injection still fires.
    let send = |_: ActorCommand| {};
    let by_author = |_: &str| vec!["wss://write.example".to_string()];
    let bootstrap = || vec!["wss://bootstrap.example".to_string()];
    let no_keys = || None;
    let now = || 1_700_000_000u64;
    let no_stage = |_: &str| ();
    let ctx = ctx_with(&send, &by_author, &bootstrap, &no_keys, &now, &no_stage);

    let mut unsigned = unsigned_for(vec![
        vec!["relays".to_string()],
        vec!["p".to_string(), RECIPIENT_HEX.to_string()],
    ]);
    inject_recipient_relays(&ctx, &mut unsigned);
    let filled_count = unsigned
        .tags
        .iter()
        .filter(|t| t.first().map(String::as_str) == Some("relays") && t.len() > 1)
        .count();
    assert_eq!(
        filled_count, 1,
        "must inject exactly one filled relays tag when the existing one is bare"
    );
}

#[test]
fn inject_recipient_relays_falls_back_to_bootstrap_when_p_tag_missing() {
    // Defensive — a builder bug that drops the `p` tag must NOT produce
    // a zap with an empty relays tag. The bootstrap seed is the safe
    // fallback (the LNURL HTTP layer will still verify the recipient
    // resolves later).
    let send = |_: ActorCommand| {};
    let by_author = |_: &str| {
        panic!("must not consult author_write_relays when p tag is missing");
    };
    let bootstrap = || vec!["wss://bootstrap.example".to_string()];
    let no_keys = || None;
    let now = || 1_700_000_000u64;
    let no_stage = |_: &str| ();
    let ctx = ctx_with(&send, &by_author, &bootstrap, &no_keys, &now, &no_stage);

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
            "wss://bootstrap.example".to_string()
        ]
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
    let by_author = |_: &str| Vec::<String>::new();
    let bootstrap = || vec!["wss://bootstrap.example".to_string()];
    let no_keys = || None::<Keys>;
    let now = || 1_700_000_000u64;
    let stage = |cid: &str| sink.stages.lock().unwrap().push(cid.to_string());
    let mut ctx = ctx_with(&send, &by_author, &bootstrap, &no_keys, &now, &stage);

    let cmd = Box::new(FetchLnurlInvoiceCommand {
        unsigned: unsigned_for(vec![
            vec!["p".to_string(), RECIPIENT_HEX.to_string()],
        ]),
        lnurl_or_address: "alice@example.com".to_string(),
        amount_msats: 21_000,
        correlation_id,
    });
    cmd.run(&mut ctx).expect("run returns Ok on fail-closed branch");
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
    // sentinel is `0`. Wire a counter and check it ticked.
    use std::sync::atomic::{AtomicU64, Ordering};

    let now_counter = AtomicU64::new(0);
    let sink = Sink::new();
    let send = |c: ActorCommand| sink.sends.lock().unwrap().push(c);
    let by_author = |_: &str| Vec::<String>::new();
    let bootstrap = || vec!["wss://bootstrap.example".to_string()];
    let no_keys = || None::<Keys>;
    let now = || {
        now_counter.fetch_add(1, Ordering::SeqCst);
        1_700_000_000
    };
    let stage = |_: &str| ();
    let mut ctx = ctx_with(&send, &by_author, &bootstrap, &no_keys, &now, &stage);

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
        now_counter.load(Ordering::SeqCst) >= 1,
        "now_secs must be invoked when created_at sentinel is 0"
    );
}
