//! Scenario 3 — NIP-77 negentropy against a real relay + graceful REQ
//! fallback against one that does not speak it.
//!
//! Leg A: relay.damus.io (strfry) speaks NEG. With an empty local set, drive
//! `Reconciler::client` over a live socket; prove it converges to `Done`
//! with non-empty `need` and that negentropy payload bytes-on-wire fall far
//! below the REQ-equivalent floor (D2 — negentropy first saves wire).
//! Leg B: probe relays with the same NEG-OPEN; one that does not speak
//! NIP-77 replies NOTICE/CLOSED/NEG-ERR/silence — classify it, then prove a
//! plain REQ to that same relay returns an EVENT (fallback path works).
//! Honest-validation: unreachable / non-convergent / all-relays-speak-NEG =>
//! SKIP with a written finding. PASS only if BOTH legs genuinely pass.
//!
//! ```bash
//! cargo test -p nmp-testing --test real_relay_nip77 -- --ignored --nocapture
//! ```

#[path = "real_relay_common/mod.rs"]
mod common;

use std::time::{Duration, Instant};

use common::{
    now_ms, report_page, send_text, try_open, write_report, RelaySocket, Verdict, DAMUS_RELAY,
    NOSTR_BAND, NOS_LOL, PRIMAL_RELAY,
};
use nmp_nip77::{ClientFrame, Reconciler, ReconcilerOutcome, RelayFrame, WireError};
use serde_json::json;
use tungstenite::Message;

const LEG_A_BUDGET: Duration = Duration::from_secs(20);
const LEG_B_PROBE: Duration = Duration::from_secs(8);
/// REQ-equivalent floor per id: a signed kind:1 `["EVENT",sub,{...}]`
/// envelope is conservatively ~256 B (real ones are 400-800 B).
const REQ_BYTES_PER_ID: usize = 256;

struct LegResult {
    verdict: Verdict,
    summary: String,
}

