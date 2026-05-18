//! Outbox end-to-end performance probe.
//!
//! Exercises the *production* planner end-to-end against live relays:
//!   1. `SubscriptionCompiler::with_relays(...)` for the per-author NIP-65 fan
//!   2. `planner::apply_selection(...)` for greedy max-coverage reduction
//!   3. `CompiledPlan::unroutable_authors` surfaces the kernel's "no relay
//!      to ask" diagnostic
//!
//! Personal / per-user relays (e.g. `wss://filter.nostr.wine/npub1...`,
//! `wss://r.x/?broadcast=true`) are NOT filtered structurally. They have
//! coverage=1 by construction — only the embedded npub uses them — so the
//! greedy max-coverage selector in `apply_selection` loses every tiebreak
//! against real shared relays. The selector is the defense; a separate
//! URL-pattern filter would be redundant.
//!
//! Flow:
//!   - Connect to wss://purplepag.es as the indexer.
//!   - Phase A: REQ kind:3 for the seed → parse `p` tags → follow set.
//!   - Phase B: REQ kind:10002 for follows → MailboxSnapshot per author.
//!   - Phase C: compile + apply_selection.
//!   - Phase D: parallel fan-out to the optimized relay set.
//!
//! Run:
//!   cargo run -p nmp-core --example outbox_perf --release

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::io::ErrorKind;
use std::net::TcpStream;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use nmp_core::nip19::decode_npub;
use nmp_core::planner::{
    apply_selection, InMemoryMailboxCache, InterestId, InterestLifecycle, InterestScope,
    InterestShape, LogicalInterest, MailboxSnapshot, SubscriptionCompiler,
};
use serde_json::{json, Value};
use tungstenite::{stream::MaybeTlsStream, Message, WebSocket};

const INDEXER: &str = "wss://purplepag.es";
const SEED_NPUB: &str =
    "npub1l2vyh47mk2p0qlsku7hg0vn29faehy9hy34ygaclpn66ukqp3afqutajft";

const KIND3_WAIT: Duration = Duration::from_secs(10);
const KIND10002_WAIT: Duration = Duration::from_secs(15);
const FANOUT_WALL: Duration = Duration::from_secs(20);
const READ_POLL: Duration = Duration::from_millis(250);
const FANOUT_MAX_WORKERS: usize = 64;

// applesauce-style selector budgets (see planner::apply_selection).
const MAX_CONNECTIONS: usize = 30;
const MAX_RELAYS_PER_USER: usize = 2;

type Sock = WebSocket<MaybeTlsStream<TcpStream>>;

