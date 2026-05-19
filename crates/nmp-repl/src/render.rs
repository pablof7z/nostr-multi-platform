//! Live status renderer. Owns the terminal between rustyline `readline()`
//! calls. No concurrent painting; the main thread drains the fanout
//! channel and updates rows in place.
//!
//! `--json` mode short-circuits the table and emits one JSON line per
//! `RelayEvent` state transition.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{stdout, Write};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crossterm::{
    cursor::{MoveTo, MoveToColumn},
    style::{Color, Print, ResetColor, SetForegroundColor},
    terminal::{Clear, ClearType},
    ExecutableCommand, QueueableCommand,
};
use serde_json::json;

use crate::fanout::{ContentReq, RelayEvent, RelayStats};
use crate::session::Session;
use crate::ws::{summarize_filter, truncate};

/// Per-relay author count for the row label — sum across the relay's
/// content REQs (the lifecycle may assign more than one sub-shape).
fn relay_author_count(reqs: &[ContentReq]) -> usize {
    reqs.iter().map(|r| r.authors).sum()
}

#[derive(Clone, Debug)]
enum RowState {
    Connecting,
    ReqSent,
    Receiving,
    Eose { elapsed: Duration },
    /// Relay closed THIS sub — verbatim reason (e.g. rate limit). Terminal.
    Closed { msg: String },
    /// Relay demanded NIP-42 AUTH; read-only REPL won't respond. Terminal.
    Auth,
    Error { msg: String },
    Timeout,
}

#[derive(Clone, Debug)]
struct Row {
    relay: String,
    /// Wire sub_id — the row key. One row per REQ.
    sub_id: String,
    /// Compact filter summary, e.g. `kind:1 (83 authors)`.
    summary: String,
    authors: usize,
    state: RowState,
    /// Last NOTICE seen on this sub's socket (non-terminal, shown inline).
    notice: Option<String>,
    events: u64,
    new: u64,
    elapsed: Option<Duration>,
}

/// Aggregate stats returned to the caller for the post-run summary line and
/// the `RunSummary` in the session.
#[derive(Default, Debug)]
pub struct FanoutSummary {
    pub deliveries: u64,
    pub unique_events: u64,
    pub new_events: u64,
    pub wall: Duration,
    pub relays: usize,
    pub per_relay: BTreeMap<String, RelayStats>,
}

/// Drain the worker channel, painting rows in place. Returns the aggregate
/// summary when all workers exit or the wall deadline elapses.
pub fn drive(
    session: &mut Session,
    rx: mpsc::Receiver<RelayEvent>,
    per_relay: &BTreeMap<String, Vec<ContentReq>>,
    wall_deadline: Instant,
) -> FanoutSummary {
    if session.json {
        return drive_json(session, rx, per_relay, wall_deadline);
    }
    drive_table(session, rx, per_relay, wall_deadline)
}

