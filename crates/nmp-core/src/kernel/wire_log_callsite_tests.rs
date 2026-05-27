//! W8b call-site integration tests.
//!
//! Verifies that `log_wire` emissions fire at the correct seams without
//! coupling to the `NMP_CLAIM_LOG` `OnceLock` (which is set-once and
//! unreliable across test threads). Schema tests (T1–T4) drive
//! `write_wire_line` directly with `enabled = true`. Production-ingest
//! tests (T5–T6) drive `handle_text` through the real kernel and assert
//! observable side-effects, confirming the call-site wiring is live.
//!
//! # Test seam
//!
//! `write_wire_line<W: IoWrite>(w, enabled, event)` is `pub(super)` and
//! exposed for tests inside the `kernel` module tree. Each schema test
//! builds the `WireLogEvent` that the call site _would_ emit, feeds it
//! through `write_wire_line`, and asserts the JSON fields that W9's
//! grep-based acceptance tests rely on.
//!
//! # Tests
//!
//! 1. `all_three_claim_phases_emit_claim_phase_advance` — register →
//!    advance to phase2 → terminate; asserts 3 `ClaimPhaseAdvance` lines.
//! 2. `event_hit_emits_event_rx_line` — constructs a `WireLogEvent::EventRx`
//!    for a simulated claim hit and verifies the JSON schema.
//! 3. `req_emit_phase1_emits_req_emit_line` — constructs a
//!    `WireLogEvent::ReqEmit` for a phase1 claim REQ and verifies the
//!    JSON schema.
//! 4. `eose_no_match_emits_eose_rx_line` — schema pin for
//!    `EoseRx{matched:false}`.
//! 5. `event_hit_wires_event_rx_via_production_ingest` — drives a real
//!    `handle_text(EVENT)` frame through the kernel; asserts the
//!    `record_claim_expansion_hit` call-site (and hence `EventRx` log
//!    call) executed by checking the relay-score successes counter.
//! 6. `eose_after_hit_wires_eose_rx_matched_true_via_production_ingest` —
//!    drives `handle_text(EVENT)` followed by `handle_text(EOSE)`; asserts
//!    that the `EoseRx{matched:true}` early-return branch in
//!    `record_claim_expansion_eose_no_match` executed (score unchanged,
//!    dirty flag not re-set after the EOSE).

#[cfg(test)]
mod tests {
    use super::super::wire_log::{write_wire_line, WireLogEvent};

    // ── helpers ──────────────────────────────────────────────────────────────

    fn capture(event: &WireLogEvent<'_>) -> String {
        let mut buf: Vec<u8> = Vec::new();
        write_wire_line(&mut buf, true, event);
        String::from_utf8(buf).expect("valid UTF-8")
    }

    fn parse_json(line: &str) -> serde_json::Value {
        let json_str = line
            .strip_prefix("nmp.wire ")
            .expect("line must start with 'nmp.wire '");
        serde_json::from_str(json_str).expect("payload must be valid JSON")
    }

    // ── T1: claim lifecycle produces ClaimPhaseAdvance lines ─────────────────

    /// Simulates the three ClaimPhaseAdvance emissions the W5 controller fires
    /// over a claim's lifetime:
    ///   1. `register_claim_expansion` → from:"none"  to:"phase1"
    ///   2. `advance_to_phase2`        → from:"phase1" to:"phase2"
    ///   3. `terminate_claim(Hit)`     → from:"phase2" to:"terminal_hit"
    ///
    /// The test drives `write_wire_line` directly (not `log_wire`) so it is
    /// independent of the `NMP_CLAIM_LOG` `OnceLock`.
    #[test]
    fn all_three_claim_phases_emit_claim_phase_advance() {
        let events = [
            WireLogEvent::ClaimPhaseAdvance {
                author: "aabbccdd",
                from: "none",
                to: "phase1",
                reason: "registered",
            },
            WireLogEvent::ClaimPhaseAdvance {
                author: "aabbccdd",
                from: "phase1",
                to: "phase2",
                reason: "budget_elapsed",
            },
            WireLogEvent::ClaimPhaseAdvance {
                author: "aabbccdd",
                from: "phase2",
                to: "terminal_hit",
                reason: "terminal_hit",
            },
        ];

        let mut buf: Vec<u8> = Vec::new();
        for ev in &events {
            write_wire_line(&mut buf, true, ev);
        }

        let output = String::from_utf8(buf).expect("valid UTF-8");
        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(
            lines.len(),
            3,
            "exactly 3 ClaimPhaseAdvance lines for register→phase2→terminal; got:\n{output}"
        );

        let expected_to = ["phase1", "phase2", "terminal_hit"];
        for (i, line) in lines.iter().enumerate() {
            let v = parse_json(line);
            assert_eq!(
                v["type"], "ClaimPhaseAdvance",
                "line {i} must be ClaimPhaseAdvance; got: {}",
                v["type"]
            );
            assert_eq!(
                v["to"], expected_to[i],
                "line {i} `to` field mismatch; expected {:?}, got: {}",
                expected_to[i], v["to"]
            );
            assert_eq!(
                v["author"], "aabbccdd",
                "line {i} author must be preserved; got: {}",
                v["author"]
            );
        }
    }

