//! Dedicated-Worker OPFS-SQLite conformance vehicle for `nmp-sqlite-wasm`
//! (#1007 PR-6).
//!
//! ## Why this crate exists (the Worker-only constraint)
//!
//! The `nmp-sqlite-wasm` backend runs SQLite over the OPFS *SyncAccessHandle
//! pool* VFS. `createSyncAccessHandle()` — the synchronous file primitive the
//! pool VFS is built on — **only exists inside a dedicated Web Worker**; it is
//! absent on the page main thread. The repo's existing wasm test setup
//! (`crates/nmp-wasm/tests/`, `wasm_bindgen_test_configure!(run_in_browser)`)
//! executes on the **main thread**, so it structurally cannot exercise this
//! backend. This crate is the missing vehicle: a `wasm-bindgen --target web`
//! cdylib whose single exported entry point ([`run_conformance`]) is invoked
//! *from inside a dedicated Worker* (see `web/worker.js`), driven by a headless
//! browser (`web/run-conformance.mjs`). It is the sole end-to-end proof that
//! the novel sync-over-OPFS path actually executes.
//!
//! ## What it proves today (PR-6, against PR-2's engine)
//!
//! PR-3 (the `OpfsSqliteEventStore` inherent insert/point-read methods) is in
//! flight in parallel, so this harness does not depend on it. It proves the
//! load-bearing primitive instead, through the PR-2 shim that is on master:
//!
//! 1. `open_store` — module init + opfs-sahpool VFS install + database open.
//! 2. `create_table` / `insert_event` / `read_back_event` — a raw SQL
//!    round-trip (DDL + bound INSERT + SELECT with typed column reads).
//! 3. `reopen_persisted` — close the store, reopen the same OPFS database, and
//!    confirm the row survived: real OPFS durability, the backend's whole point.
//!
//! ## How it grows as the engine lands
//!
//! Each assertion is an independent recorded [`Step`]. As PR-3/4/5 add inherent
//! methods to the store (`insert`, point reads, filter scans, gc), the raw
//! `exec`/`prepare` steps here are replaced — assertion for assertion — by calls
//! to those typed methods, and new steps (scan, gc-bound) slot into [`run`]
//! without touching the Worker/host/CI plumbing. The harness shape is fixed;
//! only the body of [`run`] tracks the engine surface.
//!
//! On native this crate cfg-compiles to nothing (the engine it drives is
//! wasm32-only), mirroring `nmp-sqlite-wasm`'s own target gating.

#![cfg(target_arch = "wasm32")]

use nmp_sqlite_wasm::OpfsSqliteStore;
use wasm_bindgen::prelude::*;

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

    // Fixture row. PR-3 swaps the raw SQL below for the typed
    // `OpfsSqliteEventStore` insert/point-read methods over a real nostr event.
    let fixture_id = "f00dbabe00000000000000000000000000000000000000000000000000000000";
    let fixture_kind: i64 = 1;
    let fixture_raw: &[u8] = br#"{"kind":1,"content":"hello opfs-sahpool"}"#;

    {
        let cell = store.conn();
        let conn = cell.borrow();

        // ── Step 2: DDL ──────────────────────────────────────────────────
        report.record(
            "create_table",
            conn.exec(
                "CREATE TABLE events (\
                   id   TEXT PRIMARY KEY, \
                   kind INTEGER NOT NULL, \
                   raw  BLOB NOT NULL)",
            )
            .map(|()| "events table created".to_owned())
            .map_err(|e| e.to_string()),
        );

        // ── Step 3: bound INSERT (text + int64 + blob params) ─────────────
        report.record(
            "insert_event",
            (|| {
                let stmt = conn
                    .prepare("INSERT INTO events (id, kind, raw) VALUES (?1, ?2, ?3)")
                    .map_err(|e| e.to_string())?;
                stmt.bind_text(1, fixture_id).map_err(|e| e.to_string())?;
                stmt.bind_int64(2, fixture_kind).map_err(|e| e.to_string())?;
                stmt.bind_blob(3, fixture_raw).map_err(|e| e.to_string())?;
                if stmt.step().map_err(|e| e.to_string())? {
                    return Err("INSERT unexpectedly produced a result row".to_owned());
                }
                Ok("inserted 1 event".to_owned())
            })(),
        );

        // ── Step 4: point read + typed column decode + value assert ───────
        report.record(
            "read_back_event",
            (|| {
                let stmt = conn
                    .prepare("SELECT id, kind, raw FROM events WHERE id = ?1")
                    .map_err(|e| e.to_string())?;
                stmt.bind_text(1, fixture_id).map_err(|e| e.to_string())?;
                if !stmt.step().map_err(|e| e.to_string())? {
                    return Err("SELECT returned no row".to_owned());
                }
                let id = stmt.column_text(0).map_err(|e| e.to_string())?;
                let kind = stmt.column_int64(1).map_err(|e| e.to_string())?;
                let raw = stmt.column_blob(2).map_err(|e| e.to_string())?;
                if id != fixture_id {
                    return Err(format!("id mismatch: got {id}"));
                }
                if kind != fixture_kind {
                    return Err(format!("kind mismatch: got {kind}"));
                }
                if raw.as_slice() != fixture_raw {
                    return Err(format!("raw blob mismatch: got {} bytes", raw.len()));
                }
                Ok(format!("round-tripped event id={id} kind={kind} raw={}B", raw.len()))
            })(),
        );
    }

    // ── Step 5: persistence across close + reopen ─────────────────────────
    // Drop the store (its `Drop` closes the db handle), reopen the same OPFS
    // database, and confirm the row is still there: real durability, not an
    // in-memory illusion.
    drop(store);
    match OpfsSqliteStore::open(&db_name).await {
        Ok(store2) => {
            report.record(
                "reopen_persisted",
                (|| {
                    let cell = store2.conn();
                    let conn = cell.borrow();
                    let stmt = conn
                        .prepare("SELECT count(*) FROM events")
                        .map_err(|e| e.to_string())?;
                    if !stmt.step().map_err(|e| e.to_string())? {
                        return Err("count query returned no row".to_owned());
                    }
                    let n = stmt.column_int64(0).map_err(|e| e.to_string())?;
                    if n != 1 {
                        return Err(format!("expected 1 persisted event, found {n}"));
                    }
                    Ok("event survived store close + reopen".to_owned())
                })(),
            );
            // Best-effort cleanup so the OPFS origin does not accrue tables.
            let _ = store2.conn().borrow().exec("DROP TABLE events");
        }
        Err(e) => {
            report.record("reopen_persisted", Err(format!("reopen failed: {e}")));
        }
    }

    report
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
