//! Unit tests for the EOSE / OK / NOTICE relay-frame ingest handlers.
//!
//! These three frame kinds are handled inline in `kernel/ingest/mod.rs`'s
//! `handle_text` dispatch (they are *not* separate `ingest/*.rs` files — only
//! the kind-specific *event* handlers, kind:0/3/1/6/10002, get their own
//! files). The tests drive the production `handle_text` / `handle_message`
//! seam directly, exactly as `t140_m1_retirement_tests` and
//! `t170_relay_scoped_keying_tests` do, so there is no mock surface and the
//! wire-shape → state-mutation contract is exercised end to end.
//!
//! ## Scope vs. the existing suites — no duplication
//!
//! - **EOSE**: `t140_m1_retirement_tests` / `t170_relay_scoped_keying_tests`
//!   cover only the *keep-live* branch (follow-feed `sub-*` subs stay `live`).
//!   The tests here cover the *inverse* branches that had no coverage: an EOSE
//!   for an **unknown** sub (graceful no-op) and an EOSE for a **known
//!   non-persistent** sub (marked closed → CLOSE frame emitted → `wire_subs`
//!   row evicted), plus the `eose_rx` counter increment.
//! - **OK**: `publish_engine_tests` / `publish_terminal_status_tests` cover the
//!   publish FSM exhaustively, but they all call `handle_publish_ok_at`
//!   *directly* — they never go through the `["OK", id, bool, msg]` JSON
//!   wire-frame parse. The OK tests here cover only that un-tested seam:
//!   `handle_text` → `route_publish_ok` (`parse_ok_frame`) → engine. The FSM
//!   itself is not re-tested.
//! - **NOTICE**: zero prior coverage anywhere.
//!
//! No signing is needed: EOSE / NOTICE carry no event; the OK wire-seam tests
//! seed a publish via the same `fake_signed` shape `publish_engine_tests` uses
//! (the publish engine never re-verifies the signature past `dispatch`).

use crate::kernel::Kernel;
use crate::kernel::RelayFrame;
use crate::publish::PublishTarget;
use crate::relay::{RelayRole, DEFAULT_VISIBLE_LIMIT};
use crate::store::{RawEvent, VerifiedEvent};
use crate::subs::WireFrame;
use crate::substrate::{SignedEvent, UnsignedEvent};

use crate::planner::{InterestId, InterestLifecycle};

// Canonical URL form (no empty-path trailing slash). The kernel keys
// `wire_subs` / `persistent_subs` — and the EOSE-close `OutboundMessage` —
// by the canonical relay URL (T-relay-url-normalize: `req_for_relay`, the
// planner boundary, and the EOSE/CLOSED handler all canonicalize).
const RELAY: &str = "wss://relay.eon-test";
const WRITE_R1: &str = "wss://write-r1.eon-test";

// ─── EOSE ────────────────────────────────────────────────────────────────────

/// An EOSE for a `sub_id` the kernel never opened must not mutate kernel
/// state: no panic, no synthesized `wire_subs` row. The diagnostic `eose_rx`
/// counter still increments (the frame *was* received and observed), and the
/// handler routes a defensive `CLOSE` back on the delivering socket (the
/// `close_url` fallback) so a stray relay-side sub is torn down.
#[test]
fn eose_for_unknown_sub_does_not_mutate_state_but_counts_and_closes() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let before = kernel.relay(RelayRole::Content).counters.eose_rx;

    let eose = serde_json::json!(["EOSE", "sub-never-opened"]).to_string();
    let outbound = kernel.handle_message(RelayRole::Content, RELAY, RelayFrame::Text(eose));

    assert_eq!(
        kernel.relay(RelayRole::Content).counters.eose_rx,
        before + 1,
        "eose_rx must increment even for an unknown sub_id",
    );
    assert!(
        kernel
            .wire_sub_state_for_test_on_relay(RELAY, "sub-never-opened")
            .is_none(),
        "an EOSE for an unknown sub must not synthesize a wire_subs row",
    );
    // The unknown sub is not a keep-live id, so the handler still routes a
    // defensive CLOSE — falling back to the delivering relay's URL since
    // there is no wire_subs row to read a recorded URL from.
    assert!(
        outbound.iter().any(|m| {
            m.relay_url == RELAY && m.text.contains("CLOSE") && m.text.contains("sub-never-opened")
        }),
        "an unknown-sub EOSE routes a defensive CLOSE on the delivering \
         relay; got: {:?}",
        outbound.iter().map(|m| &m.text).collect::<Vec<_>>(),
    );
}