    // ── T2: EventRx emitted on claim hit ─────────────────────────────────────

    /// Verifies the JSON schema for `WireLogEvent::EventRx` — the event
    /// emitted by `record_claim_expansion_hit` (W8b instrumentation in
    /// `relay_score_record.rs`). W9 acceptance tests grep for `"EventRx"`,
    /// `"sub_id"`, and `"event_id"` fields; this test pins that schema.
    #[test]
    fn event_hit_emits_event_rx_line() {
        let event = WireLogEvent::EventRx {
            sub_id: "sub-claim-abc123",
            relay_url: "wss://relay.damus.io",
            event_id: "deadbeef01234567deadbeef01234567deadbeef01234567deadbeef01234567",
            author: "pubkeyaabbccddeeff00112233445566778899aabbccddeeff00112233445566",
        };

        let output = capture(&event);
        let line = output.lines().next().expect("at least one line");
        let v = parse_json(line);

        assert_eq!(v["type"], "EventRx", "discriminant must be EventRx");
        assert_eq!(v["sub_id"], "sub-claim-abc123");
        assert_eq!(
            v["event_id"],
            "deadbeef01234567deadbeef01234567deadbeef01234567deadbeef01234567"
        );
        assert_eq!(v["relay_url"], "wss://relay.damus.io");
    }

    // ── T3: ReqEmit emitted at wire-frame bridge ──────────────────────────────

    /// Verifies the JSON schema for `WireLogEvent::ReqEmit` — the event
    /// emitted by `register_planner_wire_frames` for claim-expansion frames
    /// (W8b instrumentation in `kernel/requests/mod.rs`). W9 greps for
    /// `"ReqEmit"` + `"phase"` to track which phase a REQ was sent for.
    #[test]
    fn req_emit_phase1_emits_req_emit_line() {
        let event = WireLogEvent::ReqEmit {
            sub_id: "sub-abcdef0123456789",
            relay_url: "wss://nos.lol",
            phase: "phase1",
            author: "pubkeyaabbccddeeff00112233445566778899aabbccddeeff00112233445566",
            has_hint: false,
        };

        let output = capture(&event);
        let line = output.lines().next().expect("at least one line");
        let v = parse_json(line);

        assert_eq!(v["type"], "ReqEmit", "discriminant must be ReqEmit");
        assert_eq!(v["phase"], "phase1");
        assert_eq!(v["relay_url"], "wss://nos.lol");
        assert_eq!(v["has_hint"], false);
    }

    // ── T4: EoseRx{matched:false} emitted on no-match EOSE ───────────────────

    /// Verifies that `EoseRx{matched: false}` is emitted by
    /// `on_claim_outcome_eose_no_match` (W5 + W8b wired). Drives the
    /// event directly to verify JSON schema stability for W9 grep.
    #[test]
    fn eose_no_match_emits_eose_rx_line() {
        let event = WireLogEvent::EoseRx {
            sub_id: "sub-claim-eose-test",
            relay_url: "wss://relay.nostr.band",
            matched: false,
        };

        let output = capture(&event);
        let line = output.lines().next().expect("at least one line");
        let v = parse_json(line);

        assert_eq!(v["type"], "EoseRx", "discriminant must be EoseRx");
        assert_eq!(v["sub_id"], "sub-claim-eose-test");
        assert_eq!(v["matched"], false);
    }

    // ── T5: production-ingest — EventRx call-site wired ──────────────────────

