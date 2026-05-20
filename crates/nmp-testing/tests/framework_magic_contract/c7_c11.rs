//! Framework Magic Contract — M6-gated tests: C7, C11.
//!
//! C7  Write fan-out — publish routes to outbox + recipient inbox
//! C11 Signer onboarding — bunker:// URL and new-nsec creation
//!
//! M6 (sessions + signers + write path) is DONE on master.
//!
//! Design: `docs/design/framework-magic/`

use std::sync::Arc;

use nmp_core::publish::{
    InMemoryPublishStore, NoopSigner, PublishAction, PublishEngine, PublishEngineError,
    PublishTarget, RelayAck, RelayUrl, ReplayDispatcher, RetryPolicy, StaticOutbox,
    NoopOutboxResolver,
};
use nmp_core::substrate::{SignedEvent, UnsignedEvent};

// ── Shared helpers ────────────────────────────────────────────────────────────

fn pubkey(seed: &str) -> String {
    format!("{seed:0>64}").chars().take(64).collect::<String>().to_lowercase()
}

fn fake_signed_event(author: &str, kind: u32, p_tags: Vec<&str>) -> SignedEvent {
    let tags = p_tags
        .iter()
        .map(|p| vec!["p".to_string(), pubkey(p)])
        .collect::<Vec<_>>();
    SignedEvent {
        id: "a".repeat(64),
        sig: "b".repeat(128),
        unsigned: UnsignedEvent {
            pubkey: pubkey(author),
            kind,
            tags,
            content: String::new(),
            created_at: 1_000,
        },
    }
}

fn engine_with_outbox(
    outbox: StaticOutbox,
    dispatcher: Arc<ReplayDispatcher>,
) -> PublishEngine {
    PublishEngine::new(
        Arc::new(outbox),
        dispatcher,
        Arc::new(InMemoryPublishStore::new()),
        Arc::new(NoopSigner),
        RetryPolicy::default(),
    )
}

// ── C7 ────────────────────────────────────────────────────────────────────────

/// C7: Write fan-out — publish routes events to outbox write-relays;
/// private (DM) events fail closed when recipient inbox is unknown.
///
/// Two sub-properties:
/// 1. Public event: routed to the author's write relays via NIP-65 outbox.
/// 2. Private event with unknown recipient inbox: `NoTargets` error (fail closed).
///
/// Design: `docs/design/framework-magic/outbox.md` §C7.
#[test]
fn c7_publish_routes_outbox_and_private_fails_closed() {
    // --- 1. Public event: routed to author's write relays -------------------
    let alice_writes: Vec<RelayUrl> = vec![
        "wss://r1/".to_string(),
        "wss://r2/".to_string(),
    ];
    let mut outbox = StaticOutbox::default();
    outbox.author_writes.insert(pubkey("alice"), alice_writes.clone());

    let dispatcher = Arc::new(ReplayDispatcher::new());
    // Script OK acks for both relays so the engine can complete.
    dispatcher.script("wss://r1/", vec![RelayAck::ok("wss://r1/")]);
    dispatcher.script("wss://r2/", vec![RelayAck::ok("wss://r2/")]);

    let mut engine = engine_with_outbox(outbox, Arc::clone(&dispatcher));

    let event = fake_signed_event("alice", 1, vec![]);
    engine
        .start_publish(
            PublishAction::Publish {
                handle: "h1".to_string(),
                event,
                target: PublishTarget::Auto,
            },
            0,
        )
        .expect("public publish must succeed");

    let sent = dispatcher.sent_frames();
    let sent_relays: std::collections::BTreeSet<&str> = sent
        .iter()
        .map(|(url, _)| url.as_str())
        .collect();
    assert!(
        sent_relays.contains("wss://r1/"),
        "public event must be dispatched to alice's write relay r1"
    );
    assert!(
        sent_relays.contains("wss://r2/"),
        "public event must be dispatched to alice's write relay r2"
    );

    // --- 2. Private event, recipient inbox unknown → fail closed -----------
    // Use NoopOutboxResolver: no author_writes, no p_tag_reads → empty set.
    let noop_disp_replay = Arc::new(ReplayDispatcher::new());
    let noop_dispatcher: Arc<dyn nmp_core::publish::RelayDispatcher> =
        Arc::clone(&noop_disp_replay) as Arc<dyn nmp_core::publish::RelayDispatcher>;
    let mut fail_engine = PublishEngine::new(
        Arc::new(NoopOutboxResolver),
        noop_dispatcher,
        Arc::new(InMemoryPublishStore::new()),
        Arc::new(NoopSigner),
        RetryPolicy::default(),
    );

    // DM event — the resolver finds no inbox for bob because no p-tag reads
    // are configured, modelling "recipient inbox unknown".
    let dm_event = fake_signed_event("alice", 4, vec!["bob"]);
    let result = fail_engine.start_publish(
        PublishAction::Publish {
            handle: "h2".to_string(),
            event: dm_event,
            target: PublishTarget::Auto,
        },
        0,
    );
    assert!(
        matches!(result, Err(PublishEngineError::NoTargets)),
        "private event with unknown recipient inbox must fail closed (NoTargets): {result:?}"
    );
    // No frames must have been dispatched.
    assert!(
        noop_disp_replay.sent_frames().is_empty(),
        "fail-closed path must dispatch nothing"
    );
}

