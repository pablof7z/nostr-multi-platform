//! Real-relay ReducedSource feed acquisition for issue #2092 M6.
//!
//! This ignored scenario publishes a fresh kind:3 contact list and kind:1 note
//! to a live public relay, then drives the canonical `ActiveUserFollows` feed
//! through a real `NmpApp`. Public relay behaviour is not deterministic, so the
//! test writes a PASS/SKIP report and never pretends a skipped relay condition
//! is a green assertion.
//!
//! ```bash
//! cargo test -p nmp-testing --test real_relay_reduced_source -- --ignored --nocapture --test-threads=1
//! ```

#[path = "real_relay_common/mod.rs"]
mod common;
#[allow(dead_code, unused_imports)]
#[path = "reduced_source_relay_e2e/support.rs"]
mod support;

use std::env;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use common::{
    drain_until, report_page, send_text, try_open, write_report, Verdict, DAMUS_RELAY, NOS_LOL,
    PRIMAL_RELAY,
};
use nmp_ffi::NmpApp;
use nostr::{Event, Keys};
use serde_json::{json, Value};

const SCENARIO: &str = "reduced-source-feed";
const REPORT: &str = "reduced-source-relay";
const APP_WAIT: Duration = Duration::from_secs(30);
const OK_WAIT: Duration = Duration::from_secs(8);

fn candidate_relays() -> Vec<String> {
    if let Ok(raw) = env::var("NMP_TEST_RELAYS") {
        if let Ok(rows) = serde_json::from_str::<Vec<[String; 2]>>(&raw) {
            let relays = rows
                .into_iter()
                .map(|[relay, _]| relay)
                .filter(|relay| relay.starts_with("ws://") || relay.starts_with("wss://"))
                .collect::<Vec<_>>();
            if !relays.is_empty() {
                return relays;
            }
        }
    }

    if let Ok(raw) = env::var("RELAYS") {
        let relays = raw
            .split_whitespace()
            .filter(|relay| relay.starts_with("ws://") || relay.starts_with("wss://"))
            .map(str::to_string)
            .collect::<Vec<_>>();
        if !relays.is_empty() {
            return relays;
        }
    }

    [DAMUS_RELAY, PRIMAL_RELAY, NOS_LOL]
        .into_iter()
        .map(str::to_string)
        .collect()
}

fn relay_refs(relays: &[String]) -> Vec<&str> {
    relays.iter().map(String::as_str).collect()
}

fn ok_frame(text: &str, event_id: &str) -> Option<Result<(), String>> {
    let value: Value = serde_json::from_str(text).ok()?;
    let arr = value.as_array()?;
    if arr.first().and_then(Value::as_str) != Some("OK") {
        return None;
    }
    if arr.get(1).and_then(Value::as_str) != Some(event_id) {
        return None;
    }
    match arr.get(2).and_then(Value::as_bool) {
        Some(true) => Some(Ok(())),
        Some(false) => Some(Err(arr
            .get(3)
            .and_then(Value::as_str)
            .unwrap_or("relay returned OK=false")
            .to_string())),
        None => Some(Err("relay OK frame had no boolean status".to_string())),
    }
}

fn publish_event(
    socket: &mut common::RelaySocket,
    relay: &str,
    event: &Event,
) -> Result<(), String> {
    let event_id = event.id.to_hex();
    send_text(socket, json!(["EVENT", event]).to_string())
        .map_err(|e| format!("{relay}: send EVENT {event_id} failed: {e}"))?;

    let mut result = None;
    let deadline = Instant::now() + OK_WAIT;
    drain_until(socket, deadline, |text| {
        if let Some(ok) = ok_frame(text, &event_id) {
            result = Some(ok);
            true
        } else {
            false
        }
    });
    result
        .unwrap_or_else(|| {
            Err(format!(
                "{relay}: no OK for EVENT {event_id} within {OK_WAIT:?}"
            ))
        })
        .map_err(|e| format!("{relay}: EVENT {event_id} rejected: {e}"))
}