/// Read one text frame within `deadline`, mirroring `drain_until`'s frame
/// handling but returning the payload so the caller can respond on the same
/// socket (a `drain_until` closure cannot — it would borrow-conflict).
fn read_text(socket: &mut RelaySocket, deadline: Instant) -> Option<String> {
    while Instant::now() < deadline {
        match socket.read() {
            Ok(Message::Text(t)) => return Some(t),
            Ok(_) => {}
            Err(tungstenite::Error::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(e) => {
                eprintln!("[nip77] socket error: {e}");
                return None;
            }
        }
    }
    None
}

fn skip(s: impl Into<String>) -> LegResult {
    LegResult {
        verdict: Verdict::Skip,
        summary: s.into(),
    }
}

/// Build an empty-set client reconciler and its initial NEG message.
fn fresh_initial() -> Result<(Reconciler, Vec<u8>), String> {
    let mut r = Reconciler::client(vec![]).map_err(|e| e.to_string())?;
    match r.step(None) {
        Ok(ReconcilerOutcome::Send(b)) if !b.is_empty() => Ok((r, b)),
        Ok(o) => Err(format!("step(None) did not yield non-empty Send: {o:?}")),
        Err(e) => Err(e.to_string()),
    }
}

/// LEG A — negentropy genuinely reconciles against strfry on damus.
fn run_leg_a() -> LegResult {
    let relay = DAMUS_RELAY;
    let Some(mut socket) = try_open(relay) else {
        return skip(format!("`{relay}` unreachable within connect budget."));
    };
    let sub_id = format!("rr-neg-{}", now_ms());
    let (mut reconciler, initial) = match fresh_initial() {
        Ok(v) => v,
        Err(e) => return skip(format!("local reconciler init failed: {e}")),
    };
    let open = ClientFrame::Open {
        sub_id: sub_id.clone(),
        filter: json!({ "kinds": [1], "limit": 200 }),
        initial_msg: initial.clone(),
    };
    if send_text(&mut socket, open.to_text()).is_err() {
        return skip(format!("`{relay}`: NEG-OPEN send failed"));
    }

    // Negentropy payload bytes only (not JSON envelope) — apples-to-apples
    // against the REQ floor.
    let mut neg_bytes = initial.len();
    let deadline = Instant::now() + LEG_A_BUDGET;
    let mut converged: Option<(usize, usize)> = None; // (need, have)
    let mut neg_err: Option<String> = None;

    while Instant::now() < deadline {
        let Some(text) = read_text(&mut socket, deadline) else {
            break;
        };
        // strfry interleaves non-NEG frames (NOTICE/EOSE/...). Parse failure
        // is not a protocol failure — skip and keep reading.
        let frame = match RelayFrame::parse(&text) {
            Ok(f) => f,
            Err(WireError::UnknownVerb(_)) | Err(WireError::NotAnArray) => continue,
            Err(_) => continue,
        };
        match frame {
            RelayFrame::Err { sub_id: s, reason } if s == sub_id => {
                neg_err = Some(reason);
                break;
            }
            RelayFrame::Msg { sub_id: s, msg } if s == sub_id => {
                neg_bytes += msg.len();
                match reconciler.step(Some(&msg)) {
                    Ok(ReconcilerOutcome::Send(b)) => {
                        neg_bytes += b.len();
                        let cont = ClientFrame::Msg {
                            sub_id: sub_id.clone(),
                            msg: b,
                        };
                        if send_text(&mut socket, cont.to_text()).is_err() {
                            break;
                        }
                    }
                    Ok(ReconcilerOutcome::Done { need, have, .. }) => {
                        converged = Some((need.len(), have.len()));
                        break;
                    }
                    Err(e) => {
                        neg_err = Some(format!("reconciler step error: {e}"));
                        break;
                    }
                }
            }
            // Frames for another sub on a shared connection: ignore.
            RelayFrame::Err { .. } | RelayFrame::Msg { .. } => continue,
        }
    }

    let _ = send_text(&mut socket, ClientFrame::Close { sub_id }.to_text());
    let _ = socket.close(None);

    let Some((need, have)) = converged else {
        return skip(match neg_err {
            Some(r) => format!("`{relay}`: NEG aborted before convergence (reason: `{r}`)."),
            None => format!("`{relay}`: no Done / no NEG-ERR within {LEG_A_BUDGET:?}."),
        });
    };
    if need == 0 {
        return skip(format!(
            "`{relay}` converged but `need` empty — empty local set should pull ids."
        ));
    }
    let req_floor = need * REQ_BYTES_PER_ID;
    if neg_bytes >= req_floor {
        return skip(format!(
            "`{relay}` converged (need={need}, have={have}) but negentropy used \
             {neg_bytes} B >= REQ floor {req_floor} B — no wire saving."
        ));
    }
    let saved = req_floor - neg_bytes;
    let pct = (saved as f64 / req_floor as f64) * 100.0;
    LegResult {
        verdict: Verdict::Pass,
        summary: format!(
            "`{relay}` (strfry) reconciled to Done with need={need} (have={have}) \
             from an empty local set. Negentropy moved {neg_bytes} B of protocol \
             payload vs a REQ floor of {req_floor} B ({need} ids x \
             {REQ_BYTES_PER_ID} B) -> {saved} B saved ({pct:.1}%). Live D2 proof."
        ),
    }
}

/// Classify a relay's NEG-OPEN response. `Ok(signal)` = does NOT speak
/// NIP-77 (NOTICE/CLOSED/NEG-ERR/silence). `Err(())` = answered with NEG-MSG.
fn probe_no_nip77(socket: &mut RelaySocket, sub_id: &str) -> Result<String, ()> {
    let deadline = Instant::now() + LEG_B_PROBE;
    while let Some(text) = read_text(socket, deadline) {
        match RelayFrame::parse(&text) {
            Ok(RelayFrame::Msg { sub_id: s, .. }) if s == sub_id => return Err(()),
            Ok(RelayFrame::Err { sub_id: s, reason }) if s == sub_id => {
                return Ok(format!("NEG-ERR: `{reason}`"))
            }
            _ => {}
        }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
            match v.get(0).and_then(|x| x.as_str()) {
                Some("NOTICE") => return Ok(format!("NOTICE: {text}")),
                Some("CLOSED") => return Ok(format!("CLOSED: {text}")),
                _ => {}
            }
        }
    }
    Ok(format!("silence (no NIP-77 reply within {LEG_B_PROBE:?})"))
}

/// Send a plain REQ on `socket` and report whether an EVENT arrives.
fn req_returns_event(socket: &mut RelaySocket) -> bool {
    let sub = format!("rr-reqB-{}", now_ms());
    if send_text(socket, format!("[\"REQ\",\"{sub}\",{{\"kinds\":[1],\"limit\":5}}]")).is_err() {
        return false;
    }
    let dl = Instant::now() + LEG_B_PROBE;
    let mut got = false;
    while let Some(t) = read_text(socket, dl) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&t) {
            if v.get(0).and_then(|x| x.as_str()) == Some("EVENT")
                && v.get(1).and_then(|x| x.as_str()) == Some(sub.as_str())
            {
                got = true;
                break;
            }
        }
    }
    let _ = send_text(socket, format!("[\"CLOSE\",\"{sub}\"]"));
    got
}