/// An EOSE for a known, non-persistent oneshot-style sub must drive the
/// terminal close path: the `wire_subs` row is evicted and a `CLOSE` frame
/// is emitted back on the delivering socket.
#[test]
fn eose_for_known_non_persistent_sub_evicts_row_and_emits_close() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // A plain `OneShot` REQ — not a follow-feed / firehose / persistent sub,
    // so the EOSE keep-live predicate is false and the close-and-evict
    // branch runs.
    let sub_id = "profile-claim-eon";
    kernel.register_wire_frames_for_test(&[WireFrame::Req {
        relay_url: RELAY.to_string(),
        sub_id: sub_id.to_string(),
        filter_json: r#"{"kinds":[0],"authors":["aa"],"limit":1}"#.to_string(),
        interest_id: InterestId(7),
        lifecycle: InterestLifecycle::OneShot,
    }]);
    assert!(
        kernel
            .wire_sub_state_for_test_on_relay(RELAY, sub_id)
            .is_some(),
        "precondition: the wire_subs row must exist before EOSE",
    );

    let eose = serde_json::json!(["EOSE", sub_id]).to_string();
    let outbound = kernel.handle_message(RelayRole::Content, RELAY, RelayFrame::Text(eose));

    assert!(
        kernel
            .wire_sub_state_for_test_on_relay(RELAY, sub_id)
            .is_none(),
        "a non-persistent sub's wire_subs row must be evicted after EOSE",
    );
    assert!(
        outbound.iter().any(|m| {
            m.relay_url == RELAY && m.text.contains("CLOSE") && m.text.contains(sub_id)
        }),
        "EOSE on a non-persistent sub must emit a CLOSE frame for that \
         sub on the delivering relay; got: {:?}",
        outbound.iter().map(|m| &m.text).collect::<Vec<_>>(),
    );
}

// ─── NOTICE ──────────────────────────────────────────────────────────────────

/// A `NOTICE` frame's text must land on the relay's `last_notice` diagnostic
/// surface and bump the `notices_rx` counter.
#[test]
fn notice_text_is_stored_in_relay_diagnostics() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let before = kernel.relay(RelayRole::Content).counters.notices_rx;

    let notice = serde_json::json!(["NOTICE", "rate limit: slow down"]).to_string();
    kernel.handle_message(RelayRole::Content, RELAY, RelayFrame::Text(notice));

    assert_eq!(
        kernel.relay(RelayRole::Content).last_notice.as_deref(),
        Some("rate limit: slow down"),
        "NOTICE text must be surfaced on RelayHealth.last_notice",
    );
    assert_eq!(
        kernel.relay(RelayRole::Content).counters.notices_rx,
        before + 1,
        "notices_rx must increment per NOTICE frame",
    );
}

/// A `["NOTICE"]` frame with no text argument must not panic; the handler
/// falls back to the literal `"notice"` placeholder.
#[test]
fn notice_with_missing_text_argument_is_handled_without_panic() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let notice = serde_json::json!(["NOTICE"]).to_string();
    kernel.handle_message(RelayRole::Content, RELAY, RelayFrame::Text(notice));

    assert_eq!(
        kernel.relay(RelayRole::Content).last_notice.as_deref(),
        Some("notice"),
        "an arg-less NOTICE must fall back to the \"notice\" placeholder",
    );
    assert_eq!(
        kernel.relay(RelayRole::Content).counters.notices_rx,
        1,
        "an arg-less NOTICE is still a received NOTICE frame",
    );
}

/// An over-long `NOTICE` payload must be bounded to the 180-char diagnostic
/// cap (plus the `"..."` truncation marker the `truncate` helper appends)
/// before it is stored — a hostile relay must not be able to bloat the
/// snapshot with an unbounded string.
#[test]
fn notice_oversize_text_is_truncated_to_diagnostic_cap() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let huge = "z".repeat(500);
    let notice = serde_json::json!(["NOTICE", huge]).to_string();
    kernel.handle_message(RelayRole::Content, RELAY, RelayFrame::Text(notice));

    let stored = kernel
        .relay(RelayRole::Content)
        .last_notice
        .clone()
        .expect("a NOTICE with text must store last_notice");
    // `truncate(s, 180)` keeps 180 content chars then appends "..." when the
    // input is longer — so the stored string is bounded at 180 + 3 = 183.
    assert!(
        stored.chars().count() <= 183,
        "NOTICE text must be bounded to the 180-char cap (+\"...\"); \
         got {} chars",
        stored.chars().count(),
    );
    assert!(
        stored.starts_with(&"z".repeat(180)) && stored.ends_with("..."),
        "an over-cap NOTICE keeps 180 content chars then the \"...\" marker",
    );
}

// ─── OK (wire-frame parse seam) ──────────────────────────────────────────────