fn publish_to_first_accepting(relays: &[String], events: &[&Event]) -> Result<String, String> {
    let mut failures = Vec::new();
    for relay in relays {
        let Some(mut socket) = try_open(relay) else {
            failures.push(format!("{relay}: unreachable"));
            continue;
        };
        let mut accepted = true;
        for event in events {
            if let Err(err) = publish_event(&mut socket, relay, event) {
                failures.push(err);
                accepted = false;
                break;
            }
        }
        let _ = socket.close(None);
        if accepted {
            return Ok(relay.clone());
        }
    }
    Err(failures.join("\n"))
}

fn wait_feed_contains(
    rx: &Receiver<()>,
    app: &NmpApp,
    key: &str,
    event_id: &str,
    budget: Duration,
) -> bool {
    if support::flat_feed_ids(app, key)
        .iter()
        .any(|id| id == event_id)
    {
        return true;
    }
    let deadline = Instant::now() + budget;
    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if rx.recv_timeout(remaining).is_err() {
            return support::flat_feed_ids(app, key)
                .iter()
                .any(|id| id == event_id);
        }
        if support::flat_feed_ids(app, key)
            .iter()
            .any(|id| id == event_id)
        {
            return true;
        }
    }
    false
}

fn write_skip(relays: &[String], body: &str) {
    let refs = relay_refs(relays);
    write_report(
        REPORT,
        &report_page(
            "ReducedSource feed acquisition",
            SCENARIO,
            Verdict::Skip,
            &refs,
            body,
        ),
    );
    eprintln!("SKIP: {SCENARIO} - {body}");
}

#[test]
#[ignore = "real-relay (run with --ignored)"]
fn active_follows_reduced_source_over_real_relay() {
    let _serial = support::SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let relays = candidate_relays();

    let alice = Keys::generate();
    let bob = Keys::generate();
    let alice_pk = alice.public_key().to_hex();
    let bob_pk = bob.public_key().to_hex();
    let now = common::now_s().saturating_sub(2);
    let contact_list = support::signed_contact_list(&alice, std::slice::from_ref(&bob_pk), now);
    let note = support::signed_note(
        &bob,
        &format!("nmp reduced-source real-relay {}", common::now_ms()),
        now.saturating_add(1),
    );
    let note_id = note.id.to_hex();

    let Ok(relay) = publish_to_first_accepting(&relays, &[&contact_list, &note]) else {
        write_skip(
            &relays,
            "No candidate relay accepted both the fresh kind:3 source event and the fresh kind:1 note. No app assertion was attempted.",
        );
        return;
    };

    let rx = support::install_update_signal();
    let app = support::new_started_default_app();
    support::add_relay(app, &relay);
    support::sign_in(app, &alice);
    let app_ref = unsafe { &*app };
    support::wait_active(&rx, app_ref, &alice_pk);

    let key = "real-relay.reduced-source.active-follows";
    let _handle = app_ref
        .open_feed(&support::active_follows_params(key), &support::compiler)
        .expect("active-follows feed opens");

    if !wait_feed_contains(&rx, app_ref, key, &note_id, APP_WAIT) {
        nmp_ffi::nmp_app_free(app);
        support::uninstall_update_signal();
        write_skip(
            &[relay],
            "The relay accepted both events but did not serve the derived kind:1 row through the app feed within the public-relay budget. This is not marked PASS because the acquisition assertion was not observed.",
        );
        return;
    }

    nmp_ffi::nmp_app_free(app);
    support::uninstall_update_signal();
    write_report(
        REPORT,
        &report_page(
            "ReducedSource feed acquisition",
            SCENARIO,
            Verdict::Pass,
            &[relay.as_str()],
            &format!(
                "Published a fresh kind:3 for `{alice}` following `{bob}`, published `{note}`, then opened `ActiveUserFollows` through the real app/kernel feed path and observed the note in the decoded NOFS snapshot.\n\n- relay: `{relay}`\n- source event: `{source}`\n- note event: `{note}`",
                alice = alice_pk,
                bob = bob_pk,
                note = note_id,
                source = contact_list.id.to_hex(),
            ),
        ),
    );
}