fn main() {
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("install rustls crypto provider");

    let seed_hex = decode_npub(SEED_NPUB).expect("decode npub");
    println!("== outbox perf probe ==");
    println!("  indexer:   {INDEXER}");
    println!("  seed npub: {SEED_NPUB}");
    println!("  seed hex:  {seed_hex}");
    println!("  budget:    max_connections={MAX_CONNECTIONS}, max_relays_per_user={MAX_RELAYS_PER_USER}");
    println!();

    let total_start = Instant::now();

    // ── Phase A ──────────────────────────────────────────────────────────────
    let phase_a_start = Instant::now();
    let (mut indexer, follows) = phase_a_fetch_kind3(&seed_hex);
    let phase_a_elapsed = phase_a_start.elapsed();
    println!(
        "phase A — kind:3 follows: got {} follows in {:?}",
        follows.len(),
        phase_a_elapsed
    );
    if follows.is_empty() {
        eprintln!("no follows — aborting");
        return;
    }
    println!();

    // ── Phase B ──────────────────────────────────────────────────────────────
    let phase_b_start = Instant::now();
    let mailboxes = phase_b_fetch_mailboxes(&mut indexer, &follows);
    let phase_b_elapsed = phase_b_start.elapsed();
    let cached = mailboxes.len();
    println!(
        "phase B — kind:10002: {cached}/{} follows have a cached relay list in {:?}",
        follows.len(),
        phase_b_elapsed
    );
    let _ = indexer.close(None);

    let mut cache = InMemoryMailboxCache::new();
    for (pk, snap) in &mailboxes {
        cache.put(pk.clone(), snap.clone());
    }
    println!();

    // ── Phase C: compile (with no fallbacks) + apply_selection ──────────────
    let phase_c_start = Instant::now();

    let interest = LogicalInterest {
        id: InterestId(1),
        scope: InterestScope::Global,
        shape: InterestShape {
            authors: follows.iter().cloned().collect(),
            kinds: [1u32, 6u32].into_iter().collect(),
            ..Default::default()
        },
        hints: Vec::new(),
        lifecycle: InterestLifecycle::OneShot,
    };
    // No indexer / account-read / app-relay fallbacks — strict NIP-65 only.
    // Authors with no kind:10002 will land in `unroutable_authors`.
    let empty: Vec<String> = Vec::new();
    let compiler = SubscriptionCompiler::with_relays(&cache, &empty, &empty, &empty);
    let mut plan = compiler.compile(&[interest]).expect("compile plan");
    let naive_relays = plan.per_relay.len();
    let naive_deliveries: usize = plan
        .per_relay
        .values()
        .map(|rp| {
            rp.sub_shapes
                .iter()
                .map(|s| s.shape.authors.len())
                .sum::<usize>()
        })
        .sum();
    let unroutable = plan.unroutable_authors.len();

    apply_selection(&mut plan, MAX_CONNECTIONS, MAX_RELAYS_PER_USER);

    let optimized_relays = plan.per_relay.len();
    let optimized_deliveries: usize = plan
        .per_relay
        .values()
        .map(|rp| {
            rp.sub_shapes
                .iter()
                .map(|s| s.shape.authors.len())
                .sum::<usize>()
        })
        .sum();
    let phase_c_elapsed = phase_c_start.elapsed();

    println!(
        "phase C — plan: naive {} relays → optimized {} relays in {:?}",
        naive_relays, optimized_relays, phase_c_elapsed
    );
    let authors_with_relay = follows.len() - unroutable;
    println!(
        "  follows: {}, routable: {} via NIP-65, unroutable: {} (no relay to ask)",
        follows.len(),
        authors_with_relay,
        unroutable
    );
    println!(
        "  naive    : {} authors-on-wire ({:.2}× per routable author)",
        naive_deliveries,
        if authors_with_relay == 0 {
            0.0
        } else {
            naive_deliveries as f64 / authors_with_relay as f64
        },
    );
    println!(
        "  optimized: {} authors-on-wire ({:.2}× per routable author)",
        optimized_deliveries,
        if authors_with_relay == 0 {
            0.0
        } else {
            optimized_deliveries as f64 / authors_with_relay as f64
        },
    );
    println!(
        "  reduction: {}× fewer sockets, {}× fewer REQs",
        ratio(naive_relays, optimized_relays),
        ratio(naive_deliveries, optimized_deliveries),
    );

    // Build the relay → authors map from the (post-selection) plan.
    let mut per_relay_authors: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (relay_url, rp) in &plan.per_relay {
        let mut authors: BTreeSet<String> = BTreeSet::new();
        for sub in &rp.sub_shapes {
            for author in &sub.shape.authors {
                authors.insert(author.clone());
            }
        }
        per_relay_authors.insert(relay_url.clone(), authors.into_iter().collect());
    }

    let mut rows: Vec<_> = per_relay_authors.iter().collect();
    rows.sort_by(|a, b| b.1.len().cmp(&a.1.len()));
    println!("  optimized relay set:");
    for (i, (relay, authors)) in rows.iter().enumerate() {
        println!(
            "    {:>2}. {:<48} {:>4} authors",
            i + 1,
            truncate(relay, 48),
            authors.len()
        );
    }
    println!();

    // ── Phase D ──────────────────────────────────────────────────────────────
    let phase_d_start = Instant::now();
    let (event_total, unique_ids, per_relay) = phase_d_fanout(&per_relay_authors);
    let phase_d_elapsed = phase_d_start.elapsed();
    let dedup_ratio = if event_total == 0 {
        0.0
    } else {
        unique_ids as f64 / event_total as f64
    };

    println!();
    println!("phase D — fanout: {event_total} deliveries / {unique_ids} unique events");
    println!(
        "                  dedup ratio {:.2} (1.0 = no duplicates, lower = more overlap)",
        dedup_ratio
    );
    println!("                  wall {:?}", phase_d_elapsed);

    let mut sorted: Vec<_> = per_relay.into_iter().collect();
    sorted.sort_by(|a, b| b.1.events.cmp(&a.1.events));
    let connected_count = sorted.iter().filter(|(_, s)| s.connected).count();
    let eose_count = sorted.iter().filter(|(_, s)| s.eose).count();
    let with_events = sorted.iter().filter(|(_, s)| s.events > 0).count();
    println!(
        "                  {} of {} relays connected, {} returned events, {} hit EOSE",
        connected_count,
        sorted.len(),
        with_events,
        eose_count,
    );
    println!();
    println!("per-relay (all, sorted by events):");
    println!(
        "  {:<48} {:>7} {:>9} {:>14} {:>6}",
        "relay", "events", "authors", "time-to-1st", "state"
    );
    for (relay, stats) in &sorted {
        let ttf = stats
            .time_to_first
            .map(|d| format!("{:>10.0?}", d))
            .unwrap_or_else(|| "       —".to_string());
        let state = match (stats.connected, stats.eose) {
            (false, _) => "no-net",
            (true, true) => "eose",
            (true, false) => "open",
        };
        println!(
            "  {:<48} {:>7} {:>9} {:>14} {:>6}",
            truncate(relay, 48),
            stats.events,
            stats.authors_in_req,
            ttf,
            state,
        );
    }

    println!();
    println!("== totals ==");
    println!("  total wall:   {:?}", total_start.elapsed());
    println!("  phase A:      {:?}", phase_a_elapsed);
    println!("  phase B:      {:?}", phase_b_elapsed);
    println!("  phase C:      {:?}", phase_c_elapsed);
    println!("  phase D:      {:?}", phase_d_elapsed);
}

