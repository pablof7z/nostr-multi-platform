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

mod phase_a;
mod phase_b;
mod phase_d;
mod transport;

use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use nmp_core::nip19::decode_npub;
use nmp_core::planner::{
    apply_selection, InMemoryMailboxCache, InterestId, InterestLifecycle, InterestScope,
    InterestShape, LogicalInterest, SubscriptionCompiler,
};

use phase_a::phase_a_fetch_kind3;
use phase_b::phase_b_fetch_mailboxes;
use phase_d::phase_d_fanout;
use transport::truncate;

const INDEXER: &str = "wss://purplepag.es";
const SEED_NPUB: &str = "npub1l2vyh47mk2p0qlsku7hg0vn29faehy9hy34ygaclpn66ukqp3afqutajft";

// applesauce-style selector budgets (see planner::apply_selection).
const MAX_CONNECTIONS: usize = 30;
const MAX_RELAYS_PER_USER: usize = 2;

fn main() {
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("install rustls crypto provider");

    let seed_hex = decode_npub(SEED_NPUB).expect("decode npub");
    println!("== outbox perf probe ==");
    println!("  indexer:   {INDEXER}");
    println!("  seed npub: {SEED_NPUB}");
    println!("  seed hex:  {seed_hex}");
    println!(
        "  budget:    max_connections={MAX_CONNECTIONS}, max_relays_per_user={MAX_RELAYS_PER_USER}"
    );
    println!();

    let total_start = Instant::now();

    // ── Phase A ──────────────────────────────────────────────────────────────
    let phase_a_start = Instant::now();
    let (mut indexer, follows) = phase_a_fetch_kind3(INDEXER, &seed_hex);
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
        is_indexer_discovery: false,
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
        let mut authors: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
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
