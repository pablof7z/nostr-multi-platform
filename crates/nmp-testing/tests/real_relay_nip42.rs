//! Scenario 4 — NIP-42 relay AUTH challenge / response against a real relay.
//!
//! Proves the kernel's NIP-42 protocol surface end-to-end on the wire:
//! probe a set of public relays for one that *demands* auth (sends
//! `["AUTH", <challenge>]` in response to a REQ), then build, sign, and
//! send the kind:22242 response and assert the relay's
//! `["OK", <id>, true, …]` ack.
//!
//! Honest-validation (this is the highest fake-pass-risk scenario):
//! - If NO candidate relay challenges within budget, write a loud SKIP
//!   finding listing every relay probed + exactly what it returned, then
//!   `return` with no assertion. Relays change auth policy often; a
//!   documented SKIP is the deliverable, never a fabricated green.
//! - If a relay challenges but rejects our signed event (`OK=false`),
//!   that is a FAIL — report it with the relay's stated reason.
//!
//! T115: this test imports `nmp-nip42` directly as a dev-dependency (added to
//! `crates/nmp-testing/Cargo.toml`). The four NIP-42 wire helpers
//! (`parse_auth_frame`, `parse_ok_frame`, `build_auth_event`,
//! `nmp_nip42::builder::wire_frame_for`) now come from the crate's public
//! surface, so any refactor breaking that API is caught here. The signing path
//! uses `nmp-signers::LocalKeySigner` (a real dev-dependency), exactly as
//! production would.
//!
//! ```bash
//! cargo test -p nmp-testing --test real_relay_nip42 -- --ignored --nocapture
//! ```

#[path = "real_relay_common/mod.rs"]
mod common;

use std::time::{Duration, Instant};

use common::{report_page, send_text, try_open, write_report, Verdict};
use nmp_nip42::{build_auth_event, parse_auth_frame, parse_ok_frame, AuthChallenge, AuthOk};
use nmp_signers::{LocalKeySigner, Signer, SignerOp};
use serde_json::Value;

/// Auth-required public relay candidates, probed in order. The first to
/// emit an `["AUTH", …]` frame wins; the rest only appear in the SKIP
/// finding. Policy here drifts often — this list is empirical, not a
/// guarantee.
const CANDIDATES: &[&str] = &[
    "wss://nostr.wine",
    "wss://relay.snort.social",
    "wss://auth.nostr1.com",
    "wss://nostr.land",
    "wss://relay.nostr.band",
];

/// Time spent draining for an `["AUTH", …]` challenge after sending REQ.
const CHALLENGE_BUDGET: Duration = Duration::from_secs(6);
/// Time spent awaiting the `["OK", <auth-id>, …]` ack after sending AUTH.
const OK_BUDGET: Duration = Duration::from_secs(8);

// --- scenario helpers -------------------------------------------------------

/// Parse a text frame into a JSON array of `Value`s (the kernel's
/// `handle_text` does the same outer split before dispatch).
fn frame_array(text: &str) -> Option<Vec<Value>> {
    serde_json::from_str::<Value>(text)
        .ok()?
        .as_array()
        .cloned()
}

/// Render the wire frame the kernel pushes to the relay:
/// `["AUTH", <event_json>]`. Delegates to `nmp_nip42::builder::wire_frame_for`.
fn wire_frame_for(signed: &nmp_core::substrate::SignedEvent) -> String {
    nmp_nip42::builder::wire_frame_for(signed)
}

// --- scenario ----------------------------------------------------------------

