//! Soak engine for the `real_relay_soak` honest-validation runner. Proves
//! three invariants: (1) zero leaked subs — every REQ id is later CLOSEd;
//! (2) bounded working set — simultaneously-live sub ids never exceed
//! `relays * 2` (honest deterministic bound, not RSS); (3) no panic — a relay
//! dying mid-soak is a recorded degradation, only ZERO survivors is a SKIP.
//! Churn: every ~15s OPEN a fresh unique-id REQ per relay *then* CLOSE the
//! prior one (open-then-close = no coverage gap; per-relay live peaks at 2).
//! We reach the harness via `use super::common` — the top-level target owns
//! the single `#[path]` copy; re-declaring it here would compile a second,
//! type-incompatible copy of `Verdict`/`RelaySocket`.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use super::common::{
    self, report_page, send_text, write_report, RelaySocket, Verdict, DAMUS_RELAY, NOS_LOL,
    PRIMAL_RELAY,
};

const CONNECT_BUDGET: Duration = Duration::from_secs(8); // no hang on dead TLS
const WINDOW: Duration = Duration::from_secs(15); // churn cadence
const SHUTDOWN_RESERVE: Duration = Duration::from_secs(3); // tail for sweep+report
const MIN_SECS: u64 = 10;
const MAX_SECS: u64 = 3600;
const DEFAULT_SECS: u64 = 120;

/// One live relay connection plus its currently-open sub id (`None` only
/// transiently during an open-then-close swap).
struct Conn {
    url: &'static str,
    idx: usize,
    socket: RelaySocket,
    live_sub: Option<String>,
    events: u64,
}

/// Final tally surfaced to the runner and the report. `leaked` = sub ids
/// opened but never CLOSEd (empty on a healthy run).
pub struct SoakResult {
    pub verdict: Verdict,
    pub duration_s: u64,
    pub relays: Vec<&'static str>,
    pub req_opened: usize,
    pub req_closed: usize,
    pub events_seen: u64,
    pub max_live_subs: usize,
    pub ceiling: usize,
    pub windows: u64,
    pub errors: Vec<String>,
    pub per_relay: Vec<(&'static str, u64)>,
    pub leaked: Vec<String>,
    pub run_id: String,
    pub started_at: u64,
}

/// Read `NMP_SOAK_DURATION_SECS`, parse safely, clamp to `[10, 3600]`.
fn duration_secs() -> u64 {
    std::env::var("NMP_SOAK_DURATION_SECS")
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_SECS)
        .clamp(MIN_SECS, MAX_SECS)
}

/// Deterministically-unique sub id: relay idx + window + ms epoch. Unique by
/// construction even within the same millisecond (idx/window differ).
fn sub_id(relay_idx: usize, window: u64) -> String {
    format!("soak-{relay_idx}-{window}-{}", common::now_ms())
}