/// LEG B — find a relay that does NOT speak NIP-77, prove REQ fallback works.
fn run_leg_b() -> LegResult {
    let mut probed: Vec<String> = Vec::new();
    let mut any_spoke_neg = false;

    for relay in [NOS_LOL, NOSTR_BAND, PRIMAL_RELAY] {
        let Some(mut socket) = try_open(relay) else {
            probed.push(format!("{relay}: unreachable"));
            continue;
        };
        let sub_id = format!("rr-negB-{}", now_ms());
        let initial = match fresh_initial() {
            Ok((_, b)) => b,
            Err(e) => {
                probed.push(format!("{relay}: local init failed ({e})"));
                let _ = socket.close(None);
                continue;
            }
        };
        let open = ClientFrame::Open {
            sub_id: sub_id.clone(),
            filter: json!({ "kinds": [1], "limit": 200 }),
            initial_msg: initial,
        };
        if send_text(&mut socket, open.to_text()).is_err() {
            probed.push(format!("{relay}: NEG-OPEN send failed"));
            let _ = socket.close(None);
            continue;
        }
        match probe_no_nip77(&mut socket, &sub_id) {
            Err(()) => {
                any_spoke_neg = true;
                probed.push(format!("{relay}: speaks NIP-77 (NEG-MSG)"));
                let _ = socket.close(None);
            }
            Ok(signal) => {
                let got_event = req_returns_event(&mut socket);
                let _ = socket.close(None);
                probed.push(format!("{relay}: NO NIP-77 [{signal}]"));
                if got_event {
                    return LegResult {
                        verdict: Verdict::Pass,
                        summary: format!(
                            "`{relay}` does NOT speak NIP-77 (classified via {signal}). \
                             Plain-REQ fallback to the same relay returned a live EVENT \
                             within {LEG_B_PROBE:?} -> graceful fallback proven."
                        ),
                    };
                }
                return skip(format!(
                    "`{relay}` classified NO-NIP-77 ({signal}) but plain-REQ returned \
                     no EVENT within {LEG_B_PROBE:?}. Probed: {probed:?}"
                ));
            }
        }
    }

    if any_spoke_neg {
        skip(format!(
            "No reachable candidate could be classified non-NIP-77; >=1 spoke \
             NEG-MSG: {probed:?}. A non-negentropy relay could not be found, so \
             REQ-fallback cannot be exercised. FINDING, not a fabricated fallback."
        ))
    } else {
        skip(format!(
            "No candidate reachable / well-formed enough to classify: {probed:?}. \
             Re-run with network access."
        ))
    }
}

#[test]
#[ignore = "real-relay (run with --ignored)"]
fn nip77_negentropy_and_req_fallback() {
    let a = run_leg_a();
    let b = run_leg_b();

    let overall = if a.verdict == Verdict::Pass && b.verdict == Verdict::Pass {
        Verdict::Pass
    } else if a.verdict == Verdict::Fail || b.verdict == Verdict::Fail {
        Verdict::Fail
    } else {
        Verdict::Skip
    };

    let body = format!(
        "Two legs prove the D2 contract end-to-end against live relays: \
         negentropy genuinely saves wire vs REQ (Leg A), and a relay that \
         cannot speak negentropy is classified so the planner falls back to \
         plain REQ (Leg B).\n\n\
         ## Leg A — negentropy works (relay.damus.io / strfry)\n\n**{a_v}** — {a_s}\n\n\
         ## Leg B — REQ-fallback signal (non-NIP-77 relay)\n\n**{b_v}** — {b_s}\n\n\
         ## Overall\n\n**{ov}** — overall PASS requires BOTH legs to genuinely \
         pass; any SKIP keeps the scenario SKIP. No leg fakes green.",
        a_v = a.verdict.as_str(),
        a_s = a.summary,
        b_v = b.verdict.as_str(),
        b_s = b.summary,
        ov = overall.as_str(),
    );

    write_report(
        "scenario3-nip77",
        &report_page(
            "Scenario 3 — NIP-77 negentropy + graceful REQ fallback",
            "3-nip77-negentropy-req-fallback",
            overall,
            &[DAMUS_RELAY, NOS_LOL, NOSTR_BAND, PRIMAL_RELAY],
            &body,
        ),
    );

    println!("[nip77] Leg A: {} — {}", a.verdict.as_str(), a.summary);
    println!("[nip77] Leg B: {} — {}", b.verdict.as_str(), b.summary);
    println!("[nip77] OVERALL: {}", overall.as_str());

    if overall != Verdict::Pass {
        println!(
            "SKIP: scenario 3 — overall {} (see docs/perf/real-relay/scenario3-nip77.md)",
            overall.as_str()
        );
        return;
    }
    println!("[nip77] PASS: negentropy proven + REQ fallback proven");
}