fn drive_table(
    session: &mut Session,
    rx: mpsc::Receiver<RelayEvent>,
    per_relay: &BTreeMap<String, Vec<ContentReq>>,
    wall_deadline: Instant,
) -> FanoutSummary {
    // ── Build the row table. One row PER REQ, keyed by `(relay, sub_id)`
    // — NOT sub_id alone: two relays carrying the same filter hash get the
    // same sub_id string (the lifecycle keys `known_subs` by
    // `(relay_url, sub_id)` for exactly this reason), and a sub_id-only key
    // would route both relays' events to one row and strand the other in
    // `Connecting`. Relays sorted by total author count desc; within a
    // relay, by sub_id. ─────────────────────────────────────────────────
    let mut rows_by_sub: HashMap<(String, String), usize> = HashMap::new();
    let mut rows: Vec<Row> = Vec::new();
    let mut pairs: Vec<(&String, &Vec<ContentReq>)> = per_relay.iter().collect();
    pairs.sort_by(|a, b| {
        relay_author_count(b.1)
            .cmp(&relay_author_count(a.1))
            .then_with(|| a.0.cmp(b.0))
    });
    for (relay, reqs) in &pairs {
        let mut sorted: Vec<&ContentReq> = reqs.iter().collect();
        sorted.sort_by(|a, b| a.sub_id.cmp(&b.sub_id));
        for req in sorted {
            rows_by_sub.insert(((*relay).clone(), req.sub_id.clone()), rows.len());
            rows.push(Row {
                relay: (*relay).clone(),
                sub_id: req.sub_id.clone(),
                summary: summarize_filter(&req.filter_json),
                authors: req.authors,
                state: RowState::Connecting,
                notice: None,
                events: 0,
                new: 0,
                elapsed: None,
            });
        }
    }

    let mut stdout = stdout();

    // Print initial rows.
    for r in &rows {
        let _ = writeln!(stdout, "{}", format_row(r, session.verbose));
    }
    let _ = stdout.flush();

    // After the rows, cursor is one line below the last row. Capture the
    // row-zero position by moving up `rows.len()` lines.
    let n_rows = rows.len();
    if n_rows == 0 {
        let _ = writeln!(stdout, "  (no relays in plan)");
        return FanoutSummary::default();
    }

    // Drain.
    let mut deliveries = 0u64;
    let mut unique: HashSet<String> = HashSet::new();
    let mut new_events = 0u64;
    let mut per_relay_stats: BTreeMap<String, RelayStats> = BTreeMap::new();
    let mut needs_repaint: HashSet<usize> = HashSet::new();
    let mut last_paint = Instant::now();
    let started = Instant::now();
    let paint_interval = Duration::from_millis(80);

    loop {
        let now = Instant::now();
        if now >= wall_deadline {
            // Mark anything not in a terminal state as Timeout.
            for r in rows.iter_mut() {
                if matches!(r.state, RowState::Connecting | RowState::ReqSent | RowState::Receiving) {
                    r.state = RowState::Timeout;
                }
            }
            paint_dirty(&mut stdout, &rows, n_rows, &mut needs_repaint, session.verbose, true);
            break;
        }
        let remaining = wall_deadline.saturating_duration_since(now);
        let timeout = remaining.min(Duration::from_millis(100));
        match rx.recv_timeout(timeout) {
            Ok(ev) => apply_event(
                ev,
                &mut rows,
                &rows_by_sub,
                &mut deliveries,
                &mut unique,
                &mut new_events,
                &mut per_relay_stats,
                &mut session.seen_ids,
                &mut needs_repaint,
            ),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                paint_dirty(&mut stdout, &rows, n_rows, &mut needs_repaint, session.verbose, true);
                break;
            }
        }
        if last_paint.elapsed() >= paint_interval {
            paint_dirty(&mut stdout, &rows, n_rows, &mut needs_repaint, session.verbose, false);
            last_paint = Instant::now();
        }
    }

    let wall = started.elapsed();
    let dedup = if deliveries == 0 {
        0.0
    } else {
        unique.len() as f64 / deliveries as f64
    };
    let distinct_relays = rows
        .iter()
        .map(|r| r.relay.as_str())
        .collect::<HashSet<_>>()
        .len();
    let _ = writeln!(
        stdout,
        "  fanout: {} relays, {} REQs, {} deliveries, {} new (dedup {:.2}), wall {:?}",
        distinct_relays,
        rows.len(),
        deliveries,
        new_events,
        dedup,
        wall
    );
    let _ = stdout.flush();

    FanoutSummary {
        deliveries,
        unique_events: unique.len() as u64,
        new_events,
        wall,
        relays: distinct_relays,
        per_relay: per_relay_stats,
    }
}

