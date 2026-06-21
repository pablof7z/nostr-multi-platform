//! Robustness family 2 — RELAY RESILIENCE / CHAOS (the reimplemented transport).
//!
//! The app's promise under hostile relays: keep rendering from the store, never
//! busy-spin, never leak/dangle subscriptions. These oracles drive the real
//! `nmp_app_*` transport surface and read the typed `relay_statuses` /
//! `wire_subscriptions` rows + the routing-decisions ledger — no store scraping.
//!
//! Falsifiable hypotheses:
//!  - STORE-SERVES-WHILE-RELAY-DEAD: against a DEAD relay (connection refused)
//!    a known locally-seeded corpus MUST remain readable from the STORE (served
//!    from local state, not the wire), and the actor MUST stay alive. This is a
//!    store-serve assertion, NOT a render/projection claim — the typed snapshot
//!    envelope exposes only `visible_items` (a count) with no per-item author
//!    hex and no `nmp_app_read_feed_authors` seam, so the rendered rows cannot
//!    be checked for the seeded ids; the gate is named for exactly the property
//!    it proves (store-readable off-wire), not "render-from-store".
//!  - SUB-LEAK: after a view that opened wire subscriptions is closed, NO
//!    subscription may remain in the `open` state (auto-close on claim-drop).
//!  - OUTBOX ROUTING (NIP-65): reads route to the author's WRITE relays — read
//!    from the routing-decisions ledger; SKIP-LOUD without a NIP-65 fixture.
//!
//! The idle-CPU / no-spin gate (busy-spin during reconnect churn) is the
//! existing `idle_soak` detector and is referenced as a finding here — it needs
//! the OS sidecar's per-thread sampler, so it is not re-measured in-process.

use std::collections::HashSet;
use std::time::Duration;

use nostr::{EventBuilder, JsonUtil, Keys, Timestamp, ToBech32};

use crate::config::{Args, Phase};
use crate::driver::DrivenApp;
use crate::report::{GateRow, SanityReport, Verdict};

/// A relay URL guaranteed to refuse connection (no listener on port 1).
const DEAD_RELAY: &str = "ws://127.0.0.1:1";

/// How many self-authored notes the render-from-store oracle seeds.
const SEED_NOTES: u64 = 20;

pub fn run_resilience(report: &mut SanityReport, args: &Args) {
    let phase = Phase::Resilience.as_str();
    render_from_store_on_dead_relay(report, phase);
    sub_leak(report, phase, args);
    outbox_routing(report, phase, args);

    report.finding(
        "BUSY-SPIN under reconnect churn (family 2) — the idle-cpu / no-spin gates in the \
         idle_soak phase ARE the detector for a reconnect-storm wakeup/poll regression. They \
         need the OS sidecar's per-thread sampler (scripts/perf-sanity). Run \
         `--phase idle_soak --os-metrics <json>` against a flapping relay to assert CPU stays flat.",
    );
}

