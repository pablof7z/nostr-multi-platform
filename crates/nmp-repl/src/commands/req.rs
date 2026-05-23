//! `req <filter-fields...>` — the headline command. Drives the **production**
//! [`nmp_core::subs::SubscriptionLifecycle`] (not a hand-rolled outbox) and
//! renders the live per-relay status table.
//!
//! ## The tick loop
//!
//! 1. Apply session config onto the per-session lifecycle.
//! 2. Expand `$follows` (thin kind:3 fetch — variable resolution, kept).
//! 3. Build one `LogicalInterest`, replace the registry slot (`InterestId(1)`).
//! 4. Discovery convergence loop:
//!    - `recompile_and_diff(&cache)` → frames.
//!    - Partition: `mailbox-probe-*` `sub_ids` = discovery REQs.
//!    - If probes exist: run them against the indexer, `cache.put` each
//!      kind:10002, `enqueue_trigger(Nip65Arrived)`, then `drain_tick` and
//!      loop. `probed_mailboxes` dedups so this converges in 1–2 passes.
//!    - When a compile yields no probe frames, discovery is done.
//! 5. Take the lifecycle's *current plan* content frames (post-
//!    `apply_selection`) via `current_plan_frames()`, partition per relay,
//!    fan out, live render.
//!
//! Intermediate compiles' content frames are intentionally never wired —
//! they were computed against a partial cache. Only the converged plan
//! (materialised by `current_plan_frames`) is fanned out.

use std::collections::BTreeMap;
use std::time::Instant;

use nmp_core::subs::{CompileTrigger, WireFrame};
use serde_json::Value;

use crate::ast::FilterAst;
use crate::error::{ReplError, Result};
use crate::fanout::{self, ContentReq};
use crate::plan;
use crate::render;
use crate::session::{RunSummary, Session};

/// Max discovery iterations before we stop converging (defensive — the
/// `probed_mailboxes` dedup means real convergence is 1–2 passes).
const MAX_DISCOVERY_ITERS: usize = 6;