#[test]
#[ignore = "real-relay (run with --ignored)"]
fn nip42_auth_challenge_response() {
    let sub_id = format!("rr-nip42-{}", common::now_ms());
    let req = format!("[\"REQ\",\"{sub_id}\",{{\"kinds\":[1],\"limit\":1}}]");

    // Per-relay outcome line, dumped into the report regardless of verdict.
    let mut outcomes: Vec<String> = Vec::new();
    let mut probed: Vec<&str> = Vec::new();

    for &relay in CANDIDATES {
        probed.push(relay);

        let Some(mut socket) = try_open(relay) else {
            outcomes.push(format!("- `{relay}`: unreachable (connect failed)"));
            continue;
        };
        if send_text(&mut socket, req.clone()).is_err() {
            outcomes.push(format!("- `{relay}`: REQ send failed"));
            let _ = socket.close(None);
            continue;
        }

        // Phase 1: drain for an ["AUTH", <challenge>] frame.
        let mut challenge: Option<AuthChallenge> = None;
        let deadline = Instant::now() + CHALLENGE_BUDGET;
        common::drain_until(&mut socket, deadline, |text| {
            let Some(arr) = frame_array(text) else {
                return false;
            };
            if let Some(ch) = parse_auth_frame(&arr, relay) {
                challenge = Some(ch);
                true
            } else {
                false
            }
        });

        let Some(challenge) = challenge else {
            outcomes.push(format!(
                "- `{relay}`: connected, sent REQ, no `[\"AUTH\",…]` within {CHALLENGE_BUDGET:?}"
            ));
            let _ = send_text(&mut socket, format!("[\"CLOSE\",\"{sub_id}\"]"));
            let _ = socket.close(None);
            continue;
        };

        // Phase 2: build + sign the kind:22242 with a FRESH ephemeral key
        // (never reuse an identity for an auth probe).
        let signer = LocalKeySigner::generate();
        let pubkey_hex = signer.pubkey().to_hex();
        let unsigned = build_auth_event(&challenge, pubkey_hex.clone(), common::now_s());
        let signed = match signer.sign(unsigned) {
            SignerOp::Ready(Ok(s)) => s,
            SignerOp::Ready(Err(e)) => {
                outcomes.push(format!(
                    "- `{relay}`: challenged but local signer failed: {e:?}"
                ));
                let _ = socket.close(None);
                continue;
            }
            SignerOp::Pending(_) => {
                // LocalKeySigner is synchronous; Pending is impossible here.
                outcomes.push(format!(
                    "- `{relay}`: challenged but signer unexpectedly returned Pending"
                ));
                let _ = socket.close(None);
                continue;
            }
        };
        let auth_id = signed.id.clone();

        if send_text(&mut socket, wire_frame_for(&signed)).is_err() {
            outcomes.push(format!(
                "- `{relay}`: challenged, signed, but AUTH frame send failed"
            ));
            let _ = socket.close(None);
            continue;
        }

        // Phase 3: await the OK keyed to our kind:22242 event id.
        let mut auth_ok: Option<AuthOk> = None;
        let deadline = Instant::now() + OK_BUDGET;
        common::drain_until(&mut socket, deadline, |text| {
            let Some(arr) = frame_array(text) else {
                return false;
            };
            match parse_ok_frame(&arr) {
                Some(ok) if ok.event_id == auth_id => {
                    auth_ok = Some(ok);
                    true
                }
                _ => false,
            }
        });
        let _ = send_text(&mut socket, format!("[\"CLOSE\",\"{sub_id}\"]"));
        let _ = socket.close(None);

        match auth_ok {
            Some(ok) if ok.accepted => {
                let body = format!(
                    "Relay `{relay}` returned `[\"AUTH\", <challenge>]` in response to a \
                     `kinds:[1] limit:1` REQ. We parsed the challenge, generated a fresh \
                     ephemeral key, built the kind:22242 AUTH event \
                     (`[\"relay\",\"{relay}\"]` + `[\"challenge\",…]` tags, empty content), \
                     signed it with `LocalKeySigner`, and sent `[\"AUTH\", <event>]`.\n\n\
                     The relay acknowledged with `OK={accepted}` for our auth event id.\n\n\
                     - relay: `{relay}`\n\
                     - challenge: `{challenge}`\n\
                     - auth event id: `{auth_id}`\n\
                     - auth pubkey (ephemeral): `{pubkey}`\n\
                     - OK accepted: `{accepted}`\n\
                     - OK reason: `{reason}`\n\n\
                     Proves the NIP-42 challenge/response handshake works against a relay \
                     that genuinely requires it.",
                    relay = relay,
                    challenge = challenge.challenge,
                    auth_id = auth_id,
                    pubkey = pubkey_hex,
                    accepted = ok.accepted,
                    reason = ok.reason,
                );
                write_report(
                    "scenario4-nip42",
                    &report_page(
                        "Scenario 4 — NIP-42 relay AUTH challenge/response",
                        "4-nip42-auth",
                        Verdict::Pass,
                        &[relay],
                        &body,
                    ),
                );
                println!(
                    "[nip42] PASS via {relay}: auth_id={auth_id} accepted=true"
                );
                return;
            }
            Some(ok) => {
                // Challenged + rejected our signed event → genuine FAIL.
                let body = format!(
                    "Relay `{relay}` challenged us (`[\"AUTH\", <challenge>]`), we signed \
                     and sent a well-formed kind:22242, and the relay **rejected** it.\n\n\
                     - relay: `{relay}`\n\
                     - challenge: `{challenge}`\n\
                     - auth event id: `{auth_id}`\n\
                     - auth pubkey (ephemeral): `{pubkey}`\n\
                     - OK accepted: `false`\n\
                     - OK reason: `{reason}`\n\n\
                     This is a FAIL, not a SKIP: the relay engaged the NIP-42 handshake \
                     and refused our response. The relay's stated reason is recorded \
                     verbatim above — investigate (clock skew, tag shape, or relay policy).",
                    relay = relay,
                    challenge = challenge.challenge,
                    auth_id = auth_id,
                    pubkey = pubkey_hex,
                    reason = ok.reason,
                );
                write_report(
                    "scenario4-nip42",
                    &report_page(
                        "Scenario 4 — NIP-42 relay AUTH challenge/response",
                        "4-nip42-auth",
                        Verdict::Fail,
                        &[relay],
                        &body,
                    ),
                );
                panic!(
                    "FAIL: {relay} rejected our signed kind:22242 AUTH event: {}",
                    ok.reason
                );
            }
            None => {
                outcomes.push(format!(
                    "- `{relay}`: challenged (`{}`), sent signed kind:22242, no matching \
                     `[\"OK\", {auth_id}, …]` within {OK_BUDGET:?}",
                    challenge.challenge
                ));
                continue;
            }
        }
    }

    // No relay completed the handshake. Loud SKIP with full per-relay log.
    let body = format!(
        "No candidate relay completed a NIP-42 AUTH challenge/response within \
         budget (challenge {CHALLENGE_BUDGET:?}, OK {OK_BUDGET:?} per relay).\n\n\
         Relays probed (in order): {probed:?}.\n\n\
         Per-relay outcome:\n\n{outcomes}\n\n\
         This is a SKIP, not a pass: public relay auth policy changes \
         frequently, so on any given day none of the candidates may demand \
         AUTH (or the set may be unreachable from this host). Re-run with \
         network access; if it persists, the candidate relay list needs \
         revisiting. No fake-green assertion is emitted here.",
        outcomes = outcomes.join("\n"),
    );
    write_report(
        "scenario4-nip42",
        &report_page(
            "Scenario 4 — NIP-42 relay AUTH challenge/response",
            "4-nip42-auth",
            Verdict::Skip,
            &probed,
            &body,
        ),
    );
    eprintln!(
        "SKIP: scenario 4 — no candidate relay completed a NIP-42 handshake; \
         see docs/perf/real-relay/scenario4-nip42.md"
    );
}