fn fake_signed(id: &str, author: &str, content: &str) -> SignedEvent {
    SignedEvent {
        id: id.to_string(),
        sig: format!("sig-{id}"),
        unsigned: UnsignedEvent {
            pubkey: author.to_string(),
            kind: 1,
            tags: Vec::new(),
            content: content.to_string(),
            created_at: 1_700_000_000,
        },
    }
}

/// Seed a kind:10002 so `Nip65OutboxResolver` routes the publish to a real
/// write relay instead of returning `NoTargets`.
fn seed_kind10002(kernel: &mut Kernel, author: &str, write_url: &str) {
    // Use the author pubkey as the event id — guaranteed valid hex (64 hex
    // chars) and unique per author.  The old approach embedded literal
    // non-hex chars ('k', 'e', 'o', 'n'); V-70 strengthened
    // `is_structurally_valid()` to check hex chars, so those synthetic
    // events were rejected as Malformed and never entered the store.
    let id = author.to_string();
    let raw = RawEvent {
        id,
        pubkey: author.to_string(),
        created_at: 1_700_000_000,
        kind: 10002,
        tags: vec![vec![
            "r".to_string(),
            write_url.to_string(),
            "write".to_string(),
        ]],
        content: String::new(),
        sig: "0".repeat(128),
    };
    kernel
        .store
        .insert(
            VerifiedEvent::from_raw_unchecked(raw),
            &"wss://seed".to_string(),
            1_700_000_000_000,
        )
        .expect("seed_kind10002 insert");
}

/// A `["OK", id, true, ""]` frame parsed off the wire (`handle_text` →
/// `route_publish_ok`) must reach the publish engine and settle the in-flight
/// publish as accepted — proving the wire-shape → FSM wiring, which the
/// `handle_publish_ok_at`-direct tests in `publish_engine_tests` bypass.
#[test]
fn ok_true_wire_frame_settles_publish_as_accepted() {
    let author = "33".repeat(32);
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    seed_kind10002(&mut kernel, &author, WRITE_R1);

    let signed = fake_signed(&"11".repeat(32), &author, "hello eon");
    let outbound = kernel.run_publish_engine_at(&signed, &[], PublishTarget::Auto, None, 1_000);
    assert_eq!(
        outbound.len(),
        1,
        "publish routes to the single write relay"
    );

    // The OK ack arrives as a raw wire frame on the same write relay.
    let ok = serde_json::json!(["OK", signed.id, true, ""]).to_string();
    kernel.handle_message(RelayRole::Content, WRITE_R1, RelayFrame::Text(ok));

    let snap = kernel.publish_status_snapshot();
    assert_eq!(
        snap.recent_ok.len(),
        1,
        "an OK=true wire frame must settle the publish into recent_ok",
    );
    assert!(
        snap.recent_errors.is_empty(),
        "an accepted publish must not appear in recent_errors",
    );
}

/// A `["OK", id, false, "blocked: spam"]` frame parsed off the wire must
/// route the rejection into the publish engine so the failure (with its
/// reason) surfaces on the diagnostic snapshot.
#[test]
fn ok_false_wire_frame_with_reason_settles_publish_as_rejected() {
    let author = "44".repeat(32);
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    seed_kind10002(&mut kernel, &author, WRITE_R1);

    let signed = fake_signed(&"22".repeat(32), &author, "spammy eon");
    let _ = kernel.run_publish_engine_at(&signed, &[], PublishTarget::Auto, None, 1_000);

    // `blocked:` is a terminal NIP-20 rejection prefix — no retry, the
    // publish settles straight to a failure.
    let ok = serde_json::json!(["OK", signed.id, false, "blocked: spam"]).to_string();
    kernel.handle_message(RelayRole::Content, WRITE_R1, RelayFrame::Text(ok));

    let snap = kernel.publish_status_snapshot();
    assert_eq!(
        snap.recent_ok.len(),
        0,
        "a blocked publish must not appear in recent_ok",
    );
    assert_eq!(
        snap.recent_errors.len(),
        1,
        "an OK=false wire frame must settle the publish into recent_errors",
    );
}

/// An `OK` frame for an event id the publish engine has no in-flight row for
/// must be an idempotent no-op (NIP-20 OKs can arrive late / unsolicited) —
/// no panic, no spurious snapshot entry.
#[test]
fn ok_wire_frame_for_unknown_event_id_is_a_graceful_no_op() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let ok = serde_json::json!(["OK", "ff".repeat(32), true, ""]).to_string();
    let outbound = kernel.handle_message(RelayRole::Content, WRITE_R1, RelayFrame::Text(ok));

    let snap = kernel.publish_status_snapshot();
    assert!(
        snap.recent_ok.is_empty() && snap.recent_errors.is_empty(),
        "an OK for an unknown event id must not synthesize a snapshot entry",
    );
    assert!(
        outbound.is_empty(),
        "an OK for an unknown event id must not schedule any retry frame",
    );
}
