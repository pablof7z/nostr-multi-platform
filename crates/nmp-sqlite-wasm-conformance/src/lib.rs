//! Dedicated-Worker OPFS-SQLite conformance vehicle for `nmp-sqlite-wasm`
//! (#1007 PR-6).
//!
//! ## Why this crate exists (the Worker-only constraint)
//!
//! The `nmp-sqlite-wasm` backend runs SQLite over the OPFS *SyncAccessHandle
//! pool* VFS. `createSyncAccessHandle()` — the synchronous file primitive the
//! pool VFS is built on — **only exists inside a dedicated Web Worker**; it is
//! absent on the page main thread. The repo's existing wasm test setup
//! (`crates/nmp-browser-runtime/src/wasm/*_tests.rs`)
//! executes on the **main thread**, so it structurally cannot exercise this
//! backend. This crate is the missing vehicle: a `wasm-bindgen --target web`
//! cdylib whose single exported entry point ([`run_conformance`]) is invoked
//! *from inside a dedicated Worker* (see `web/worker.js`), driven by a headless
//! browser (`web/run-conformance.mjs`). It is the sole end-to-end proof that
//! the novel sync-over-OPFS path actually executes.
//!
//! ## What it proves today (through PR-3's engine)
//!
//! As of #1007 PR-3 the harness drives the store's real typed surface end to
//! end, inside the dedicated Worker:
//!
//! 1. `open_store` — module init + opfs-sahpool VFS install + database open.
//! 2. `schema_ready` — `open()` auto-migrated the full events schema (fresh db
//!    has a queryable, empty `events` table; no hand-rolled DDL).
//! 3. `insert_event` — a structurally-valid kind:1 event through the typed
//!    [`OpfsSqliteStore::insert`], asserting an `Inserted` outcome.
//! 4. `read_back_event` — typed [`OpfsSqliteStore::get_by_id`] point read with
//!    field-level value assertions.
//! 5. `reopen_persisted` — close the store, reopen the same OPFS database, and
//!    confirm the event is still readable: real OPFS durability, the backend's
//!    whole point.
//!
//! PR-4 then drives the scan / streaming-query read paths over a small corpus
//! inserted into the reopened store:
//!
//! 6. `insert_scan_corpus` — nine events across three authors and two kinds.
//! 7. `scan_by_author_kind` — single-author newest-first ordering.
//! 8. `scan_by_authors_kind_global_order` — **global** `(created_at desc)` merge
//!    across authors (interleaved, not grouped by author).
//! 9. `scan_by_tags_index_served` — AND-across-`#e`/`#p`, OR-within-`#e`-values
//!    tag intersection, proving the `event_tags` index-served LMDB parity.
//! 10. `query_visit_budget` — the streaming visitor honours the per-call budget
//!     (budget 2 visits exactly the two newest rows).
//! 11. `relay_kind_privacy_gate` — relay-kind coverage/count hides private
//!     NIP-04/17/59 kinds even though SQLite derives the projection from
//!     provenance rows.
//!
//! ## How it grows as the engine lands
//!
//! Each assertion is an independent recorded [`Step`]. As PR-4/5 add scan and gc
//! methods, new steps slot into [`run`] without touching the Worker/host/CI
//! plumbing. The harness shape is fixed; only the body of [`run`] tracks the
//! engine surface.
//!
//! On native this crate cfg-compiles to nothing (the engine it drives is
//! wasm32-only), mirroring `nmp-sqlite-wasm`'s own target gating.

#![cfg(target_arch = "wasm32")]

use std::collections::{BTreeMap, BTreeSet};
use std::ops::ControlFlow;

use nmp_sqlite_wasm::{EngineEvent, EngineQuery, InsertOutcome, OpfsSqliteStore};
use wasm_bindgen::prelude::*;

mod relay_kind_privacy;

/// One recorded conformance assertion: its stable name, pass/fail, and a
/// human-readable detail (success summary or the failing reason).
struct Step {
    name: &'static str,
    ok: bool,
    detail: String,
}

/// Ordered log of every assertion attempted in a single harness run.
#[derive(Default)]
struct Report {
    steps: Vec<Step>,
}

impl Report {
    /// Record the outcome of one assertion. `Ok(detail)` passes; `Err(detail)`
    /// fails. Returns whether it passed so callers can short-circuit.
    fn record(&mut self, name: &'static str, result: Result<String, String>) -> bool {
        let (ok, detail) = match result {
            Ok(d) => (true, d),
            Err(e) => (false, e),
        };
        self.steps.push(Step { name, ok, detail });
        ok
    }

    /// A run passes only if it recorded at least one step and every step passed.
    fn passed(&self) -> bool {
        !self.steps.is_empty() && self.steps.iter().all(|s| s.ok)
    }