/// STORE-SERVES-WHILE-RELAY-DEAD: against a DEAD relay, a known locally-seeded
/// item must be readable from the STORE (served from local state, not the wire),
/// AND the actor must stay live. The prior version only proved frame/actor
/// LIVENESS — it never seeded a store item. An earlier strengthening seeded a
/// corpus and proved store-readability, but the gate was NAMED
/// "render-from-store" while only proving STORE-READABLE — it never asserted the
/// seeded ids appear in the rendered projection, so a render-ignores-store
/// regression would still PASS. That render assertion is not reachable here: the
/// typed snapshot envelope carries only `visible_items` (a count, no per-item
/// author hex) and there is no `nmp_app_read_feed_authors` seam, so the rendered
/// rows cannot be checked for the seeded ids, and self-authored notes do not
/// reliably surface in the home feed anyway (the reactive phase documents the
/// same no-self-include limitation). Honest fix: NAME the gate for exactly the
/// property it proves — `resilience-store-serves-while-relay-dead` — and assert
/// only that every seeded id is store-readable via `nmp_app_read_author_event_ids`
/// WHILE the relay is down. No "render" claim.
fn render_from_store_on_dead_relay(report: &mut SanityReport, phase: &str) {
    let dead = DEAD_RELAY.to_string();
    // Known key so we can read our own seeded events back out of the store.
    let keys = Keys::generate();
    let nsec = keys.secret_key().to_bech32().unwrap_or_default();
    let pubkey_hex = keys.public_key().to_hex();
    let app = DrivenApp::launch(Some(&nsec), Some(&pubkey_hex), std::slice::from_ref(&dead));
    // Prove the actor came up at all.
    if app
        .wait_until(Duration::from_secs(10), |s| !s.records.is_empty())
        .is_none()
    {
        report.push(GateRow::unmeasured(
            "resilience-actor-liveness",
            phase,
            "decode_snapshot_envelope",
            "first SnapshotFrame",
            ">= 1 frame in 10s",
            Verdict::Blocked,
            "actor emitted no frame against a dead relay — kernel did not start (BLOCKED)",
        ));
        return;
    }

    // Seed a known corpus while the only configured relay is dead. These events
    // never touch the wire — they go straight to the store via the inject seam.
    let base = crate::report::now_unix();
    let seed_ids: HashSet<String> = (0..SEED_NOTES)
        .filter_map(|i| {
            let ev: nostr::Event = EventBuilder::text_note(format!("render-from-store seed {i}"))
                .custom_created_at(Timestamp::from(base + i))
                .sign_with_keys(&keys)
                .ok()?;
            let id = ev.id.to_hex();
            let c = std::ffi::CString::new(ev.as_json()).ok()?;
            nmp_ffi::nmp_app_inject_signed_event_json(app.raw(), c.as_ptr()).then_some(id)
        })
        .collect();

    if seed_ids.is_empty() {
        // No seed accepted — without a populated store the render assertion
        // would be vacuous. SKIP LOUD rather than pass on an empty store.
        let alive = app.is_alive();
        report.push(GateRow::unmeasured(
            "resilience-store-serves-while-relay-dead",
            phase,
            "nmp_app_inject_signed_event_json + nmp_app_read_author_event_ids (dead relay)",
            "seeded ids readable from the store while the relay is dead",
            "all seeded ids store-readable",
            Verdict::SkipRelayMiss,
            "no seed event was accepted into the store — cannot assert store-serve-while-dead this \
             run (SKIP LOUD, never a vacuous pass)",
        ));
        report.push(GateRow::min(
            "resilience-actor-survives-dead-relay",
            phase,
            "nmp_app_is_alive",
            "actor thread liveness",
            if alive { 1.0 } else { 0.0 },
            1.0,
            "alive",
        ));
        return;
    }

    if !app.wait_barrier(Duration::from_secs(5)) {
        let alive = app.is_alive();
        report.push(GateRow::unmeasured(
            "resilience-store-serves-while-relay-dead",
            phase,
            "nmp_app_inject_signed_event_json + actor barrier",
            "barrier ack before store read",
            "seed injections settled before store assertion",
            Verdict::Blocked,
            "actor barrier did not settle after seeding the dead-relay store corpus — cannot read \
             the store without risking a stale assertion",
        ));
        report.push(GateRow::min(
            "resilience-actor-survives-dead-relay",
            phase,
            "nmp_app_is_alive",
            "actor thread liveness",
            if alive { 1.0 } else { 0.0 },
            1.0,
            "alive",
        ));
        return;
    }

    // Read the seeded items back from the STORE while the relay is still dead.
    let stored_ids: HashSet<String> = {
        let pk = std::ffi::CString::new(pubkey_hex.as_str()).ok();
        let ptr = pk.map(|pk| nmp_ffi::nmp_app_read_author_event_ids(app.raw(), pk.as_ptr(), 0));
        match ptr {
            Some(ptr) if !ptr.is_null() => {
                let parsed = unsafe { std::ffi::CStr::from_ptr(ptr) }
                    .to_str()
                    .ok()
                    .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok());
                nmp_ffi::nmp_free_string(ptr);
                parsed
                    .and_then(|v| {
                        v.as_array().map(|arr| {
                            arr.iter()
                                .filter_map(|o| o["id"].as_str().map(str::to_owned))
                                .collect()
                        })
                    })
                    .unwrap_or_default()
            }
            _ => HashSet::new(),
        }
    };
    let alive = app.is_alive();
    let missing = seed_ids
        .iter()
        .filter(|id| !stored_ids.contains(*id))
        .count();

    // Every seeded id must be store-readable WHILE the relay is dead: the
    // app serves it from the populated store, not the wire. (This is a
    // store-serve assertion, NOT a render/projection claim — see the fn doc.)
    report.push(
        GateRow::max(
            "resilience-store-serves-while-relay-dead",
            phase,
            "nmp_app_read_author_event_ids (dead relay)",
            "seeded ids missing from the store while the relay is dead",
            missing as f64,
            0.0,
            "missing-seeded-ids",
        )
        .with_note(&format!(
            "dead relay {DEAD_RELAY}: seeded {} self-authored notes off-wire; {} store-readable, \
             {missing} missing after actor barrier settled=true — the populated store SERVES the \
             seeded items with NO live relay (store-readable, not a render/projection claim); \
             actor_alive={alive}",
            seed_ids.len(),
            stored_ids.len(),
        )),
    );

    // The actor thread must survive a dead-relay connect (no panic/wedge).
    report.push(GateRow::min(
        "resilience-actor-survives-dead-relay",
        phase,
        "nmp_app_is_alive",
        "actor thread liveness",
        if alive { 1.0 } else { 0.0 },
        1.0,
        "alive",
    ));
}

