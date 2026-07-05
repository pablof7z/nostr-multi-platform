//! #3010 — the end-to-end acceptance test for "await recipient kind:10019
//! instead of fail-fast": a `SendNutzap` dispatched to a recipient whose
//! kind:10019 is not yet cached parks instead of failing; the moment that
//! info arrives (simulated here via `nutzap_await::NutzapInfoArrivalParser`,
//! the real kind:10019-arrival seam — see that module's docs), the send is
//! automatically redriven and settles through a REAL mock mint, with NO
//! caller retry. A companion test proves the OTHER half of the bound: a
//! recipient whose info never arrives fails closed after the TTL
//! (`nutzap_await::run_ttl_sweep`, the `Kernel`-free core of the idle-tick
//! sweep — see that module's tests for why `Kernel` itself is not needed
//! here). A third test proves at-most-once: even if the same kind:10019 is
//! ingested twice (a duplicate relay delivery), the parked send is redriven
//! exactly once.

use super::*;
use nmp_core::substrate::IngestParser;
use nmp_store::{RawEvent, VerifiedEvent};

const ACCOUNT: &str = "e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5";
const RECIPIENT: &str = "f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6";
const AMOUNT_SATS: u64 = 21;

fn ctx(account_pubkey: Option<&str>) -> WalletBackendContext<'_> {
    WalletBackendContext {
        now_secs: 1_700_000_000,
        selected_backend: None,
        account_pubkey,
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn dummy_pubkey() -> nostr::secp256k1::PublicKey {
    let sk = nostr::secp256k1::SecretKey::new(&mut nostr::secp256k1::rand::thread_rng());
    nostr::secp256k1::PublicKey::from_secret_key(&nostr::secp256k1::Secp256k1::new(), &sk)
}

fn keyset_response_body(mint_pk: &nostr::secp256k1::PublicKey) -> String {
    let mut keys = serde_json::Map::new();
    for bit in 0..16u64 {
        keys.insert(
            (1u64 << bit).to_string(),
            serde_json::Value::String(hex_encode(&mint_pk.serialize())),
        );
    }
    serde_json::json!({
        "keysets": [{"id": "00keyset", "unit": "sat", "keys": keys, "input_fee_ppk": 0}]
    })
    .to_string()
}

fn echo_signatures(body: &[u8]) -> String {
    let request: serde_json::Value =
        serde_json::from_slice(body).expect("valid blinded-outputs request");
    let signatures: Vec<serde_json::Value> = request["outputs"]
        .as_array()
        .expect("outputs array")
        .iter()
        .map(|out| serde_json::json!({"amount": out["amount"], "id": out["id"], "C_": out["B_"]}))
        .collect();
    serde_json::json!({ "signatures": signatures }).to_string()
}

/// A mint that serves exactly what a `SendNutzap` worker needs: `get_sat_
/// keyset` (keys + keysets) then a REAL echoed `/v1/swap` signature (the
/// swap step needs the actual request's blinded outputs to build a
/// structurally-valid response — a static canned body would fail
/// `finalize_swap_response`'s validation, mirrors `cross_mint_headroom_
/// fallback_support.rs`'s identical `echo_signatures` shortcut).
fn spawn_send_mint_scripted() -> String {
    use std::net::TcpListener;
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind send mint");
    let addr = listener.local_addr().expect("local_addr");
    let url = format!("http://{addr}");
    std::thread::spawn(move || {
        let mint_pk = dummy_pubkey();
        let keys_body = keyset_response_body(&mint_pk);
        for _ in 0..2 {
            if let Ok((mut stream, _)) = listener.accept() {
                let _ = read_http_request(&mut stream);
                write_http_response(&mut stream, 200, &keys_body);
            }
        }
        if let Ok((mut stream, _)) = listener.accept() {
            let (buf, header_end) = read_http_request(&mut stream);
            let body = echo_signatures(&buf[header_end..]);
            write_http_response(&mut stream, 200, &body);
        }
    });
    url
}

fn recipient_info(mint: &str) -> nmp_nip60::nutzap::NutZapInfo {
    nmp_nip60::nutzap::NutZapInfo {
        relays: vec!["wss://recipient-relay.example".to_string()],
        mints: vec![mint.to_string()],
        cashu_pubkey: Some("02".to_string() + &"44".repeat(32)),
    }
}

fn verified_kind_10019(pubkey: &str) -> VerifiedEvent {
    VerifiedEvent::from_raw_unchecked(RawEvent {
        id: "00".repeat(32),
        pubkey: pubkey.to_string(),
        created_at: 1_700_000_100,
        kind: nmp_nip60::kinds::KIND_NIP61_NUTZAP_INFO,
        tags: Vec::new(),
        content: String::new(),
        sig: "22".repeat(64),
    })
}

/// The full acceptance scenario: dispatch `SendNutzap` to a recipient with no
/// cached kind:10019 -> it parks (no error surfaced, no caller retry needed)
/// -> the recipient's kind:10019 "arrives" (the real ingest-parser seam) ->
/// the send is automatically redriven under the SAME correlation id -> it
/// reaches the mint and settles.
#[test]
fn send_proceeds_and_settles_once_recipient_info_arrives_without_caller_retry() {
    let mint_url = spawn_send_mint_scripted();

    let backend = CashuWalletBackend::new();
    {
        let mut state = state::lock_state(&backend.state);
        state.mints = vec![mint_url.clone()];
        state.add_proofs(
            None,
            mint_url.clone(),
            vec![synthetic_proof(100, &("02".to_string() + &"aa".repeat(32)))],
        );
    }

    // 1. Dispatch with NO cached recipient info -> parks.
    let commands = backend.start_intent(
        ctx(Some(ACCOUNT)),
        WalletIntent::SendNutzap {
            recipient_pubkey: RECIPIENT.to_string(),
            amount_sats: AMOUNT_SATS,
            target_event_id: None,
        },
        Some("cid-await-arrival".to_string()),
    );
    let ActorCommand::Protocol(cmd) = commands.into_iter().next().unwrap() else {
        panic!("expected a Protocol command");
    };
    let sink = Sink::new();
    let send = |c: ActorCommand| sink.sends.lock().unwrap().push(c);
    let clock = FixedClock(1_700_000_000);
    let recipients = FixedRecipientLookup(Vec::new());
    let errors = RecordingErrorSurface::default();
    let no_info = FixedCachedEvents::default();
    let (worker_tx, worker_rx) = std::sync::mpsc::channel::<nmp_core::ActorMail>();
    {
        let mut c = ctx_with_cached_events_and_errors(
            &send,
            nmp_core::CommandSender::new(worker_tx.clone()),
            &clock,
            &recipients,
            &no_info,
            &errors,
        );
        cmd.run(&mut c).expect("run returns Ok");
    }
    assert_eq!(
        errors.last_token_code.lock().unwrap().as_deref(),
        None,
        "no error must be surfaced on a miss — it parks instead"
    );

    // 2. Simulate the recipient's kind:10019 arriving — the real ingest seam.
    let arrival_parser = nutzap_await::NutzapInfoArrivalParser {
        state: std::sync::Arc::clone(&backend.state),
        tx: nmp_core::CommandSender::new(worker_tx),
    };
    arrival_parser.parse_at(&verified_kind_10019(RECIPIENT), 1_700_000_100);

    // 3. Exactly one redriven command must have been enqueued.
    let redriven = match recv_command(&worker_rx) {
        ActorCommand::Protocol(cmd) => cmd,
        other => panic!("expected the redriven SendNutzapCommand, got {other:?}"),
    };

    // 4. Run the redrive with the recipient's info NOW cached (exactly what
    // the kernel's own cache reflects by the time an `IngestParser` fires —
    // see `nutzap_await.rs`'s module docs on ordering) — it must reach the
    // mint and settle.
    let cached = FixedCachedEvents(vec![nutzap_info_kernel_event(
        RECIPIENT,
        &recipient_info(&mint_url),
        1_700_000_100,
    )]);
    let (worker_tx2, worker_rx2) = std::sync::mpsc::channel::<nmp_core::ActorMail>();
    {
        let mut c = ctx_with_cached_events(
            &send,
            nmp_core::CommandSender::new(worker_tx2),
            &clock,
            &recipients,
            &cached,
        );
        redriven.run(&mut c).expect("run returns Ok");
    }
    let sign_continuation = match recv_command(&worker_rx2) {
        ActorCommand::Sign(nmp_core::actor::SignCommand::EventForAccount {
            unsigned,
            continuation,
            ..
        }) => {
            assert_eq!(unsigned.kind, nmp_nip60::kinds::KIND_NIP61_NUTZAP);
            continuation
        }
        other => panic!("expected the redriven send's kind:9321 sign, got {other:?}"),
    };
    sign_continuation.call(Ok(nmp_signer_iface::SignedEvent {
        id: "1".repeat(64),
        sig: "2".repeat(128),
        unsigned: nmp_signer_iface::UnsignedEvent {
            pubkey: ACCOUNT.to_string(),
            kind: nmp_nip60::kinds::KIND_NIP61_NUTZAP,
            tags: Vec::new(),
            content: String::new(),
            created_at: 1_700_000_100,
        },
    }));
    match recv_command(&worker_rx2) {
        ActorCommand::Publish(_) => {}
        other => panic!("expected the kind:9321 publish, got {other:?}"),
    }

    // The redriven operation (a FRESH id, distinct from the superseded
    // original) reached `Settled` — never the caller retrying, never a second
    // attempt at the SAME (superseded) operation id.
    let state = state::lock_state(&backend.state);
    let original = state
        .journal
        .get(&crate::journal::WalletOperationId::new("cid-await-arrival"))
        .expect("original operation recorded");
    assert_eq!(
        original.state,
        WalletOperationState::Failed,
        "the original attempt stays superseded"
    );
    let settled = state
        .journal
        .pending_operations()
        .iter()
        .chain(state.journal.terminal_operations().iter())
        .find(|op| {
            op.kind == WalletOperationKind::SendNutzap && op.id.as_str() != "cid-await-arrival"
        })
        .cloned()
        .expect("the redriven operation must be recorded under a fresh id");
    assert_eq!(settled.state, WalletOperationState::Settled);
}

/// The other half of the bound: a recipient whose kind:10019 never arrives
/// fails closed once the TTL sweep runs, rather than leaving the caller's
/// correlation id hanging forever.
#[test]
fn send_fails_closed_after_the_bound_when_recipient_info_never_arrives() {
    let backend = CashuWalletBackend::new();
    lock_state(&backend.state).mints = vec!["https://unused-mint.example".to_string()];

    let commands = backend.start_intent(
        ctx(Some(ACCOUNT)),
        WalletIntent::SendNutzap {
            recipient_pubkey: RECIPIENT.to_string(),
            amount_sats: AMOUNT_SATS,
            target_event_id: None,
        },
        Some("cid-never-arrives".to_string()),
    );
    let ActorCommand::Protocol(cmd) = commands.into_iter().next().unwrap() else {
        panic!("expected a Protocol command");
    };
    let sink = Sink::new();
    let send = |c: ActorCommand| sink.sends.lock().unwrap().push(c);
    let clock = FixedClock(1_700_000_000);
    let recipients = FixedRecipientLookup(Vec::new());
    let errors = RecordingErrorSurface::default();
    let no_info = FixedCachedEvents::default();
    let (worker_tx, worker_rx) = std::sync::mpsc::channel::<nmp_core::ActorMail>();
    {
        let mut c = ctx_with_cached_events_and_errors(
            &send,
            nmp_core::CommandSender::new(worker_tx.clone()),
            &clock,
            &recipients,
            &no_info,
            &errors,
        );
        cmd.run(&mut c).expect("run returns Ok");
    }
    assert_eq!(errors.last_token_code.lock().unwrap().as_deref(), None);

    // Fast-forward past the wait bound with no arrival — the sweep fails it
    // closed.
    nutzap_await::run_ttl_sweep(
        &backend.state,
        &nmp_core::CommandSender::new(worker_tx),
        1_700_000_000 + nutzap_await::NUTZAP_INFO_AWAIT_TIMEOUT_SECS + 1,
    );

    match recv_command(&worker_rx) {
        ActorCommand::ShowErrorToken { token } => {
            assert_eq!(token.code(), ui_codes::NO_RECIPIENT_NUTZAP_INFO);
        }
        other => panic!("expected ShowErrorToken(NO_RECIPIENT_NUTZAP_INFO), got {other:?}"),
    }
    match recv_command(&worker_rx) {
        ActorCommand::ActionLedger(nmp_core::actor::ActionLedgerCommand::RecordFailure {
            correlation_id,
            ..
        }) => {
            assert_eq!(correlation_id, "cid-never-arrives");
        }
        other => panic!("expected RecordFailure(cid-never-arrives), got {other:?}"),
    }
}

/// At-most-once, driven through the REAL backend (not just the isolated
/// `NutzapInfoArrivalParser` unit tests in `nutzap_await_tests.rs`): a
/// duplicate delivery of the recipient's kind:10019 (e.g. from a second
/// relay) after the first arrival has already redriven the parked send must
/// enqueue nothing further — the money-safety invariant that a cross-mint
/// melt (or any send) is never driven twice for one parked attempt.
#[test]
fn duplicate_kind_10019_arrival_never_redrives_a_second_time() {
    let backend = CashuWalletBackend::new();
    lock_state(&backend.state).mints = vec!["https://unused-mint.example".to_string()];

    let commands = backend.start_intent(
        ctx(Some(ACCOUNT)),
        WalletIntent::SendNutzap {
            recipient_pubkey: RECIPIENT.to_string(),
            amount_sats: AMOUNT_SATS,
            target_event_id: None,
        },
        Some("cid-at-most-once".to_string()),
    );
    let ActorCommand::Protocol(cmd) = commands.into_iter().next().unwrap() else {
        panic!("expected a Protocol command");
    };
    let sink = Sink::new();
    let send = |c: ActorCommand| sink.sends.lock().unwrap().push(c);
    let clock = FixedClock(1_700_000_000);
    let recipients = FixedRecipientLookup(Vec::new());
    let errors = RecordingErrorSurface::default();
    let no_info = FixedCachedEvents::default();
    let (worker_tx, worker_rx) = std::sync::mpsc::channel::<nmp_core::ActorMail>();
    {
        let mut c = ctx_with_cached_events_and_errors(
            &send,
            nmp_core::CommandSender::new(worker_tx.clone()),
            &clock,
            &recipients,
            &no_info,
            &errors,
        );
        cmd.run(&mut c).expect("run returns Ok");
    }

    let arrival_parser = nutzap_await::NutzapInfoArrivalParser {
        state: std::sync::Arc::clone(&backend.state),
        tx: nmp_core::CommandSender::new(worker_tx),
    };
    // First arrival redrives the parked send exactly once.
    arrival_parser.parse_at(&verified_kind_10019(RECIPIENT), 1_700_000_100);
    match recv_command(&worker_rx) {
        ActorCommand::Protocol(_) => {}
        other => panic!("expected the redriven SendNutzapCommand, got {other:?}"),
    }

    // A duplicate delivery of the SAME kind:10019 (second relay, or a
    // re-observed EOSE replay) must find nothing left parked.
    arrival_parser.parse_at(&verified_kind_10019(RECIPIENT), 1_700_000_101);
    assert!(
        worker_rx
            .recv_timeout(std::time::Duration::from_millis(200))
            .is_err(),
        "a duplicate arrival must never redrive a second time"
    );
}
