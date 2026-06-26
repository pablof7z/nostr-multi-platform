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

const FEED_SCENARIO: &str = "reduced-source-feed";
const FEED_REPORT: &str = "reduced-source-relay";
const NIP65_SCENARIO: &str = "reduced-source-nip65-reroute";
const NIP65_REPORT: &str = "reduced-source-nip65-reroute";
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

fn publish_all_to_relay(relay: &str, events: &[&Event]) -> Result<(), String> {
    let Some(mut socket) = try_open(relay) else {
        return Err(format!("{relay}: unreachable"));
    };
    for event in events {
        publish_event(&mut socket, relay, event)?;
    }
    let _ = socket.close(None);
    Ok(())
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
    write_skip_for(
        FEED_REPORT,
        "ReducedSource feed acquisition",
        FEED_SCENARIO,
        relays,
        body,
    );
}

fn write_skip_for(report: &str, title: &str, scenario: &str, relays: &[String], body: &str) {
    let refs = relay_refs(relays);
    write_report(
        report,
        &report_page(title, scenario, Verdict::Skip, &refs, body),
    );
    eprintln!("SKIP: {scenario} - {body}");
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
        FEED_REPORT,
        &report_page(
            "ReducedSource feed acquisition",
            FEED_SCENARIO,
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

#[test]
#[ignore = "real-relay (run with --ignored)"]
fn list_members_nip65_reroute_over_real_relays() {
    let _serial = support::SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let relays = candidate_relays();
    let target_relays = relays
        .iter()
        .filter(|relay| relay.starts_with("wss://"))
        .cloned()
        .collect::<Vec<_>>();

    if target_relays.len() < 2 {
        write_skip_for(
            NIP65_REPORT,
            "ReducedSource NIP-65 reroute",
            NIP65_SCENARIO,
            &relays,
            "Need at least two distinct `wss://` candidate relays to prove source-relay acquisition followed by NIP-65 target-relay fetch.",
        );
        return;
    }

    let mut failures = Vec::new();
    for source_relay in &target_relays {
        for target_relay in &target_relays {
            if source_relay == target_relay {
                continue;
            }
            match try_nip65_reroute_pair(source_relay, target_relay) {
                Ok(()) => {
                    write_report(
                        NIP65_REPORT,
                        &report_page(
                            "ReducedSource NIP-65 reroute",
                            NIP65_SCENARIO,
                            Verdict::Pass,
                            &[source_relay.as_str(), target_relay.as_str()],
                            &format!(
                                "Published the active user's kind:10000 source list and Bob's kind:10002 relay list to the source relay, published Bob's kind:1 note only to the target relay, configured the app with the source relay only, then observed the note in the decoded NOFS snapshot.\n\n- source relay: `{source_relay}`\n- target relay learned from kind:10002: `{target_relay}`"
                            ),
                        ),
                    );
                    return;
                }
                Err(err) => failures.push(format!("{source_relay} -> {target_relay}: {err}")),
            }
        }
    }

    write_skip_for(
        NIP65_REPORT,
        "ReducedSource NIP-65 reroute",
        NIP65_SCENARIO,
        &target_relays,
        &format!(
            "No candidate relay pair produced an observed app snapshot row for the NIP-65 target note. Attempts:\n\n```text\n{}\n```",
            failures.join("\n")
        ),
    );
}

fn try_nip65_reroute_pair(source_relay: &str, target_relay: &str) -> Result<(), String> {
    let alice = Keys::generate();
    let bob = Keys::generate();
    let alice_pk = alice.public_key().to_hex();
    let bob_pk = bob.public_key().to_hex();
    let now = common::now_s().saturating_sub(5);
    let mute_list = support::signed_mute_list(&alice, std::slice::from_ref(&bob_pk), now);
    let relay_list = support::signed_relay_list(&bob, &[target_relay], now.saturating_add(1));
    let note = support::signed_note(
        &bob,
        &format!("nmp reduced-source nip65 reroute {}", common::now_ms()),
        now.saturating_add(2),
    );
    let note_id = note.id.to_hex();

    publish_all_to_relay(source_relay, &[&mute_list, &relay_list])?;
    publish_all_to_relay(target_relay, &[&note])?;

    let rx = support::install_update_signal();
    let app = support::new_started_default_app();
    support::add_relay(app, source_relay);
    support::sign_in(app, &alice);
    let app_ref = unsafe { &*app };
    support::wait_active(&rx, app_ref, &alice_pk);

    let key = "real-relay.reduced-source.nip65-reroute";
    let _handle = app_ref
        .open_feed(&support::mute_source_params(key), &support::compiler)
        .expect("active mute-list feed opens");

    let observed = wait_feed_contains(&rx, app_ref, key, &note_id, APP_WAIT);
    nmp_ffi::nmp_app_free(app);
    support::uninstall_update_signal();

    if observed {
        Ok(())
    } else {
        Err(format!(
            "accepted source `{source}` and relay-list `{relay_list}`, accepted target note `{note}`, but the app feed did not observe the note within {APP_WAIT:?}",
            source = mute_list.id.to_hex(),
            relay_list = relay_list.id.to_hex(),
            note = note_id,
        ))
    }
}
