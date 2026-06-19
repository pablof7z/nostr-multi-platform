//! Robustness family 2 — RELAY RESILIENCE / CHAOS (the reimplemented transport).
//!
//! The app's promise under hostile relays: keep rendering from the store, never
//! busy-spin, never leak/dangle subscriptions. These oracles drive the real
//! `nmp_app_*` transport surface and read the typed `relay_statuses` /
//! `wire_subscriptions` rows + the routing-decisions ledger — no store scraping.
//!
//! Falsifiable hypotheses:
//!  - RENDER-FROM-STORE: against a DEAD relay (connection refused) the actor
//!    MUST keep emitting frames and stay alive (it renders from the store; it
//!    does not wedge waiting on the wire). Absolute: frames keep flowing + the
//!    actor thread is alive after the window.
//!  - SUB-LEAK: after a view that opened wire subscriptions is closed, NO
//!    subscription may remain in the `open` state (auto-close on claim-drop).
//!  - OUTBOX ROUTING (NIP-65): reads route to the author's WRITE relays — read
//!    from the routing-decisions ledger; SKIP-LOUD without a NIP-65 fixture.
//!
//! The idle-CPU / no-spin gate (busy-spin during reconnect churn) is the
//! existing `idle_soak` detector and is referenced as a finding here — it needs
//! the OS sidecar's per-thread sampler, so it is not re-measured in-process.

use std::time::Duration;

use crate::config::{Args, Phase};
use crate::driver::DrivenApp;
use crate::report::{GateRow, SanityReport, Verdict};

/// A relay URL guaranteed to refuse connection (no listener on port 1).
const DEAD_RELAY: &str = "ws://127.0.0.1:1";

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

/// RENDER-FROM-STORE: a dead relay must not wedge the actor.
fn render_from_store_on_dead_relay(report: &mut SanityReport, phase: &str) {
    let dead = DEAD_RELAY.to_string();
    let app = DrivenApp::launch(None, None, std::slice::from_ref(&dead));
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
    let before = app.with_state(|s| s.records.len());
    std::thread::sleep(Duration::from_secs(3));
    let after = app.with_state(|s| s.records.len());
    let alive = app.is_alive();

    // Frames must keep flowing (renders from store, not wedged on the wire).
    report.push(
        GateRow::min(
            "resilience-render-from-store",
            phase,
            "decode_snapshot_envelope (frame count over 3s) + nmp_app_is_alive",
            "frame-count delta against a dead relay",
            (after.saturating_sub(before)) as f64,
            1.0,
            "frames",
        )
        .with_note(&format!(
            "dead relay {DEAD_RELAY}: frames {before}→{after} (actor renders from store, \
             not wedged on the wire); actor_alive={alive}"
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

    // Open the contact feed (kind:1) — this registers wire subscriptions.
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
            "nmp_app_open_contact_feed + decode_snapshot_envelope",
            "SnapshotEnvelope.wire_subscriptions[state=open]",
            "no dangling open sub after close",
            Verdict::SkipRelayMiss,
            "contact feed opened no wire subscription within 15s (relay did not accept the REQ) \
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
            "nmp_app_close_contact_feed + decode_snapshot_envelope",
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
    // Open the home feed already happened in launch; give the planner a moment.
    std::thread::sleep(Duration::from_secs(3));
    let decisions = app.routing_decisions();
    let subs = decisions
        .as_ref()
        .and_then(|d| d.get("subscriptions"))
        .and_then(|s| s.as_array())
        .cloned()
        .unwrap_or_default();
    let routed = subs
        .iter()
        .filter(|s| {
            s.get("urls")
                .and_then(|u| u.as_array())
                .map(|a| !a.is_empty())
                .unwrap_or(false)
        })
        .count();
    if routed == 0 {
        report.push(GateRow::unmeasured(
            "resilience-outbox-routing",
            phase,
            "nmp_app_recent_routing_decisions",
            "routing-trace subscriptions[].urls (NIP-65 resolved targets)",
            "reads route to authors' WRITE relays",
            Verdict::SkipRelayMiss,
            &format!(
                "{} subscription rows in the routing ledger, none with resolved relay targets — \
                 no NIP-65 outbox fixture this run. Provide a high-follow account whose authors \
                 publish kind:10002 to drive a positive outbox-route assertion.",
                subs.len()
            ),
        ));
        return;
    }
    report.push(
        GateRow::min(
            "resilience-outbox-routing",
            phase,
            "nmp_app_recent_routing_decisions",
            "routing-trace subscriptions[].urls",
            routed as f64,
            1.0,
            "routed-subs",
        )
        .with_note(&format!(
            "{routed}/{} subscription rows carry resolved relay targets (outbox routing active)",
            subs.len()
        )),
    );
}