fn drive_json(
    session: &mut Session,
    rx: mpsc::Receiver<RelayEvent>,
    per_relay: &BTreeMap<String, Vec<ContentReq>>,
    wall_deadline: Instant,
) -> FanoutSummary {
    let started = Instant::now();
    let mut deliveries = 0u64;
    let mut unique: HashSet<String> = HashSet::new();
    let mut new_events = 0u64;
    let mut per_relay_stats: BTreeMap<String, RelayStats> = BTreeMap::new();
    let mut authors_by_relay: HashMap<String, usize> = HashMap::new();
    for (k, v) in per_relay {
        authors_by_relay.insert(k.clone(), relay_author_count(v));
    }

    loop {
        let now = Instant::now();
        if now >= wall_deadline {
            break;
        }
        let remaining = wall_deadline.saturating_duration_since(now);
        let timeout = remaining.min(Duration::from_millis(200));
        match rx.recv_timeout(timeout) {
            Ok(ev) => {
                let line = match &ev {
                    RelayEvent::Connecting { relay, sub_id } => json!({
                        "relay": relay,
                        "sub_id": sub_id,
                        "state": "connecting",
                        "authors": authors_by_relay.get(relay).copied().unwrap_or(0),
                    }),
                    RelayEvent::ReqSent { relay, sub_id } => json!({
                        "relay": relay,
                        "sub_id": sub_id,
                        "state": "req_sent",
                    }),
                    RelayEvent::Frame {
                        relay,
                        sub_id,
                        event_id,
                    } => {
                        deliveries += 1;
                        let is_new = session.seen_ids.insert(event_id.clone());
                        if is_new {
                            new_events += 1;
                        }
                        unique.insert(event_id.clone());
                        let stats = per_relay_stats.entry(relay.clone()).or_default();
                        stats.events += 1;
                        json!({
                            "relay": relay,
                            "sub_id": sub_id,
                            "state": "event",
                            "event_id": event_id,
                            "new": is_new,
                        })
                    }
                    RelayEvent::Eose {
                        relay,
                        sub_id,
                        elapsed,
                    } => {
                        let stats = per_relay_stats.entry(relay.clone()).or_default();
                        stats.eose = true;
                        stats.elapsed = Some(*elapsed);
                        json!({
                            "relay": relay,
                            "sub_id": sub_id,
                            "state": "eose",
                            "elapsed_ms": elapsed.as_millis() as u64,
                        })
                    }
                    RelayEvent::Closed { relay, sub_id, msg } => {
                        let stats = per_relay_stats.entry(relay.clone()).or_default();
                        stats.error = Some(format!("CLOSED: {msg}"));
                        json!({
                            "relay": relay,
                            "sub_id": sub_id,
                            "state": "closed",
                            "msg": msg,
                        })
                    }
                    RelayEvent::Auth { relay, sub_id } => {
                        let stats = per_relay_stats.entry(relay.clone()).or_default();
                        stats.error = Some("AUTH required".to_string());
                        json!({
                            "relay": relay,
                            "sub_id": sub_id,
                            "state": "auth_required",
                        })
                    }
                    RelayEvent::Notice { relay, sub_id, msg } => json!({
                        "relay": relay,
                        "sub_id": sub_id,
                        "state": "notice",
                        "msg": msg,
                    }),
                    RelayEvent::Error { relay, sub_id, msg } => json!({
                        "relay": relay,
                        "sub_id": sub_id,
                        "state": "error",
                        "msg": msg,
                    }),
                    RelayEvent::Done {
                        relay,
                        sub_id,
                        stats,
                    } => {
                        let entry = per_relay_stats.entry(relay.clone()).or_default();
                        entry.authors_in_req = entry.authors_in_req.max(stats.authors_in_req);
                        entry.connected |= stats.connected;
                        entry.eose |= stats.eose;
                        entry.elapsed = entry.elapsed.or(stats.elapsed);
                        if entry.time_to_first.is_none() {
                            entry.time_to_first = stats.time_to_first;
                        }
                        json!({
                            "relay": relay,
                            "sub_id": sub_id,
                            "state": "done",
                            "events": stats.events,
                            "elapsed_ms": stats.elapsed.map(|d| d.as_millis() as u64),
                            "eose": stats.eose,
                            "error": stats.error,
                        })
                    }
                };
                println!("{line}");
            }
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    let wall = started.elapsed();
    println!(
        "{}",
        json!({
            "state": "summary",
            "relays": per_relay.len(),
            "deliveries": deliveries,
            "new": new_events,
            "unique": unique.len(),
            "wall_ms": wall.as_millis() as u64,
        })
    );

    FanoutSummary {
        deliveries,
        unique_events: unique.len() as u64,
        new_events,
        wall,
        relays: per_relay.len(),
        per_relay: per_relay_stats,
    }
}

/// True if `state` is a terminal state that must NOT be downgraded by a
/// later non-terminal event (e.g. a `Done` arriving after `Eose`, or a
/// trailing NOTICE).
fn is_terminal(state: &RowState) -> bool {
    matches!(
        state,
        RowState::Eose { .. }
            | RowState::Closed { .. }
            | RowState::Auth
            | RowState::Error { .. }
            | RowState::Timeout
    )
}

#[allow(clippy::too_many_arguments)]
fn apply_event(
    ev: RelayEvent,
    rows: &mut [Row],
    rows_by_sub: &HashMap<(String, String), usize>,
    deliveries: &mut u64,
    unique: &mut HashSet<String>,
    new_events: &mut u64,
    per_relay_stats: &mut BTreeMap<String, RelayStats>,
    seen_ids: &mut HashSet<String>,
    needs_repaint: &mut HashSet<usize>,
) {
    let bump_dirty = |idx: usize, set: &mut HashSet<usize>| {
        set.insert(idx);
    };
    match ev {
        RelayEvent::Connecting { relay, sub_id } => {
            if let Some(&idx) = rows_by_sub.get(&(relay, sub_id)) {
                rows[idx].state = RowState::Connecting;
                bump_dirty(idx, needs_repaint);
            }
        }
        RelayEvent::ReqSent { relay, sub_id } => {
            if let Some(&idx) = rows_by_sub.get(&(relay, sub_id)) {
                if !is_terminal(&rows[idx].state) {
                    rows[idx].state = RowState::ReqSent;
                    bump_dirty(idx, needs_repaint);
                }
            }
        }
        RelayEvent::Frame {
            relay,
            sub_id,
            event_id,
        } => {
            *deliveries += 1;
            let is_new = seen_ids.insert(event_id.clone());
            if is_new {
                *new_events += 1;
            }
            unique.insert(event_id);
            if let Some(&idx) = rows_by_sub.get(&(relay.clone(), sub_id)) {
                if !is_terminal(&rows[idx].state) {
                    rows[idx].state = RowState::Receiving;
                }
                rows[idx].events += 1;
                if is_new {
                    rows[idx].new += 1;
                }
                bump_dirty(idx, needs_repaint);
            }
            let stats = per_relay_stats.entry(relay).or_default();
            stats.events += 1;
        }
        RelayEvent::Eose {
            relay,
            sub_id,
            elapsed,
        } => {
            if let Some(&idx) = rows_by_sub.get(&(relay.clone(), sub_id)) {
                rows[idx].state = RowState::Eose { elapsed };
                rows[idx].elapsed = Some(elapsed);
                bump_dirty(idx, needs_repaint);
            }
            let stats = per_relay_stats.entry(relay).or_default();
            stats.eose = true;
            stats.elapsed = Some(elapsed);
        }
        RelayEvent::Closed { relay, sub_id, msg } => {
            if let Some(&idx) = rows_by_sub.get(&(relay.clone(), sub_id)) {
                if !matches!(rows[idx].state, RowState::Eose { .. }) {
                    rows[idx].state = RowState::Closed { msg: msg.clone() };
                    bump_dirty(idx, needs_repaint);
                }
            }
            let stats = per_relay_stats.entry(relay).or_default();
            stats.error = Some(format!("CLOSED: {msg}"));
        }
        RelayEvent::Auth { relay, sub_id } => {
            if let Some(&idx) = rows_by_sub.get(&(relay.clone(), sub_id)) {
                if !matches!(rows[idx].state, RowState::Eose { .. }) {
                    rows[idx].state = RowState::Auth;
                    bump_dirty(idx, needs_repaint);
                }
            }
            let stats = per_relay_stats.entry(relay).or_default();
            stats.error = Some("AUTH required".to_string());
        }
        RelayEvent::Notice { relay, sub_id, msg } => {
            // Non-terminal: annotate the row, don't change its state.
            if let Some(&idx) = rows_by_sub.get(&(relay, sub_id)) {
                rows[idx].notice = Some(msg);
                bump_dirty(idx, needs_repaint);
            }
        }
        RelayEvent::Error { relay, sub_id, msg } => {
            if let Some(&idx) = rows_by_sub.get(&(relay.clone(), sub_id)) {
                if !is_terminal(&rows[idx].state) {
                    rows[idx].state = RowState::Error { msg: msg.clone() };
                    bump_dirty(idx, needs_repaint);
                }
            }
            let stats = per_relay_stats.entry(relay).or_default();
            stats.error = Some(msg);
        }
        RelayEvent::Done {
            relay,
            sub_id,
            stats,
        } => {
            if let Some(&idx) = rows_by_sub.get(&(relay.clone(), sub_id)) {
                rows[idx].elapsed = stats.elapsed;
                // Only fill in a terminal state if the row never reached one
                // (no EOSE/CLOSED/AUTH/explicit error arrived live).
                if !is_terminal(&rows[idx].state) {
                    if !stats.connected {
                        rows[idx].state = RowState::Error {
                            msg: stats
                                .error
                                .clone()
                                .unwrap_or_else(|| "connect refused".to_string()),
                        };
                    } else if stats.error.is_some() {
                        rows[idx].state = RowState::Error {
                            msg: stats.error.clone().unwrap_or_default(),
                        };
                    }
                }
                bump_dirty(idx, needs_repaint);
            }
            let entry = per_relay_stats.entry(relay).or_default();
            entry.authors_in_req = entry.authors_in_req.max(stats.authors_in_req);
            entry.connected |= stats.connected;
            entry.eose |= stats.eose;
            entry.elapsed = entry.elapsed.or(stats.elapsed);
            if entry.time_to_first.is_none() {
                entry.time_to_first = stats.time_to_first;
            }
            if entry.error.is_none() {
                entry.error = stats.error;
            }
        }
    }
}

fn paint_dirty(
    out: &mut std::io::Stdout,
    rows: &[Row],
    n_rows: usize,
    dirty: &mut HashSet<usize>,
    verbose: bool,
    force_all: bool,
) {
    if rows.is_empty() {
        return;
    }
    // Move cursor up from below-last-row to row 0.
    let _ = out.queue(MoveToColumn(0));
    let _ = out.queue(crossterm::cursor::MoveUp(n_rows as u16));

    for (i, row) in rows.iter().enumerate() {
        let need = force_all || dirty.contains(&i);
        if need {
            let _ = out.queue(MoveToColumn(0));
            let _ = out.queue(Clear(ClearType::CurrentLine));
            let (color, line) = format_row_with_color(row, verbose);
            if let Some(c) = color {
                let _ = out.queue(SetForegroundColor(c));
            }
            let _ = out.queue(Print(line));
            if color.is_some() {
                let _ = out.queue(ResetColor);
            }
        }
        if i + 1 < n_rows {
            let _ = out.queue(crossterm::cursor::MoveDown(1));
            let _ = out.queue(MoveToColumn(0));
        }
    }
    // Park cursor below the last row.
    let _ = out.queue(MoveToColumn(0));
    let _ = out.queue(crossterm::cursor::MoveDown(1));
    let _ = out.flush();
    dirty.clear();
    // The outer `_ = stdout` macro doesn't run if we returned early.
    let _ = MoveTo(0, 0); // keep crossterm linked.
}

fn format_row(row: &Row, verbose: bool) -> String {
    format_row_with_color(row, verbose).1
}

fn format_row_with_color(row: &Row, verbose: bool) -> (Option<Color>, String) {
    let url = if verbose {
        row.relay.clone()
    } else {
        truncate(&row.relay, 44)
    };
    let (glyph, color, tail) = match &row.state {
        RowState::Connecting => (" ", None, "[connecting…]".to_string()),
        RowState::ReqSent => (" ", None, "[streaming…]".to_string()),
        RowState::Receiving => (
            " ",
            None,
            format!("{} events  {}/{} new", row.events, row.new, row.events),
        ),
        RowState::Eose { elapsed } => (
            ">",
            Some(Color::Green),
            format!(
                "EOSE  {} events  {} new in {}ms",
                row.events,
                row.new,
                elapsed.as_millis()
            ),
        ),
        RowState::Closed { msg } => (
            "x",
            Some(Color::Red),
            format!("CLOSED: {}", truncate(msg, 56)),
        ),
        RowState::Auth => (
            "x",
            Some(Color::Red),
            "AUTH required (read-only — not authing)".to_string(),
        ),
        RowState::Error { msg } => ("x", Some(Color::Red), truncate(msg, 56)),
        RowState::Timeout => ("x", Some(Color::Yellow), "[wall timeout]".to_string()),
    };
    // The summary already carries the author count, e.g. `kind:1 (83
    // authors)`. Show sub_id so each REQ row is individually identifiable.
    let head = format!("{} {}", row.summary, row.sub_id);
    let mut line = format!(
        "{glyph} REQ {url:<44} {head:<30} {tail}",
        url = url,
        head = truncate(&head, 30),
    );
    if let Some(n) = &row.notice {
        line.push_str(&format!("  • NOTICE: {}", truncate(n, 40)));
    }
    let _ = row.authors; // count is rendered via `summary`.
    (color, line)
}

/// Print the "outbox: N relays, M authors-on-wire, K unroutable" line
/// directly to stdout, with crossterm color for the unroutable count when
/// non-zero.
pub fn print_outbox_line(relays: usize, authors_on_wire: usize, unroutable: usize) {
    let mut stdout = stdout();
    let _ = stdout.execute(Print(format!(
        "  outbox: {} relays, {} authors-on-wire, ",
        relays, authors_on_wire
    )));
    if unroutable > 0 {
        let _ = stdout.execute(SetForegroundColor(Color::Yellow));
    }
    let _ = stdout.execute(Print(format!("{} unroutable", unroutable)));
    let _ = stdout.execute(ResetColor);
    let _ = stdout.execute(Print("\n"));
}