pub fn run(session: &mut Session, filter: FilterAst) -> Result<()> {
    let line_for_summary = format!("{filter:?}");
    let started = Instant::now();

    // ── 1. Apply session config onto the lifecycle ───────────────────────
    session
        .lifecycle
        .set_indexer_relays(session.indexer_relays.clone());
    session.lifecycle.set_app_relays(session.app_relays.clone());
    session
        .lifecycle
        .set_selection_budget(session.max_connections, session.max_per_user);
    for dead in &session.dead_relays {
        session.lifecycle.mark_relay_dead(dead.clone());
    }

    // ── 2. Expand $follows (thin kind:3 fetch — variable resolution) ─────
    let follows = if plan::needs_follows(&filter) {
        let r = crate::discovery::fetch_follows(session)?;
        if r.cached {
            println!("  follows: cached ({} follows)", r.follows.len());
        } else {
            println!(
                "  follows: fetched {} follows in {}ms",
                r.follows.len(),
                r.elapsed.as_millis()
            );
        }
        r.follows
    } else {
        std::collections::BTreeSet::new()
    };

    // ── 3. Build the interest, replace the registry slot ─────────────────
    let interest = plan::build_interest(session, &filter, &follows)?;
    session.lifecycle.registry_mut().push(interest);

    // ── 4. Discovery convergence loop ────────────────────────────────────
    let discovery_started = Instant::now();
    let mut probed_total = 0usize;
    let mut iters = 0usize;
    loop {
        iters += 1;
        let frames = session
            .lifecycle
            .recompile_and_diff(&session.mailbox_cache)
            .map_err(|e| ReplError::Planner(format!("{e:?}")))?;

        let probes: Vec<(String, String, String)> = frames
            .iter()
            .filter_map(|f| match f {
                WireFrame::Req {
                    relay_url,
                    sub_id,
                    filter_json,
                    ..
                } if sub_id.starts_with("mailbox-probe-") => {
                    Some((relay_url.clone(), sub_id.clone(), filter_json.clone()))
                }
                _ => None,
            })
            .collect();

        if probes.is_empty() {
            break;
        }

        let probe_authors: usize =
            probes.iter().map(|(_, _, fj)| author_count(fj)).sum();
        probed_total += probe_authors;

        // `run_discovery` prints a per-REQ row for EVERY implicit
        // kind:10002 probe (relay + filter summary + sub_id + terminal
        // status). No aggregation hides an implicit REQ.
        let snapshots = fanout::run_discovery(&probes);
        for (pubkey, snap) in snapshots {
            session.mailbox_cache.put(pubkey.clone(), snap);
            session
                .lifecycle
                .enqueue_trigger(CompileTrigger::Nip65Arrived {
                    pubkey,
                    created_at: 0,
                });
        }

        // Drain the Nip65Arrived triggers → next plan routes via NIP-65.
        let _ = session.lifecycle.drain_tick(&session.mailbox_cache);

        if iters >= MAX_DISCOVERY_ITERS {
            println!("  discovery: stopped after {iters} iterations (convergence guard)");
            break;
        }
    }
    let discovery_elapsed = discovery_started.elapsed();
    if probed_total > 0 {
        println!(
            "  discovery: converged in {} pass{} ({}ms, {} probed)",
            iters,
            if iters == 1 { "" } else { "es" },
            discovery_elapsed.as_millis(),
            probed_total
        );
    }

    // ── 5. Materialise the converged content plan ────────────────────────
    // `current_plan` is the converged plan after the loop's last
    // recompile/drain. `current_plan_frames()` materialises the FULL REQ
    // set (the diff would only show the delta). Probe REQs are absent by
    // construction (they live outside `current_plan`).
    let unroutable = session.lifecycle.current_plan_unroutable().len();
    let content_reqs: Vec<ContentReq> = session
        .lifecycle
        .current_plan_frames()
        .iter()
        .filter_map(|f| match f {
            WireFrame::Req {
                relay_url,
                sub_id,
                filter_json,
                ..
            } if !sub_id.starts_with("mailbox-probe-") => Some(ContentReq {
                relay: relay_url.clone(),
                sub_id: sub_id.clone(),
                authors: author_count(filter_json),
                filter_json: filter_json.clone(),
            }),
            _ => None,
        })
        .collect();

    let per_relay: BTreeMap<String, Vec<ContentReq>> = partition(&content_reqs);
    let authors_on_wire: usize = per_relay
        .values()
        .flat_map(|v| v.iter())
        .map(|r| r.authors)
        .sum();

    render::print_outbox_line(per_relay.len(), authors_on_wire, unroutable);

    if per_relay.is_empty() {
        println!("  (no relays in plan — nothing to fan out to)");
        session.last_run = Some(RunSummary {
            command_line: line_for_summary,
            relays_used: 0,
            authors_on_wire,
            unroutable,
            events_total: 0,
            events_new: 0,
            wall: started.elapsed(),
        });
        return Ok(());
    }

    // ── Content fanout + live render ─────────────────────────────────────
    let (rx, _workers, deadline) = fanout::launch(&per_relay, session.wall);
    let summary = render::drive(session, rx, &per_relay, deadline);

    session.last_run = Some(RunSummary {
        command_line: line_for_summary,
        relays_used: summary.relays,
        authors_on_wire,
        unroutable,
        events_total: summary.deliveries,
        events_new: summary.new_events,
        wall: started.elapsed(),
    });
    Ok(())
}

/// Partition a flat content-REQ list into a per-relay map.
fn partition(reqs: &[ContentReq]) -> BTreeMap<String, Vec<ContentReq>> {
    let mut map: BTreeMap<String, Vec<ContentReq>> = BTreeMap::new();
    for r in reqs {
        map.entry(r.relay.clone()).or_default().push(r.clone());
    }
    map
}

/// Count authors in a filter JSON object (for the row label / diagnostics).
fn author_count(filter_json: &str) -> usize {
    serde_json::from_str::<Value>(filter_json)
        .ok()
        .and_then(|v| v.get("authors").and_then(Value::as_array).map(std::vec::Vec::len))
        .unwrap_or(0)
}