/// SUB-LEAK: open a view's wire subscriptions, then close it; assert no `open`
/// subscription dangles after the close settles.
fn sub_leak(report: &mut SanityReport, phase: &str, args: &Args) {
    let Some(app) = super::connect_or_skip(report, phase, args) else {
        return;
    };
    let baseline_open = app.with_state(|s| s.latest_open_sub_count());

    // Open the active-follows feed through the legacy C shim.
    let kinds = std::ffi::CString::new("[1]").unwrap();
    nmp_ffi::nmp_app_open_contact_feed(app.raw(), kinds.as_ptr());
    let opened = app
        .wait_until(Duration::from_secs(15), |s| {
            s.latest_open_sub_count() > baseline_open
        })
        .is_some();
    if !opened {
        report.push(GateRow::unmeasured(
            "resilience-sub-leak",
            phase,
            "legacy active-follows open shim + decode_snapshot_envelope",
            "SnapshotEnvelope.wire_subscriptions[state=open]",
            "no dangling open sub after close",
            Verdict::SkipRelayMiss,
            "active-follows feed opened no wire subscription within 15s (relay did not accept the REQ) \
             — cannot assert the leak invariant this run; SKIP LOUD (never fake green)",
        ));
        return;
    }
    let peak_open = app.with_state(|s| s.latest_open_sub_count());

    // Close the view; the lifecycle registry emits CLOSE frames on the next
    // idle tick. After settling, NO open sub may remain above baseline.
    nmp_ffi::nmp_app_close_contact_feed(app.raw());
    let drained = app
        .wait_until(Duration::from_secs(15), |s| {
            s.latest_open_sub_count() <= baseline_open
        })
        .is_some();
    let final_open = app.with_state(|s| s.latest_open_sub_count());
    let leaked = final_open.saturating_sub(baseline_open);
    report.push(
        GateRow::max(
            "resilience-sub-leak",
            phase,
            "legacy active-follows close shim + decode_snapshot_envelope",
            "SnapshotEnvelope.wire_subscriptions[state=open] after close",
            leaked as f64,
            0.0,
            "dangling-subs",
        )
        .with_note(&format!(
            "open subs baseline={baseline_open}, peak={peak_open}, after-close={final_open} \
             (drained={drained}); a non-zero residual is a leaked/dangling subscription"
        )),
    );
}