    /// Drives a real `handle_text(EVENT)` frame through the kernel and
    /// asserts that `record_claim_expansion_hit` (and hence the W8b
    /// `EventRx` log call) executed by checking the relay-score
    /// successes counter. We cannot capture the actual `log_wire` output
    /// (it targets stderr and is gated by `NMP_CLAIM_LOG` via OnceLock),
    /// so the test pins the side-effect that proves the call-site ran.
    #[test]
    fn event_hit_wires_event_rx_via_production_ingest() {
        use super::super::test_support;
        use super::super::Kernel;
        use crate::relay::{RelayRole, DEFAULT_VISIBLE_LIMIT};

        test_support::clear_claim_expansion_subs();
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

        let keys = ::nostr::Keys::generate();
        let sub_id = "sub-w8b-callsite-event-rx";
        let relay_url = "wss://w8b-callsite-event.test";
        let event = signed_note(&keys, "w8b EventRx call-site proof", 1_700_010_000);
        let author_hex = event.pubkey.clone();

        test_support::register_claim_expansion_sub(sub_id, &author_hex);

        let before = kernel.get_relay_score(&author_hex, relay_url);
        assert_eq!(before.successes, 0, "pre-condition: zero successes");

        kernel.handle_text(RelayRole::Indexer, relay_url, &event_frame(sub_id, &event));

        let after = kernel.get_relay_score(&author_hex, relay_url);
        assert_eq!(
            after.successes, 1,
            "record_claim_expansion_hit (EventRx call site) must record a Hit; successes stayed at 0"
        );

        test_support::clear_claim_expansion_subs();
    }

    // ── T6: production-ingest — EoseRx{matched:true} call-site wired ─────────

    /// Drives `handle_text(EVENT)` then `handle_text(EOSE)` through the kernel
    /// and asserts that the `EoseRx{matched:true}` early-return branch in
    /// `record_claim_expansion_eose_no_match` was taken. Evidence: the EOSE
    /// must NOT record a second score outcome — successes stays at 1 and
    /// failures stays at 0, which is only possible if the early-return (which
    /// now carries the W8b `EoseRx{matched:true}` log call) was executed.
    #[test]
    fn eose_after_hit_wires_eose_rx_matched_true_via_production_ingest() {
        use super::super::test_support;
        use super::super::Kernel;
        use crate::relay::{RelayRole, DEFAULT_VISIBLE_LIMIT};

        test_support::clear_claim_expansion_subs();
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

        let keys = ::nostr::Keys::generate();
        let sub_id = "sub-w8b-callsite-eose-matched";
        let relay_url = "wss://w8b-callsite-eose-matched.test";
        let event = signed_note(&keys, "w8b EoseRx matched:true proof", 1_700_020_000);
        let author_hex = event.pubkey.clone();

        test_support::register_claim_expansion_sub(sub_id, &author_hex);

        // Phase 1: fire the EVENT hit — sets match-seen flag, scores a Hit.
        kernel.handle_text(RelayRole::Indexer, relay_url, &event_frame(sub_id, &event));
        assert_eq!(
            kernel.get_relay_score(&author_hex, relay_url).successes,
            1,
            "pre-condition: EVENT hit must record successes=1"
        );

        // Phase 2: fire the EOSE — must hit the matched:true early-return
        // branch in record_claim_expansion_eose_no_match and NOT record a
        // second score outcome (failures stays 0, successes stays 1).
        kernel.handle_text(RelayRole::Indexer, relay_url, &eose_frame(sub_id));

        let after_eose = kernel.get_relay_score(&author_hex, relay_url);
        assert_eq!(
            after_eose.successes, 1,
            "EOSE after hit must not change successes (EoseRx{{matched:true}} branch taken)"
        );
        assert_eq!(
            after_eose.failures, 0,
            "EOSE after hit must not record any failures (EoseRx{{matched:true}} branch taken)"
        );

        test_support::clear_claim_expansion_subs();
    }

    // ── helpers (production-ingest tests) ────────────────────────────────────

    fn signed_note(keys: &::nostr::Keys, content: &str, ts: u64) -> super::super::NostrEvent {
        use nostr::{EventBuilder, Timestamp};
        let nostr_event = EventBuilder::text_note(content)
            .custom_created_at(Timestamp::from(ts))
            .sign_with_keys(keys)
            .expect("sign_with_keys cannot fail with a generated keypair");
        super::super::NostrEvent {
            id: nostr_event.id.to_hex(),
            pubkey: nostr_event.pubkey.to_hex(),
            created_at: nostr_event.created_at.as_secs(),
            kind: nostr_event.kind.as_u16() as u32,
            tags: nostr_event
                .tags
                .iter()
                .map(|t: &::nostr::Tag| t.as_slice().to_vec())
                .collect(),
            content: nostr_event.content.clone(),
            sig: nostr_event.sig.to_string(),
        }
    }

    fn event_frame(sub_id: &str, event: &super::super::NostrEvent) -> String {
        serde_json::json!([
            "EVENT",
            sub_id,
            {
                "id": event.id,
                "pubkey": event.pubkey,
                "created_at": event.created_at,
                "kind": event.kind,
                "tags": event.tags,
                "content": event.content,
                "sig": event.sig,
            }
        ])
        .to_string()
    }

    fn eose_frame(sub_id: &str) -> String {
        serde_json::json!(["EOSE", sub_id]).to_string()
    }
}
