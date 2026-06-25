//! Performance measurement binary for all `StoreQuery` shapes against
//! `LmdbEventStore`. Captures baseline timings before PR #1516 (true
//! streaming `query_visit`).
//!
//! Run: cargo run -p nmp-testing --features lmdb-backend --bin cache-baseline

fn main() {
    #[cfg(not(feature = "lmdb-backend"))]
    {
        eprintln!("cache-baseline requires --features lmdb-backend");
        return;
    }
    #[cfg(feature = "lmdb-backend")]
    run();
}

#[cfg(feature = "lmdb-backend")]
fn run() {
    use std::collections::BTreeSet;
    use std::time::Instant;

    use nmp_store::{EventStore, StoreQuery, StoredEvent};
    use nmp_testing::store_harness::{hex_to_id, StoreHarness, ALICE_HEX, ALICE_PUBKEY, BOB_HEX};

    const WARMUP: usize = 1;
    const ITERS: usize = 30;

    fn author_hex(i: usize) -> String {
        format!("{:02x}{}", (i % 256) as u8, "0".repeat(62))
    }

    fn tags_query(letter: char, value: &str, kinds: Vec<u32>) -> StoreQuery {
        let mut tags = std::collections::BTreeMap::new();
        tags.insert(
            nostr::SingleLetterTag::from_char(letter).unwrap(),
            BTreeSet::from([value.to_string()]),
        );
        StoreQuery::Tags {
            authors: BTreeSet::new(),
            kinds,
            tags,
            since: None,
            until: None,
        }
    }

    fn measure(store: &dyn EventStore, q: &StoreQuery, limit: usize) -> (usize, u128, u128) {
        let mut returned = 0usize;
        // warmup
        for _ in 0..WARMUP {
            returned = 0;
            store
                .query_visit(q, limit, &mut |_: &StoredEvent| {
                    returned += 1;
                    std::ops::ControlFlow::Continue(())
                })
                .unwrap();
        }
        // timed runs
        let mut times: Vec<u128> = Vec::with_capacity(ITERS);
        for _ in 0..ITERS {
            returned = 0;
            let t0 = Instant::now();
            store
                .query_visit(q, limit, &mut |_: &StoredEvent| {
                    returned += 1;
                    std::ops::ControlFlow::Continue(())
                })
                .unwrap();
            times.push(t0.elapsed().as_micros());
        }
        let mean = times.iter().sum::<u128>() / times.len() as u128;
        let min = *times.iter().min().unwrap();
        (returned, mean, min)
    }

    println!(
        "{:<25} {:>10} {:>10} {:>7} {:>9} {:>9}",
        "scenario", "store_size", "returned", "limit", "mean_us", "min_us"
    );
    println!("{}", "-".repeat(75));

    // ─── 1. feed_kindtime ────────────────────────────────────────────────────
    {
        let h = StoreHarness::lmdb();
        let kinds = [1u32, 6, 16];
        let n = 5000usize;
        for i in 0..n as u64 {
            let kind = kinds[(i as usize) % 3];
            h.insert(ALICE_HEX, kind, 1000 + i, "bench");
        }
        let q = StoreQuery::KindTime {
            kinds: vec![1, 6, 16],
            since: None,
            until: None,
        };
        let (ret, mean, min) = measure(&*h.store, &q, 500);
        println!(
            "{:<25} {:>10} {:>10} {:>7} {:>9} {:>9}",
            "feed_kindtime", n, ret, 500, mean, min
        );
    }

    // ─── 2. home_timeline ───────────────────────────────────────────────────
    {
        let h = StoreHarness::lmdb();
        let n_authors = 100usize;
        let events_per = 50usize;
        let mut authors: BTreeSet<[u8; 32]> = BTreeSet::new();
        for a in 1..=n_authors {
            let ahex = author_hex(a);
            authors.insert(hex_to_id(&ahex));
            for e in 0..events_per as u64 {
                h.insert(&ahex, 1, a as u64 * 100 + e, "bench");
            }
        }
        let n = n_authors * events_per;
        let q = StoreQuery::AuthorsKind {
            authors,
            kinds: vec![1],
            since: None,
            until: None,
        };
        let (ret, mean, min) = measure(&*h.store, &q, 500);
        println!(
            "{:<25} {:>10} {:>10} {:>7} {:>9} {:>9}",
            "home_timeline", n, ret, 500, mean, min
        );
    }

    // ─── 3. thread_etag ─────────────────────────────────────────────────────
    {
        let h = StoreHarness::lmdb();
        let root_hex = "a".repeat(64);
        let root_ev = h.make_event_with_id(&root_hex, ALICE_HEX, 1, 500);
        h.insert_raw(root_ev, "bench", 500_000);
        let n = 2000usize;
        for i in 0..n as u64 {
            let reply = h.make_event_with_tags(
                BOB_HEX,
                1,
                600 + i,
                vec![vec!["e".into(), root_hex.clone()]],
            );
            h.insert_raw(reply, "bench", (600 + i) * 1000);
        }
        let q = tags_query('e', &root_hex, vec![1]);
        let (ret, mean, min) = measure(&*h.store, &q, 500);
        println!(
            "{:<25} {:>10} {:>10} {:>7} {:>9} {:>9}",
            "thread_etag", n + 1, ret, 500, mean, min
        );
    }

    // ─── 4. mentions_ptag ───────────────────────────────────────────────────
    {
        let h = StoreHarness::lmdb();
        let n = 2000usize;
        for i in 0..n as u64 {
            let ev = h.make_event_with_tags(
                BOB_HEX,
                1,
                1000 + i,
                vec![vec!["p".into(), ALICE_HEX.into()]],
            );
            h.insert_raw(ev, "bench", (1000 + i) * 1000);
        }
        let q = tags_query('p', ALICE_HEX, vec![1]);
        let (ret, mean, min) = measure(&*h.store, &q, 500);
        println!(
            "{:<25} {:>10} {:>10} {:>7} {:>9} {:>9}",
            "mentions_ptag", n, ret, 500, mean, min
        );
    }

    // ─── 5. dm_authorkind ───────────────────────────────────────────────────
    {
        let h = StoreHarness::lmdb();
        let n4 = 1000usize;
        let n14 = 1000usize;
        for i in 0..n4 as u64 {
            h.insert(ALICE_HEX, 4, 1000 + i, "bench");
        }
        for i in 0..n14 as u64 {
            h.insert(ALICE_HEX, 14, 2000 + i, "bench");
        }
        let q = StoreQuery::AuthorKind {
            author: ALICE_PUBKEY,
            kinds: vec![4, 14],
            since: None,
            until: None,
        };
        let (ret, mean, min) = measure(&*h.store, &q, 500);
        println!(
            "{:<25} {:>10} {:>10} {:>7} {:>9} {:>9}",
            "dm_authorkind",
            n4 + n14,
            ret,
            500,
            mean,
            min
        );
    }

    // ─── 6. profile_kind0 ───────────────────────────────────────────────────
    {
        let h = StoreHarness::lmdb();
        let n_authors = 50usize;
        for a in 1..=n_authors {
            let ahex = author_hex(a);
            // kind:0 is replaceable per author; each author has 1 surviving event
            h.insert(&ahex, 0, 1000 + a as u64, "bench");
        }
        let q = StoreQuery::AuthorKind {
            author: ALICE_PUBKEY,
            kinds: vec![0],
            since: None,
            until: None,
        };
        let (ret, mean, min) = measure(&*h.store, &q, 50);
        println!(
            "{:<25} {:>10} {:>10} {:>7} {:>9} {:>9}",
            "profile_kind0", n_authors, ret, 50, mean, min
        );
    }

    // ─── 7. relay_ptag ──────────────────────────────────────────────────────
    {
        let h = StoreHarness::lmdb();
        for i in 1u8..=50 {
            let ahex = format!("{:02x}{}", i, "0".repeat(62));
            let kind = if i <= 25 { 3u32 } else { 10002u32 };
            let ev = h.make_event_with_tags(
                &ahex,
                kind,
                1000 + i as u64,
                vec![vec!["p".into(), ALICE_HEX.into()]],
            );
            h.insert_raw(ev, "bench", (1000 + i as u64) * 1000);
        }
        let q = tags_query('p', ALICE_HEX, vec![3, 10002]);
        let (ret, mean, min) = measure(&*h.store, &q, 200);
        println!(
            "{:<25} {:>10} {:>10} {:>7} {:>9} {:>9}",
            "relay_ptag", 50, ret, 200, mean, min
        );
    }

    println!("\nDone. {} iteration(s) per scenario (+ {} warmup).", ITERS, WARMUP);
}