/// OUTBOX ROUTING: read the routing-decisions ledger and assert subscriptions
/// carry resolved relay targets (NIP-65 write relays for the followed authors).
/// Without a NIP-65 fixture there is nothing to positively assert → SKIP-LOUD.
fn outbox_routing(report: &mut SanityReport, phase: &str, args: &Args) {
    let Some(app) = super::connect_or_skip(report, phase, args) else {
        return;
    };
    // Also inspect publishes[] for context, but the gate passes only on
    // subscription rows with `Nip65/Read`: that is the route shape that proves
    // follow-feed reads used authors' mailbox relays instead of an AppRelay or
    // user-configured fallback. Publish rows require `Nip65/Write` and are
    // reported separately in the note; they do not satisfy the read oracle.
    let nip65_routed = |rows: &[serde_json::Value], direction: &str| -> usize {
        rows.iter()
            .filter(|row| {
                row.get("urls")
                    .and_then(|u| u.as_array())
                    .map(|urls| {
                        urls.iter().any(|entry| {
                            entry
                                .get("lanes")
                                .and_then(|l| l.as_array())
                                .map(|lanes| {
                                    lanes.iter().any(|lane| {
                                        lane.get("kind").and_then(|k| k.as_str()) == Some("Nip65")
                                            && lane.get("direction").and_then(|d| d.as_str())
                                                == Some(direction)
                                    })
                                })
                                .unwrap_or(false)
                        })
                    })
                    .unwrap_or(false)
            })
            .count()
    };
    if !app.wait_barrier(Duration::from_secs(3)) {
        report.push(GateRow::unmeasured(
            "resilience-outbox-routing",
            phase,
            "actor barrier + nmp_app_recent_routing_decisions",
            "barrier ack before routing-ledger read",
            "actor settled before route assertion",
            Verdict::Blocked,
            "actor barrier did not settle before the routing-ledger read — cannot assert NIP-65 \
             routing without risking a stale read",
        ));
        return;
    }
    let decisions = app.routing_decisions();
    let subs = decision_rows(decisions.as_ref(), "subscriptions");
    let publishes = decision_rows(decisions.as_ref(), "publishes");
    let read_routed = nip65_routed(&subs, "Read");
    let write_routed = nip65_routed(&publishes, "Write");
    if read_routed == 0 {
        report.push(GateRow::unmeasured(
            "resilience-outbox-routing",
            phase,
            "nmp_app_recent_routing_decisions",
            "routing-trace subscriptions[].urls[].lanes[] kind==Nip65 direction==Read",
            "at least one subscription target resolved via the NIP-65 read lane",
            Verdict::SkipRelayMiss,
            &format!(
                "{} subscription + {} publish rows in the routing ledger, NONE with a \
                 subscription urls[].lanes[] entry of Nip65/Read (publish Nip65/Write rows={write_routed}; \
                 actor barrier settled=true) — every read target came from an \
                 AppRelay fallback / user-configured lane, NOT NIP-65 read routing. No NIP-65 \
                 fixture this run. Provide a high-follow account whose authors publish kind:10002 \
                 to drive a positive read-route assertion. SKIP LOUD.",
                subs.len(),
                publishes.len(),
            ),
        ));
        return;
    }
    report.push(
        GateRow::min(
            "resilience-outbox-routing",
            phase,
            "nmp_app_recent_routing_decisions",
            "routing-trace subscriptions[].urls[].lanes[] kind==Nip65 direction==Read",
            read_routed as f64,
            1.0,
            "nip65-read-subscription-rows",
        )
        .with_note(&format!(
            "{read_routed} subscription routing-ledger row(s) carry a NIP-65 read lane on a \
             resolved target — reads route to authors' write relays via NIP-65, not the \
             AppRelay fallback (subs={}, publishes={}, publish Nip65/Write rows={write_routed}, \
             actor barrier settled=true)",
            subs.len(),
            publishes.len()
        )),
    );
}

fn decision_rows(decisions: Option<&serde_json::Value>, key: &str) -> Vec<serde_json::Value> {
    decisions
        .and_then(|d| d.get(key))
        .and_then(|rows| rows.as_array())
        .cloned()
        .unwrap_or_default()
}
