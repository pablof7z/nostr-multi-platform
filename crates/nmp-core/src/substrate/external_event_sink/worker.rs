//! The dispatcher's background worker — in-process relay fan-out.
//!
//! [`run_worker`] is the thread loop: it drains [`DispatchWork`] off the
//! bounded channel and, for each frame, runs every matching relay-forwarding
//! policy and sends `["EVENT", <canonical_json>]` to each resolved relay
//! target. All of this happens on the worker thread, never the actor thread.
//!
//! **Panic isolation.** A policy's `destinations()` is wrapped so a panic
//! increments a diagnostic counter and is contained — one bad policy never
//! kills the worker or starves the others.

use std::sync::mpsc::Receiver;

use nmp_network::pool::WireFrame;

use super::diagnostics::ExternalEventSinkDiagnostics;
use super::dispatcher::{DispatchWork, Runtime};
use super::SinkDestination;

/// The dispatcher's worker thread. Runs until the inbound channel closes (all
/// `tx` clones dropped, i.e. the dispatcher is gone).
pub(super) fn run_worker(
    rx: Receiver<DispatchWork>,
    runtime: Option<&Runtime>,
    diagnostics: &ExternalEventSinkDiagnostics,
) {
    while let Ok(work) = rx.recv() {
        fan_out_relays(&work, runtime, diagnostics);
    }
}

/// Relay forwarding: run each policy's `destinations()` under panic isolation
/// and send `["EVENT", <canonical_json>]` to each resolved relay target ONCE.
fn fan_out_relays(
    work: &DispatchWork,
    runtime: Option<&Runtime>,
    diagnostics: &ExternalEventSinkDiagnostics,
) {
    let Some(rt) = runtime else {
        return; // not yet bound — no Pool to send on
    };
    let mut frame_text: Option<String> = None;
    for registered in &work.policies {
        let policy = &registered.policy;
        let frame = &work.frame;
        // Isolate a panicking policy: count it and keep going on the others.
        let destinations = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            policy.destinations(frame)
        })) {
            Ok(d) => d,
            Err(_) => {
                diagnostics.inc_policy_panics();
                continue;
            }
        };
        if destinations.is_empty() {
            continue;
        }
        // Build the wire frame at most once across all destinations/policies.
        let text =
            frame_text.get_or_insert_with(|| format!(r#"["EVENT",{}]"#, work.frame.canonical_json));
        for dest in destinations {
            match dest {
                SinkDestination::Relay(target) => {
                    let handle = rt
                        .pool
                        .ensure_open_with_role(&target.relay_url, target.relay_role);
                    let enqueued = rt.pool.send(handle, WireFrame::Text(text.clone()));
                    tracing::debug!(
                        target: "nmp.external_event_sink.worker",
                        event_id = %work.frame.raw.id,
                        kind = work.frame.raw.kind,
                        relay_url = %target.relay_url,
                        relay_role = ?target.relay_role,
                        enqueued,
                        "forwarded external event sink frame to relay"
                    );
                }
            }
        }
    }
}