    /// Serialise to a compact JSON object the host page can render. Hand-rolled
    /// (no `serde`) because the shape is tiny and fixed.
    fn to_json(&self) -> String {
        let mut out = String::from("{\"passed\":");
        out.push_str(if self.passed() { "true" } else { "false" });
        out.push_str(",\"steps\":[");
        for (i, s) in self.steps.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str("{\"name\":");
            push_json_str(&mut out, s.name);
            out.push_str(",\"ok\":");
            out.push_str(if s.ok { "true" } else { "false" });
            out.push_str(",\"detail\":");
            push_json_str(&mut out, &s.detail);
            out.push('}');
        }
        out.push_str("]}");
        out
    }
}

/// Append `s` to `out` as a JSON string literal (minimal escaping).
fn push_json_str(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Drive the full conformance sequence, returning the recorded [`Report`].
async fn run() -> Report {
    let mut report = Report::default();

    // A fresh database name per run keeps repeated runs in the same browser
    // origin independent (OPFS persists across runs).
    let db_name = format!("nmp-conformance-{}", js_sys::Date::now() as u64);

    // ── Step 1: open ──────────────────────────────────────────────────────
    // Exercises the Worker-only path end to end: sqlite3 module init, OPFS
    // opfs-sahpool VFS install (the SyncAccessHandle pool acquisition that only
    // works off the main thread), and database open.
    let store = match OpfsSqliteStore::open(&db_name).await {
        Ok(s) => {
            report.record("open_store", Ok(format!("opened OPFS db '{db_name}'")));
            s
        }
        Err(e) => {
            report.record("open_store", Err(format!("open failed: {e}")));
            return report; // nothing downstream can run without a store
        }
    };

    // A structurally-valid kind:1 fixture. `insert` gates only on hex lengths
    // (id 64, pubkey 64, sig 128) — there is no signature check at the store
    // layer (verification happens upstream in the kernel) — so a deterministic
    // fixture is accepted and stored.
    let event = EngineEvent {
        id: "f00dbabe00000000000000000000000000000000000000000000000000000000".to_owned(),
        pubkey: "ab".repeat(32),
        created_at: 1_700_000_000,
        kind: 1,
        tags: vec![vec!["t".to_owned(), "opfs".to_owned()]],
        content: "hello opfs-sahpool".to_owned(),
        sig: "00".repeat(64),
    };
    let id = match event.id_bytes() {
        Some(id) => id,
        None => {
            report.record(
                "fixture_id",
                Err("fixture id is not 64-char hex".to_owned()),
            );
            return report;
        }
    };
    let received_at_ms: u64 = 1_700_000_000_123;

    // ── Step 2: open() auto-migrated the real schema ──────────────────────
    // PR-3's `open` creates the full events schema; a fresh OPFS db must report
    // an empty, queryable `events` table (proves migration ran — the harness no
    // longer hand-rolls DDL).
    report.record(
        "schema_ready",
        (|| {
            let cell = store.conn();
            let conn = cell.borrow();
            let stmt = conn
                .prepare("SELECT count(*) FROM events")
                .map_err(|e| e.to_string())?;
            if !stmt.step().map_err(|e| e.to_string())? {
                return Err("count query returned no row".to_owned());
            }
            let n = stmt.column_int64(0).map_err(|e| e.to_string())?;
            if n != 0 {
                return Err(format!("fresh db already had {n} events"));
            }
            Ok("open() migrated schema; events table present and empty".to_owned())
        })(),
    );

    // ── Step 3: typed insert (#1007 PR-3 OpfsSqliteStore::insert) ──────────
    report.record(
        "insert_event",
        match store.insert(event.clone(), "conformance", received_at_ms) {
            Ok(InsertOutcome::Inserted { sources_after, .. }) => {
                Ok(format!("inserted 1 event (sources_after={sources_after})"))
            }
            Ok(other) => Err(format!("insert did not store the event: {other:?}")),
            Err(e) => Err(format!("insert failed: {e}")),
        },
    );

    // ── Step 4: typed point read (get_by_id) + value assert ───────────────
    report.record(
        "read_back_event",
        match store.get_by_id(&id) {
            Ok(Some(stored)) => {
                if stored.event.id != event.id {
                    Err(format!("id mismatch: got {}", stored.event.id))
                } else if stored.event.content != event.content {
                    Err(format!("content mismatch: got {:?}", stored.event.content))
                } else if stored.received_at_ms != received_at_ms {
                    Err(format!(
                        "received_at_ms mismatch: got {}",
                        stored.received_at_ms
                    ))
                } else {
                    Ok(format!(
                        "round-tripped id={} kind={} via typed insert/get_by_id",
                        stored.event.id, stored.event.kind
                    ))
                }
            }
            Ok(None) => Err("get_by_id returned None for the inserted event".to_owned()),
            Err(e) => Err(format!("get_by_id failed: {e}")),
        },
    );

    // ── Step 5: persistence across close + reopen ─────────────────────────
    // Drop the store (its `Drop` closes the db handle), reopen the same OPFS
    // database, and confirm the event is still readable via the typed point
    // read: real durability, not an in-memory illusion.
    drop(store);
    let store2 = match OpfsSqliteStore::open(&db_name).await {
        Ok(store2) => {
            report.record(
                "reopen_persisted",
                match store2.get_by_id(&id) {
                    Ok(Some(_)) => Ok("event survived store close + reopen".to_owned()),
                    Ok(None) => Err("event missing after reopen".to_owned()),
                    Err(e) => Err(format!("get_by_id after reopen failed: {e}")),
                },
            );
            store2
        }
        Err(e) => {
            report.record("reopen_persisted", Err(format!("reopen failed: {e}")));
            return report; // nothing downstream can run without a store
        }
    };

    // ── Steps 6–10: PR-4 scan / query read paths ──────────────────────────
    scan_corpus_steps(&mut report, &store2);

    // ── Step 11: #2223 relay-kind privacy gate ────────────────────────────
    relay_kind_privacy::record(&mut report, &store2);

    report
}

/// `["k", "v"]` tag-row helper for the corpus fixtures.
fn tag(key: &str, value: &str) -> Vec<String> {
    vec![key.to_owned(), value.to_owned()]
}

/// Insert a small multi-event corpus and assert the PR-4 read surface:
/// single-author scan ordering, multi-author **global** newest-first ordering,
/// the index-served AND-across-two-letters / OR-within-values tag scan, and the
/// `query_visit` budget loop. These steps prove the scans are real (durable
/// rows, correct order, tag-index parity), not just that they compile.
fn scan_corpus_steps(report: &mut Report, store: &OpfsSqliteStore) {
    // Three disjoint authors (raw bytes are the hex pubkeys decoded): the
    // ordering corpus (kind 1, authors A/B) and the tag corpus (kind 7, author
    // C) never cross-contaminate each other's assertions.
    let (a_hex, b_hex, c_hex) = ("11".repeat(32), "22".repeat(32), "33".repeat(32));
    let (a, b) = ([0x11u8; 32], [0x22u8; 32]);

    let mk = |id_n: u64, pk: &str, created: u64, kind: u32, tags: Vec<Vec<String>>| EngineEvent {
        id: format!("{id_n:064x}"),
        pubkey: pk.to_owned(),
        created_at: created,
        kind,
        tags,
        content: String::new(),
        sig: "00".repeat(64),
    };

    // id helpers (the `{:064x}` ids the corpus uses), newest-first per query.
    let id = |n: u64| format!("{n:064x}");

    let corpus = [
        // Ordering corpus — kind 1, interleaved created_at across A and B.
        mk(0xA1, &a_hex, 1000, 1, vec![]),
        mk(0xA2, &a_hex, 3000, 1, vec![]),
        mk(0xB1, &b_hex, 2000, 1, vec![]),
        mk(0xB2, &b_hex, 4000, 1, vec![]),
        // Tag corpus — kind 7, author C. Target: `#e ∈ {evx,evy} AND #p = p1`.
        mk(0x71, &c_hex, 5000, 7, vec![tag("e", "evx"), tag("p", "p1")]), // match
        mk(0x72, &c_hex, 6000, 7, vec![tag("e", "evy"), tag("p", "p1")]), // match
        mk(0x73, &c_hex, 7000, 7, vec![tag("e", "evx")]),                 // missing #p
        mk(0x74, &c_hex, 8000, 7, vec![tag("p", "p1")]),                  // missing #e
        mk(0x75, &c_hex, 9000, 7, vec![tag("e", "evz"), tag("p", "p1")]), // #e ∉ {x,y}
    ];

    // ── Step 6: insert the corpus ─────────────────────────────────────────
    let inserted_ok = report.record(
        "insert_scan_corpus",
        (|| {
            for ev in &corpus {
                match store.insert(ev.clone(), "conformance", 1_700_000_000_000) {
                    Ok(InsertOutcome::Inserted { .. }) => {}
                    Ok(other) => return Err(format!("id={} not stored: {other:?}", ev.id)),
                    Err(e) => return Err(format!("insert id={} failed: {e}", ev.id)),
                }
            }
            Ok(format!("inserted {} corpus events", corpus.len()))
        })(),
    );
    if !inserted_ok {
        return; // the scan assertions are meaningless without the corpus
    }

    // ── Step 7: scan_by_author_kind — author A, kind 1, newest-first ──────
    report.record(
        "scan_by_author_kind",
        match store.scan_by_author_kind(&a, &[1], None, None, 100) {
            Ok(rows) => {
                let got: Vec<String> = rows.into_iter().map(|s| s.event.id).collect();
                let want = vec![id(0xA2), id(0xA1)]; // 3000 then 1000
                if got == want {
                    Ok("author A kind 1 newest-first: [A2, A1]".to_owned())
                } else {
                    Err(format!("got {got:?}, want {want:?}"))
                }
            }
            Err(e) => Err(format!("scan failed: {e}")),
        },
    );

    // ── Step 8: scan_by_authors_kind — GLOBAL order across A and B ────────
    report.record("scan_by_authors_kind_global_order", {
        let authors: BTreeSet<[u8; 32]> = [a, b].into_iter().collect();
        match store.scan_by_authors_kind(&authors, &[1], None, None, 100) {
            Ok(rows) => {
                let got: Vec<String> = rows.into_iter().map(|s| s.event.id).collect();
                // Interleaved by created_at desc, NOT grouped by author:
                // B2(4000), A2(3000), B1(2000), A1(1000).
                let want = vec![id(0xB2), id(0xA2), id(0xB1), id(0xA1)];
                if got == want {
                    Ok("merged newest-first across A+B: [B2, A2, B1, A1]".to_owned())
                } else {
                    Err(format!("got {got:?}, want {want:?}"))
                }
            }
            Err(e) => Err(format!("scan failed: {e}")),
        }
    });

    // ── Step 9: scan_by_tags — AND across #e/#p, OR within #e values ──────
    report.record("scan_by_tags_index_served", {
        let mut tags: BTreeMap<char, BTreeSet<String>> = BTreeMap::new();
        tags.insert(
            'e',
            ["evx".to_owned(), "evy".to_owned()].into_iter().collect(),
        );
        tags.insert('p', ["p1".to_owned()].into_iter().collect());
        // authors/kinds empty = any: prove the tag index alone selects.
        match store.scan_by_tags(&BTreeSet::new(), &[], &tags, None, None, 100) {
            Ok(rows) => {
                let got: Vec<String> = rows.into_iter().map(|s| s.event.id).collect();
                // Only T2(6000) and T1(5000): T3 lacks #p, T4 lacks #e, T5's
                // #e=evz ∉ {evx,evy}. Newest-first.
                let want = vec![id(0x72), id(0x71)];
                if got == want {
                    Ok("AND(#e∈{evx,evy}, #p=p1) ⇒ [T2, T1] via tag index".to_owned())
                } else {
                    Err(format!("got {got:?}, want {want:?}"))
                }
            }
            Err(e) => Err(format!("scan failed: {e}")),
        }
    });

    // ── Step 10: query_visit budget — visit only `budget` rows ────────────
    report.record("query_visit_budget", {
        let query = EngineQuery::AuthorsKind {
            authors: [a, b].into_iter().collect(),
            kinds: vec![1],
            since: None,
            until: None,
        };
        let mut seen: Vec<String> = Vec::new();
        match store.query_visit(&query, 2, &mut |ev| {
            seen.push(ev.event.id.clone());
            ControlFlow::Continue(())
        }) {
            Ok(()) => {
                // Budget 2 ⇒ exactly the two newest: B2(4000), A2(3000).
                let want = vec![id(0xB2), id(0xA2)];
                if seen == want {
                    Ok("budget=2 visited exactly [B2, A2] (newest-first)".to_owned())
                } else {
                    Err(format!("visited {seen:?}, want {want:?}"))
                }
            }
            Err(e) => Err(format!("query_visit failed: {e}")),
        }
    });
}

/// Dedicated-Worker entry point.
///
/// Invoked from `web/worker.js` inside a dedicated Web Worker. Resolves with the
/// JSON [`Report`] when every assertion passes; **rejects** with the same JSON
/// when any assertion fails, so the host page renders the exact failing step
/// and the headless runner exits non-zero. The harness never reports a false
/// pass: a thrown panic also rejects (via `console_error_panic_hook`).
#[wasm_bindgen]
pub async fn run_conformance() -> Result<JsValue, JsValue> {
    console_error_panic_hook::set_once();
    let report = run().await;
    let json = report.to_json();
    if report.passed() {
        Ok(JsValue::from_str(&json))
    } else {
        Err(JsValue::from_str(&json))
    }
}

/// Compiled ownership descriptor for crate-ownership reports.
pub mod ownership;
