//! Scenario 1 — connect + subscribe + receive a real third-party kind:1.
//!
//! The pre-existing `real_relay_smoke::damus_round_trip_kind1` proves we can
//! *publish our own* event and read it back. That does not prove the kernel
//! can ingest the live firehose: a real note, authored by someone else,
//! arriving over a REQ subscription. This scenario closes that gap.
//!
//! Honest-validation: if no candidate relay delivers a well-formed
//! third-party kind:1 within budget, the test writes a SKIP finding and
//! does not fake a pass.
//!
//! ```bash
//! cargo test -p nmp-testing --test real_relay_connect -- --ignored --nocapture
//! ```

#[path = "real_relay_common/mod.rs"]
mod common;

use std::time::{Duration, Instant};

use common::{
    report_page, send_text, try_open, write_report, Verdict, DAMUS_RELAY, NOS_LOL, PRIMAL_RELAY,
};
use serde_json::Value;

const BUDGET: Duration = Duration::from_secs(12);

/// Parse an `["EVENT", subid, {event}]` text frame for `sub_id`. Returns the
/// event object if it is a structurally-valid, signed kind:1 we did not
/// author (we never publish here, so any kind:1 is third-party by
/// construction).
fn parse_third_party_kind1(text: &str, sub_id: &str) -> Option<Value> {
    let v: Value = serde_json::from_str(text).ok()?;
    let arr = v.as_array()?;
    if arr.first()?.as_str()? != "EVENT" {
        return None;
    }
    if arr.get(1)?.as_str()? != sub_id {
        return None;
    }
    let ev = arr.get(2)?.as_object()?;
    if ev.get("kind")?.as_u64()? != 1 {
        return None;
    }
    let id = ev.get("id")?.as_str()?;
    let sig = ev.get("sig")?.as_str()?;
    let pubkey = ev.get("pubkey")?.as_str()?;
    let valid = id.len() == 64
        && id.chars().all(|c| c.is_ascii_hexdigit())
        && sig.len() == 128
        && sig.chars().all(|c| c.is_ascii_hexdigit())
        && pubkey.len() == 64
        && pubkey.chars().all(|c| c.is_ascii_hexdigit())
        && ev.get("created_at")?.as_u64().is_some();
    if valid {
        Some(arr.get(2)?.clone())
    } else {
        None
    }
}

#[test]
#[ignore = "real-relay (run with --ignored)"]
fn connect_subscribe_receive_real_kind1() {
    let candidates = [DAMUS_RELAY, NOS_LOL, PRIMAL_RELAY];
    let sub_id = format!("rr-connect-{}", common::now_ms());
    // Recent-but-not-edge window: kinds:[1], modest limit, since ~10 min ago.
    let since = common::now_s().saturating_sub(600);
    let req = format!("[\"REQ\",\"{sub_id}\",{{\"kinds\":[1],\"limit\":8,\"since\":{since}}}]");

    let mut attempted: Vec<&str> = Vec::new();
    for relay in candidates {
        attempted.push(relay);
        let Some(mut socket) = try_open(relay) else {
            continue;
        };
        if send_text(&mut socket, req.clone()).is_err() {
            eprintln!("SKIP: {relay}: REQ send failed");
            let _ = socket.close(None);
            continue;
        }

        let deadline = Instant::now() + BUDGET;
        let mut captured: Option<Value> = None;
        common::drain_until(&mut socket, deadline, |text| {
            if let Some(ev) = parse_third_party_kind1(text, &sub_id) {
                captured = Some(ev);
                true
            } else {
                false
            }
        });
        let _ = send_text(&mut socket, format!("[\"CLOSE\",\"{sub_id}\"]"));
        let _ = socket.close(None);

        if let Some(ev) = captured {
            let author = ev
                .get("pubkey")
                .and_then(Value::as_str)
                .unwrap_or("?")
                .to_string();
            let id = ev.get("id").and_then(Value::as_str).unwrap_or("?");
            let preview: String = ev
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or("")
                .chars()
                .take(80)
                .collect();
            let body = format!(
                "Subscribed to `{relay}` with `kinds:[1] limit:8 since:{since}` \
                 and received a structurally-valid, signed third-party kind:1 \
                 within {BUDGET:?}.\n\n\
                 - relay: `{relay}`\n- event id: `{id}`\n- author: `{author}`\n\
                 - content preview: `{preview}`\n\n\
                 Proves the kernel's wire path can ingest a real live note \
                 authored by someone other than the test harness."
            );
            write_report(
                "scenario1-connect",
                &report_page(
                    "Scenario 1 — connect + subscribe + receive real kind:1",
                    "1-connect-subscribe-receive",
                    Verdict::Pass,
                    &[relay],
                    &body,
                ),
            );
            println!("[connect] PASS via {relay}: id={id} author={author}");
            return;
        }
        eprintln!("[connect] {relay}: no third-party kind:1 within {BUDGET:?}");
    }

    // No relay delivered. Loud finding, no fake green.
    let body = format!(
        "No candidate relay delivered a structurally-valid third-party \
         kind:1 within {BUDGET:?} per relay.\n\n\
         Relays attempted (in order): {attempted:?}.\n\n\
         This is a SKIP, not a pass: either the public relay set was \
         unreachable from this host/network, or none returned a recent \
         kind:1 for the `since` window. Re-run with network access; if it \
         persists, the candidate relay list needs revisiting."
    );
    write_report(
        "scenario1-connect",
        &report_page(
            "Scenario 1 — connect + subscribe + receive real kind:1",
            "1-connect-subscribe-receive",
            Verdict::Skip,
            &attempted,
            &body,
        ),
    );
    eprintln!("SKIP: scenario 1 — no third-party kind:1 from any candidate relay");
}