fn ratio(numer: usize, denom: usize) -> String {
    if denom == 0 {
        "∞".to_string()
    } else {
        format!("{:.1}", numer as f64 / denom as f64)
    }
}

// ── Phase A ──────────────────────────────────────────────────────────────────

fn phase_a_fetch_kind3(seed_hex: &str) -> (Sock, BTreeSet<String>) {
    let mut socket = connect(INDEXER);
    let sub_id = "follows-1";
    let req = json!([
        "REQ",
        sub_id,
        { "kinds": [3], "authors": [seed_hex], "limit": 1 }
    ])
    .to_string();
    socket.send(Message::Text(req)).expect("send REQ");

    let deadline = Instant::now() + KIND3_WAIT;
    let mut follows: BTreeSet<String> = BTreeSet::new();
    while Instant::now() < deadline {
        match next_text(&mut socket) {
            Some(text) => {
                let v: Value = match serde_json::from_str(&text) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                if matches!(v[0].as_str(), Some("EVENT")) && v[1].as_str() == Some(sub_id) {
                    if let Some(event) = v.get(2) {
                        for tag in event["tags"].as_array().into_iter().flatten() {
                            if let Some(arr) = tag.as_array() {
                                if arr.first().and_then(Value::as_str) == Some("p") {
                                    if let Some(pk) = arr.get(1).and_then(Value::as_str) {
                                        if pk.len() == 64
                                            && pk.chars().all(|c| c.is_ascii_hexdigit())
                                        {
                                            follows.insert(pk.to_string());
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                if matches!(v[0].as_str(), Some("EOSE")) && v[1].as_str() == Some(sub_id) {
                    break;
                }
            }
            None => continue,
        }
    }
    let _ = socket.send(Message::Text(json!(["CLOSE", sub_id]).to_string()));
    (socket, follows)
}

// ── Phase B ──────────────────────────────────────────────────────────────────

fn phase_b_fetch_mailboxes(
    socket: &mut Sock,
    follows: &BTreeSet<String>,
) -> BTreeMap<String, MailboxSnapshot> {
    let sub_id = "mailboxes-1";
    let authors: Vec<String> = follows.iter().cloned().collect();
    let req = json!([
        "REQ",
        sub_id,
        { "kinds": [10002], "authors": authors }
    ])
    .to_string();
    socket.send(Message::Text(req)).expect("send REQ");

    let deadline = Instant::now() + KIND10002_WAIT;
    let mut out: BTreeMap<String, MailboxSnapshot> = BTreeMap::new();
    while Instant::now() < deadline {
        match next_text(socket) {
            None => continue,
            Some(text) => {
                let v: Value = match serde_json::from_str(&text) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                if matches!(v[0].as_str(), Some("EVENT")) && v[1].as_str() == Some(sub_id) {
                    if let Some(event) = v.get(2) {
                        if let Some((pk, snap)) = parse_kind10002(event) {
                            // newest-wins approximation
                            out.insert(pk, snap);
                        }
                    }
                }
                if matches!(v[0].as_str(), Some("EOSE")) && v[1].as_str() == Some(sub_id) {
                    break;
                }
            }
        }
    }
    let _ = socket.send(Message::Text(json!(["CLOSE", sub_id]).to_string()));
    out
}

/// Parse a kind:10002 event into a `MailboxSnapshot`.
///
/// No personal-relay URL filtering: the greedy max-coverage selector in
/// `apply_selection` is the defense. Personal relays have coverage=1 by
/// construction and lose every tiebreak against real shared relays.
fn parse_kind10002(event: &Value) -> Option<(String, MailboxSnapshot)> {
    if event["kind"].as_u64()? != 10002 {
        return None;
    }
    let pk = event["pubkey"].as_str()?.to_string();
    let mut snap = MailboxSnapshot::default();
    for tag in event["tags"].as_array().into_iter().flatten() {
        let arr = match tag.as_array() {
            Some(a) => a,
            None => continue,
        };
        if arr.first().and_then(Value::as_str) != Some("r") {
            continue;
        }
        let url = match arr.get(1).and_then(Value::as_str) {
            Some(u) => normalize_url(u),
            None => continue,
        };
        if url.is_empty() {
            continue;
        }
        let marker = arr.get(2).and_then(Value::as_str);
        match marker {
            Some("read") => snap.read_relays.push(url),
            Some("write") => snap.write_relays.push(url),
            None | Some(_) => snap.both_relays.push(url),
        }
    }
    Some((pk, snap))
}

fn normalize_url(s: &str) -> String {
    let trimmed = s.trim();
    if !(trimmed.starts_with("wss://") || trimmed.starts_with("ws://")) {
        return String::new();
    }
    let mut s = trimmed.to_string();
    while s.ends_with('/') && s.matches('/').count() > 2 {
        s.pop();
    }
    if s.ends_with('/') {
        s.pop();
    }
    s
}

// ── Phase D ──────────────────────────────────────────────────────────────────

#[derive(Default, Clone)]
struct RelayStats {
    events: u64,
    authors_in_req: usize,
    time_to_first: Option<Duration>,
    connected: bool,
    eose: bool,
}

enum Msg {
    Frame { relay: String, value: Value },
    Done { relay: String, stats: RelayStats },
}

fn phase_d_fanout(
    per_relay: &BTreeMap<String, Vec<String>>,
) -> (u64, u64, BTreeMap<String, RelayStats>) {
    let (msg_tx, msg_rx) = mpsc::channel::<Msg>();
    let (work_tx, work_rx) = mpsc::channel::<(String, Vec<String>)>();
    let work_rx = Arc::new(Mutex::new(work_rx));
    let global_deadline = Instant::now() + FANOUT_WALL;

    let mut total_jobs = 0usize;
    for (relay_url, authors) in per_relay {
        if !relay_url.starts_with("wss://") && !relay_url.starts_with("ws://") {
            continue;
        }
        work_tx
            .send((relay_url.clone(), authors.clone()))
            .expect("queue job");
        total_jobs += 1;
    }
    drop(work_tx);

    let workers = FANOUT_MAX_WORKERS.min(total_jobs.max(1));
    println!(
        "phase D — fanout: {} jobs across {} parallel workers (wall {:?})",
        total_jobs, workers, FANOUT_WALL
    );
    for _ in 0..workers {
        let work_rx = work_rx.clone();
        let msg_tx = msg_tx.clone();
        thread::spawn(move || loop {
            if Instant::now() >= global_deadline {
                return;
            }
            let job = {
                let lock = work_rx.lock().unwrap();
                lock.try_recv()
            };
            match job {
                Ok((url, authors)) => {
                    run_relay_thread(url, authors, msg_tx.clone(), global_deadline);
                }
                Err(mpsc::TryRecvError::Empty) => {
                    thread::sleep(Duration::from_millis(50));
                }
                Err(mpsc::TryRecvError::Disconnected) => return,
            }
        });
    }
    drop(msg_tx);

    let mut unique: HashSet<String> = HashSet::new();
    let mut totals = 0u64;
    let mut per_relay_stats: BTreeMap<String, RelayStats> = BTreeMap::new();

    loop {
        let now = Instant::now();
        if now >= global_deadline {
            break;
        }
        let remaining = global_deadline.saturating_duration_since(now);
        let timeout = remaining.min(Duration::from_millis(500));
        let msg = match msg_rx.recv_timeout(timeout) {
            Ok(m) => m,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        match msg {
            Msg::Frame { relay, value } => {
                let stats = per_relay_stats.entry(relay).or_default();
                if let Some(id) = value
                    .get(2)
                    .and_then(|v| v.get("id"))
                    .and_then(Value::as_str)
                {
                    stats.events += 1;
                    totals += 1;
                    unique.insert(id.to_string());
                }
            }
            Msg::Done { relay, stats } => {
                let entry = per_relay_stats.entry(relay).or_default();
                entry.authors_in_req = stats.authors_in_req;
                entry.connected |= stats.connected;
                entry.eose |= stats.eose;
                if entry.time_to_first.is_none() {
                    entry.time_to_first = stats.time_to_first;
                }
            }
        }
    }
    (totals, unique.len() as u64, per_relay_stats)
}

fn run_relay_thread(
    relay_url: String,
    authors: Vec<String>,
    tx: mpsc::Sender<Msg>,
    deadline: Instant,
) {
    let authors_in_req = authors.len();
    let mut stats = RelayStats {
        events: 0,
        authors_in_req,
        time_to_first: None,
        connected: false,
        eose: false,
    };
    let started = Instant::now();

    let mut socket = match try_connect(&relay_url) {
        Some(s) => s,
        None => {
            let _ = tx.send(Msg::Done {
                relay: relay_url,
                stats,
            });
            return;
        }
    };
    stats.connected = true;

    let sub_id = "feed-1";
    let filter = json!({
        "kinds": [1, 6],
        "authors": authors,
        "limit": 200,
    });
    let req = json!(["REQ", sub_id, filter]).to_string();
    if socket.send(Message::Text(req)).is_err() {
        let _ = tx.send(Msg::Done {
            relay: relay_url,
            stats,
        });
        return;
    }

    while Instant::now() < deadline {
        match next_text(&mut socket) {
            None => continue,
            Some(text) => {
                let v: Value = match serde_json::from_str(&text) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                match v[0].as_str() {
                    Some("EVENT") if v[1].as_str() == Some(sub_id) => {
                        if stats.time_to_first.is_none() {
                            stats.time_to_first = Some(started.elapsed());
                        }
                        let _ = tx.send(Msg::Frame {
                            relay: relay_url.clone(),
                            value: v,
                        });
                    }
                    Some("EOSE") if v[1].as_str() == Some(sub_id) => {
                        stats.eose = true;
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    let _ = socket.send(Message::Text(json!(["CLOSE", sub_id]).to_string()));
    let _ = socket.close(None);
    let _ = tx.send(Msg::Done {
        relay: relay_url,
        stats,
    });
}

// ── transport helpers ───────────────────────────────────────────────────────

fn connect(url: &str) -> Sock {
    try_connect(url).unwrap_or_else(|| {
        eprintln!("connect failed: {url}");
        std::process::exit(1);
    })
}

fn try_connect(url: &str) -> Option<Sock> {
    let (socket, _response) = match tungstenite::connect(url) {
        Ok(p) => p,
        Err(_) => return None,
    };
    let _ = match socket.get_ref() {
        MaybeTlsStream::Plain(s) => s.set_read_timeout(Some(READ_POLL)),
        MaybeTlsStream::Rustls(s) => s.get_ref().set_read_timeout(Some(READ_POLL)),
        _ => Ok(()),
    };
    Some(socket)
}

fn next_text(socket: &mut Sock) -> Option<String> {
    match socket.read() {
        Ok(Message::Text(s)) => Some(s),
        Ok(Message::Close(_)) => Some(String::new()),
        Ok(_) => None,
        Err(tungstenite::Error::Io(e))
            if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut =>
        {
            None
        }
        Err(_) => Some(String::new()),
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        s.to_string()
    } else {
        format!("{}…", &s[..n.saturating_sub(1)])
    }
}