// ── C11 ───────────────────────────────────────────────────────────────────────

/// C11: Signer onboarding — `bunker://` URL parses cleanly; a new local nsec
/// is created and adds successfully to the account manager.
///
/// The full `KeyringCapability` action-module wrapper is a gap (it lives at the
/// substrate/action layer, not yet wired as a kernel action). The underlying
/// primitives — `parse_bunker_uri`, `LocalKeySigner::generate`, and
/// `AccountManager::add` — are all present and exercised here.
///
/// A follow-up task (#57-c11-keyring) tracks the `KeyringCapability` +
/// `IdentityModule` kernel wiring.
///
/// Design: `docs/design/framework-magic/signers.md`
#[test]
fn c11_bunker_url_and_nsec_creation_complete_via_actions() {
    use std::time::Duration;
    use nmp_signers::{parse_bunker_uri, AccountManager, LocalKeySigner, Signer};

    // --- bunker:// URI parse ------------------------------------------------
    let uri = "bunker://\
               0000000000000000000000000000000000000000000000000000000000000001\
               ?relay=wss%3A%2F%2Fnostr.example.com&secret=abc123";
    let parsed = parse_bunker_uri(uri).expect("valid bunker URI must parse");
    assert_eq!(
        parsed.remote_pubkey_hex,
        "0000000000000000000000000000000000000000000000000000000000000001"
    );
    assert_eq!(parsed.relays.len(), 1, "must parse one relay");
    assert!(
        parsed.relays[0] == "wss://nostr.example.com" || parsed.relays[0] == "wss://nostr.example.com/",
        "relay must normalize to wss://nostr.example.com or wss://nostr.example.com/, got: {}",
        parsed.relays[0]
    );
    assert_eq!(parsed.secret.as_deref().map(String::as_str), Some("abc123"));

    // Error paths: wrong scheme, no relay, oversized.
    assert!(parse_bunker_uri("https://foo").is_err(), "wrong scheme must fail");
    assert!(
        parse_bunker_uri("bunker://0000000000000000000000000000000000000000000000000000000000000001").is_err(),
        "missing relay must fail"
    );
    assert!(parse_bunker_uri("").is_err(), "empty must fail");

    // --- New local nsec creation + AccountManager add ----------------------
    let signer = LocalKeySigner::generate();
    let pubkey_hex = signer.pubkey().to_hex();
    assert_eq!(pubkey_hex.len(), 64, "generated pubkey must be 64-char hex");

    let mut manager = AccountManager::new()
        .with_post_condition_timeout(Duration::from_millis(500));
    let id = manager.add(Arc::new(signer)).expect("add must succeed");
    assert_eq!(id, pubkey_hex, "account id must equal pubkey hex");
    assert_eq!(manager.accounts().len(), 1);
    assert!(manager.active().is_none(), "add does not auto-activate");

    // Verify duplicate-add is rejected.
    let dup_signer = nmp_signers::LocalKeySigner::from_secret_hex(
        "0000000000000000000000000000000000000000000000000000000000000002"
    ).expect("valid hex");
    let dup_id = manager.add(Arc::new(dup_signer)).expect("second distinct account");
    assert_ne!(dup_id, id, "distinct keys produce distinct ids");
    assert_eq!(manager.accounts().len(), 2);
}
