//! `req <filter-fields...>` — the headline command. Drives discovery,
//! compiles a plan, fans out, and renders the live status table.

use std::time::Instant;

use crate::ast::FilterAst;
use crate::error::Result;
use crate::fanout;
use crate::plan;
use crate::render;
use crate::session::{RunSummary, Session};

pub fn run(session: &mut Session, filter: FilterAst) -> Result<()> {
    let line_for_summary = format!("{filter:?}");

    let prepared = plan::prepare(session, &filter)?;

    // Phase A line.
    if prepared.follows_used {
        if prepared.phase_a_cached {
            println!(
                "  phase A: cached ({} follows)",
                prepared.phase_a_count
            );
        } else {
            println!(
                "  phase A: fetched {} follows in {}ms",
                prepared.phase_a_count,
                prepared.phase_a_elapsed.as_millis()
            );
        }
    }
    // Phase B line. The `have/queried` asymmetry is the load-bearing
    // unroutable-surface diagnostic (design §13.8) — never collapse it.
    if prepared.phase_b_queried > 0 {
        println!(
            "  phase B: {}/{} mailboxes (cached {}, fetched {} in {}ms)",
            prepared.phase_b_have,
            prepared.phase_b_queried,
            prepared.phase_b_cached,
            prepared.phase_b_fetched,
            prepared.phase_b_elapsed.as_millis()
        );
    }

    // Phase C summary line.
    render::print_outbox_line(
        prepared.per_relay_authors.len(),
        prepared.authors_on_wire,
        prepared.unroutable,
    );

    if prepared.per_relay_authors.is_empty() {
        println!("  (no relays in plan — nothing to fan out to)");
        return Ok(());
    }

    // Phase D — fanout + live render.
    let started = Instant::now();
    let (rx, _workers, deadline) =
        fanout::launch(&prepared.per_relay_authors, prepared.filter.clone(), session.wall);
    let summary = render::drive(session, rx, &prepared.per_relay_authors, deadline);

    session.last_run = Some(RunSummary {
        command_line: line_for_summary,
        relays_used: summary.relays,
        authors_on_wire: prepared.authors_on_wire,
        unroutable: prepared.unroutable,
        events_total: summary.deliveries,
        events_new: summary.new_events,
        wall: started.elapsed(),
    });
    Ok(())
}