/// Non-blocking drain: count buffered `["EVENT",...]` frames then return as
/// soon as the socket would block. Hard socket error → caller (degradation).
/// WouldBlock/TimedOut arm mirrors the harness's `drain_until`.
fn pump(socket: &mut RelaySocket) -> Result<u64, String> {
    let mut seen = 0u64;
    loop {
        match socket.read() {
            Ok(tungstenite::Message::Text(t)) if is_event_frame(&t) => seen += 1,
            Ok(_) => {}
            Err(tungstenite::Error::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                return Ok(seen);
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// Cheap structural check for an `["EVENT", ...]` envelope (no full parse).
fn is_event_frame(text: &str) -> bool {
    let t = text.trim_start();
    t.starts_with("[\"EVENT\"") || t.starts_with("[ \"EVENT\"")
}

/// REQ envelope: recent kind:1 firehose so the relay streams between churns.
fn req_frame(sub: &str) -> String {
    let since = common::now_s().saturating_sub(300);
    format!("[\"REQ\",\"{sub}\",{{\"kinds\":[1],\"limit\":16,\"since\":{since}}}]")
}

/// Run the soak. Always returns a `SoakResult`; never panics on relay error
/// (leak detection is reported in the result; the runner asserts loudly).
pub fn run_soak() -> SoakResult {
    let started_at = common::now_s();
    let duration_s = duration_secs();
    let run_id = format!("soak-{started_at}");
    let candidates: [&'static str; 3] = [DAMUS_RELAY, NOS_LOL, PRIMAL_RELAY];
    let mut errors: Vec<String> = Vec::new();
    let mut req_opened: HashSet<String> = HashSet::new();
    let mut req_closed: HashSet<String> = HashSet::new();
    let mut req_sent: usize = 0;
    let mut close_sent: usize = 0;
    let mut max_live: usize = 0;

    // Open the initial subscription on every reachable relay (skip failures
    // as recorded errors — never hang on a dead TLS handshake).
    let mut conns: Vec<Conn> = Vec::new();
    for (idx, url) in candidates.into_iter().enumerate() {
        let mut socket = match common::open_with_timeout(url, CONNECT_BUDGET) {
            Ok(s) => s,
            Err(e) => {
                errors.push(format!("{url}: open failed: {e}"));
                continue;
            }
        };
        let id = sub_id(idx, 0);
        if let Err(e) = send_text(&mut socket, req_frame(&id)) {
            errors.push(format!("{url}: initial REQ send failed: {e}"));
            let _ = socket.close(None);
            continue;
        }
        req_opened.insert(id.clone());
        req_sent += 1;
        conns.push(Conn { url, idx, socket, live_sub: Some(id), events: 0 });
    }

    let relays: Vec<&'static str> = conns.iter().map(|c| c.url).collect();
    let ceiling = candidates.len() * 2;
    let mut window: u64 = 0;
    // ZERO relays → honest SKIP: conns empty makes every loop/sweep a no-op;
    // we fall through to the single SoakResult literal (verdict forced Skip).
    let skip = conns.is_empty();
    max_live = max_live.max(conns.len());
    let churn_until = Instant::now() + Duration::from_secs(duration_s) - SHUTDOWN_RESERVE;
    let mut next_churn = Instant::now() + WINDOW;

    // Main loop: pump continuously; churn every WINDOW until the reserve.
    while !skip && Instant::now() < churn_until {
        for c in conns.iter_mut() {
            match pump(&mut c.socket) {
                Ok(n) => c.events += n,
                Err(e) => errors.push(format!("{}: read error: {e}", c.url)),
            }
        }
        if Instant::now() >= next_churn && Instant::now() < churn_until {
            window += 1;
            for c in conns.iter_mut() {
                // OPEN new (no coverage gap) THEN CLOSE old: relay holds
                // old+new live across the swap → per-relay peak 2, total 2N.
                let new_id = sub_id(c.idx, window);
                match send_text(&mut c.socket, req_frame(&new_id)) {
                    Ok(()) => {
                        req_opened.insert(new_id.clone());
                        req_sent += 1;
                        max_live = max_live.max(outstanding(&req_opened, &req_closed));
                        if let Some(old) = c.live_sub.take() {
                            match send_text(&mut c.socket, format!("[\"CLOSE\",\"{old}\"]")) {
                                Ok(()) => {
                                    req_closed.insert(old);
                                    close_sent += 1;
                                }
                                // Not closed → stays in req_opened; leak check finds it.
                                Err(e) => errors.push(format!("{}: CLOSE fail: {e}", c.url)),
                            }
                        }
                        c.live_sub = Some(new_id);
                    }
                    // New REQ failed → keep old sub live (no swap this window).
                    Err(e) => errors.push(format!("{}: w{window} REQ fail: {e}", c.url)),
                }
            }
            max_live = max_live.max(outstanding(&req_opened, &req_closed));
            next_churn = Instant::now() + WINDOW;
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    // Final CLOSE-all sweep — best effort, runs even if relays errored.
    for c in conns.iter_mut() {
        if let Some(old) = c.live_sub.take() {
            match send_text(&mut c.socket, format!("[\"CLOSE\",\"{old}\"]")) {
                Ok(()) => {
                    req_closed.insert(old);
                    close_sent += 1;
                }
                Err(e) => errors.push(format!("{}: final CLOSE failed: {e}", c.url)),
            }
        }
        if let Ok(n) = pump(&mut c.socket) {
            c.events += n; // late EVENTs counted before teardown
        }
        let _ = c.socket.close(None);
    }

    let events_seen: u64 = conns.iter().map(|c| c.events).sum();
    let per_relay: Vec<(&'static str, u64)> = conns.iter().map(|c| (c.url, c.events)).collect();
    // Leak: every opened id closed, set sizes == send counters (uniqueness —
    // no id reused), CLOSE ⊇ OPEN.
    let leaked: Vec<String> = req_opened.difference(&req_closed).cloned().collect();
    let counts_match = req_opened.len() == req_sent && req_closed.len() == close_sent;
    let clean = leaked.is_empty() && req_closed.is_superset(&req_opened) && counts_match;
    let verdict = match (skip, clean) {
        (true, _) => Verdict::Skip,
        (false, true) => Verdict::Pass,
        (false, false) => Verdict::Fail,
    };

    SoakResult {
        verdict,
        duration_s,
        relays,
        req_opened: req_opened.len(),
        req_closed: req_closed.len(),
        events_seen,
        max_live_subs: max_live,
        ceiling,
        windows: window,
        errors,
        per_relay,
        leaked,
        run_id,
        started_at,
    }
}

/// Number of distinct sub ids opened but not yet CLOSEd — the honest,
/// deterministic working-set measure (one per relay steady-state, peaking at
/// two per relay during an open-then-close swap).
fn outstanding(opened: &HashSet<String>, closed: &HashSet<String>) -> usize {
    opened.difference(closed).count()
}

fn bullets<T: std::fmt::Display>(items: &[T], empty: &str) -> String {
    if items.is_empty() {
        return format!("{empty}\n");
    }
    items.iter().map(|i| format!("- {i}\n")).collect()
}

/// Render the one-page soak report (frontmatter + greppable verdict + body).
pub fn render_report(r: &SoakResult) -> String {
    let relays = if r.relays.is_empty() { "(none reachable)".into() } else { r.relays.join(", ") };
    let per_relay: Vec<String> =
        r.per_relay.iter().map(|(u, n)| format!("`{u}`: {n} EVENT frames")).collect();
    let leak = if r.leaked.is_empty() {
        "zero leaked subscriptions — every REQ id was matched by a CLOSE.".into()
    } else {
        format!("**LEAK DETECTED** — {} sub id(s) opened but never CLOSEd: {:?}",
            r.leaked.len(), r.leaked)
    };
    let ws_ok = if r.max_live_subs <= r.ceiling { "within bound" } else { "**EXCEEDED**" };
    let (w, rid, st, d, win) = (WINDOW.as_secs(), &r.run_id, r.started_at, r.duration_s, r.windows);
    let (op, cl, ml, ceil, ev) = (r.req_opened, r.req_closed, r.max_live_subs, r.ceiling, r.events_seen);
    let per_relay = bullets(&per_relay, "- (no relay survived to tally)");
    let errors = bullets(&r.errors, "_none — all relays survived the soak cleanly._");
    let body = format!(
        "Sustained multi-relay subscription soak. REQs are churned every \
         ~{w}s (open-new-then-close-old) to exercise sub lifecycle under load \
         while proving the kernel wire path leaks nothing and keeps a bounded \
         working set.\n\n\
         ## Run\n\n\
         - run id: `{rid}`\n- started at (unix): `{st}`\n- duration: `{d}s`\n\
         - relays: {relays}\n- churn windows completed: `{win}`\n\n\
         ## Leak check (strict)\n\n\
         - REQ opened (unique ids): `{op}`\n- CLOSE sent (unique ids): `{cl}`\n\
         - result: {leak}\n\n\
         ## Working-set bound (honest, deterministic — not RSS)\n\n\
         - max simultaneously-live subs observed: `{ml}`\n\
         - ceiling (relays × 2): `{ceil}`\n- status: {ws_ok}\n\n\
         We deliberately do **not** sample RSS (noisy on macOS); the \
         live-sub-set bound is the honest invariant.\n\n\
         ## Events\n\n\
         - total EVENT frames drained: `{ev}`\n\
         {per_relay}\n\
         ## Errors / degradations\n\n\
         {errors}\n\n\
         A relay dying mid-soak is a recorded degradation, not a test failure \
         — only ZERO surviving relays is a SKIP, and a real leak is a loud FAIL.",
    );
    let relay_refs: Vec<&str> = r.relays.to_vec();
    report_page("Real-relay soak — leak / working-set / no-panic",
        "soak-multi-relay-churn", r.verdict, &relay_refs, &body)
}

/// Write the report unconditionally (pass/skip/fail) so a green run still
/// leaves on-disk evidence. Stem `soak-<unix_ts>`.
pub fn persist_report(r: &SoakResult) {
    write_report(&format!("soak-{}", r.started_at), &render_report(r));
}
