//! Dedicated-Worker OPFS durability conformance vehicle for
//! `nmp-browser-runtime` (#1007 PR-9 — the final PR of #1007).
//!
//! ## Why this crate exists (coverage, not new mechanism)
//!
//! PR-2..PR-6 built and proved the OPFS-SQLite *engine* (the
//! `nmp-sqlite-wasm-conformance` vehicle proves the Worker-only sync-over-OPFS
//! path end to end). PR-7/PR-8 wired that durable store into the **real browser
//! runtime**: `NmpWasmRuntime::prepare_store` opens an `OpfsSqliteEventStore`
//! and `handle_start` injects it into the kernel reducer instead of an in-memory
//! store.
//!
//! This crate is the missing end-to-end proof that the *full durable path* works
//! over OPFS through the **real composition** (`BrowserAppBuilder` →
//! `BrowserRuntimeHandle`), not just the bare engine. It drives the three #1007
//! PR-9 proofs inside a dedicated Web Worker (the only context where the OPFS
//! SyncAccessHandle pool VFS works) and asserts every one in Rust — where it has
//! full type access to the `EventStore` read-model and the durable publish queue
//! — then rejects with a JSON report on any failure (never a faked pass).
//!
//! ## The three proofs (one real OPFS close+reopen drives all three)
//!
//! 1. **Second-launch render / hydration** — a small event corpus is inserted
//!    into a fresh OPFS db and the full runtime is composed over it; the store is
//!    then closed and the SAME OPFS db reopened. The events survive the reload
//!    and the second-launch runtime's *reducer store* serves them with no relay
//!    (`runtime_reducer_hydrates_no_relay`), and the runtime boots and produces a
//!    snapshot frame over the hydrated store (`second_launch_runtime_frame`).
//! 2. **Offline-first reads** — with NO relay connectivity ever configured, a
//!    global read over the reopened durable store returns the persisted events
//!    (`offline_first_reads`). Reads are served straight from OPFS.
//! 3. **Durable offline-publish-queue survival** — an event queued for publish
//!    while offline (a pending `PublishRecord` in the durable `DomainPublishStore`)
//!    survives the close+reopen and is reloaded by `load_pending`
//!    (`offline_publish_queue_survives`) — the seam `PublishEngine::resume_from_store`
//!    consumes at boot.
//!
//! ## How it runs
//!
//! `web/build.sh` builds the cdylib for `wasm32-unknown-unknown` (the full
//! runtime pulls `secp256k1-sys`, so the build needs a C-to-wasm toolchain —
//! clang + llvm-ar — see the CI note in `web/build.sh`), runs `wasm-bindgen
//! --target web`, stages the vendored sqlite engine next to the copied shim
//! snippet, and copies the Worker entry + host page. `web/run-conformance.mjs`
//! serves `pkg/` over `http://127.0.0.1` (a secure context for OPFS), spawns the
//! dedicated Worker, and asserts every recorded step.
//!
//! On native this crate cfg-compiles to nothing (the store it drives is
//! wasm32-only), mirroring `nmp-sqlite-wasm`'s own target gating.

#![cfg(target_arch = "wasm32")]

use std::sync::Arc;

use nmp_browser_runtime::{
    BrowserAppBuilder, BrowserRunConfig, BrowserRuntimeHandle, SnapshotOutcome,
};
use nmp_core::publish::{
    DomainPublishStore, PerRelayState, PublishRecord, PublishStore, PublishTarget,
};
use nmp_signer_iface::{SignedEvent, UnsignedEvent};
use nmp_store::{EventStore, OpfsSqliteEventStore, RawEvent, StoreQuery};
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

// ── Corpus + composition helpers ───────────────────────────────────────────────

/// Author of the whole corpus (pubkey bytes are `0xAA` repeated).
const AUTHOR_HEX: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const AUTHOR_BYTES: [u8; 32] = [0xAA; 32];
/// Three deterministic kind:1 notes, oldest→newest by `created_at`.
const CORPUS: [(u64, u64); 3] = [(0xA1, 1_000), (0xA2, 2_000), (0xA3, 3_000)];

/// Build a structurally-valid kind:1 event. The store insert gates only on hex
/// field lengths (id 64, pubkey 64, sig 128) — Schnorr verification happens
/// upstream in the kernel — so a deterministic fixture inserts cleanly.
/// `from_store_verified_unchecked` is the same seam `nmp-core`'s cache-serve
/// uses to re-admit already-verified rows.
fn raw_note(id_n: u64, created_at: u64) -> RawEvent {
    RawEvent {
        id: format!("{id_n:064x}"),
        pubkey: AUTHOR_HEX.to_owned(),
        created_at,
        kind: 1,
        tags: vec![],
        content: format!("conformance note {id_n:#x}"),
        sig: "00".repeat(64),
    }
}

/// Compose the full browser runtime over `store` exactly as `handle_start` does
/// (#1007 PR-7 storage gate): inject the durable store, consume the builtin
/// projections, configure NO relays (offline by construction), decide providers,
/// install the system clock, and start. `start()` runs the browser runtime's
/// explicit owner composition.
#[allow(clippy::field_reassign_with_default)] // non_exhaustive: no struct literal / functional update
fn compose_runtime(store: Arc<dyn EventStore>, app_id: &str) -> BrowserRuntimeHandle {
    // BrowserRunConfig is #[non_exhaustive] (constructable only inside its crate
    // via a struct literal), so build it through Default and set the public field.
    let mut config = BrowserRunConfig::default();
    config.app_id = app_id.to_owned();
    BrowserAppBuilder::new()
        .inject_store(store)
        .consume_all_builtin_projections()
        .without_initial_relays()
        .decide_providers(config)
        .with_system_clock()
        .start()
}

/// The expected corpus ids, newest-first (the order every scan yields).
fn expected_ids_newest_first() -> Vec<String> {
    let mut v: Vec<(u64, u64)> = CORPUS.to_vec();
    v.sort_by(|a, b| b.1.cmp(&a.1)); // created_at desc
    v.into_iter()
        .map(|(id_n, _)| format!("{id_n:064x}"))
        .collect()
}

// ── The conformance run ─────────────────────────────────────────────────────────

/// Drive the full durability sequence over one real OPFS close+reopen.
async fn run() -> Report {
    let mut report = Report::default();

    // A fresh db name per run keeps repeated runs in the same browser origin
    // independent (OPFS persists across runs).
    let db = format!("nmp-rt-conf-{}", js_sys::Date::now() as u64);

    // ── Open the durable store (first launch) ──────────────────────────────────
    let store1 = match OpfsSqliteEventStore::open(&db).await {
        Ok(s) => {
            report.record("open_store", Ok(format!("opened OPFS db '{db}'")));
            s
        }
        Err(e) => {
            report.record("open_store", Err(format!("open failed: {e}")));
            return report; // nothing downstream can run without a store
        }
    };
    let store1_dyn: Arc<dyn EventStore> = Arc::new(store1);
    let relay: nmp_store::RelayUrl = "wss://conformance.invalid".to_owned();

    // ── Insert the event corpus (the durable read-model) ──────────────────────
    let inserted = report.record(
        "insert_corpus",
        (|| {
            for (id_n, created) in CORPUS {
                let v = nmp_store::__nmp_core_internal::from_store_verified_unchecked(raw_note(
                    id_n, created,
                ));
                match store1_dyn.insert(v, &relay, 1_700_000_000_000) {
                    Ok(nmp_store::InsertOutcome::Inserted { .. }) => {}
                    Ok(other) => return Err(format!("id={id_n:#x} not stored: {other:?}")),
                    Err(e) => return Err(format!("insert id={id_n:#x} failed: {e}")),
                }
            }
            Ok(format!("inserted {} corpus notes (kind:1)", CORPUS.len()))
        })(),
    );
    if !inserted {
        return report;
    }

    // ── Queue an event for publish WHILE OFFLINE (durable publish record) ──────
    // No relay is reachable, so the record stays `Pending` in the durable
    // DomainPublishStore — the exact state `resume_from_store` reloads at boot.
    const PUB_HANDLE: &str = "conformance-offline-publish-1";
    const PUB_RELAY: &str = "wss://offline.invalid";
    report.record(
        "queue_offline_publish",
        (|| {
            let pubstore =
                DomainPublishStore::open(Arc::clone(&store1_dyn)).map_err(|e| format!("{e:?}"))?;
            let record = offline_publish_record(PUB_HANDLE, PUB_RELAY);
            pubstore.upsert(&record).map_err(|e| format!("{e:?}"))?;
            let pending = pubstore.load_pending().map_err(|e| format!("{e:?}"))?;
            if pending.len() == 1 && pending[0].handle == PUB_HANDLE {
                Ok("1 pending publish persisted (Pending, no relay reachable)".to_owned())
            } else {
                Err(format!("expected 1 pending, got {pending:?}"))
            }
        })(),
    );

    // ── First-launch runtime composes over the durable store ──────────────────
    {
        let mut handle1 = compose_runtime(Arc::clone(&store1_dyn), "conformance");
        report.record(
            "first_launch_runtime_frame",
            match handle1.next_frame(true) {
                SnapshotOutcome::Frame(bytes) if !bytes.is_empty() => Ok(format!(
                    "composed runtime over OPFS store; frame={} bytes",
                    bytes.len()
                )),
                SnapshotOutcome::Frame(_) => Err("runtime produced an empty frame".to_owned()),
                SnapshotOutcome::Degraded { reason, .. } => {
                    Err(format!("snapshot degraded: {reason}"))
                }
                SnapshotOutcome::Panic(msg) => Err(format!("snapshot panic: {msg}")),
            },
        );
        // handle1 dropped here → its reducer's Arc<dyn EventStore> clone released.
    }

    // ── Close the store completely (drop the last Arc) ────────────────────────
    drop(store1_dyn);

    // ── Reopen the SAME OPFS db (second launch) ───────────────────────────────
    let store2 = match OpfsSqliteEventStore::open(&db).await {
        Ok(s) => s,
        Err(e) => {
            report.record("reopen_store", Err(format!("reopen failed: {e}")));
            return report;
        }
    };
    report.record("reopen_store", Ok(format!("reopened OPFS db '{db}'")));
    let store2_dyn: Arc<dyn EventStore> = Arc::new(store2);
    let want = expected_ids_newest_first();

    // ── Proof 1: events survive reload (second-launch durable read) ───────────
    report.record(
        "second_launch_events_survive",
        read_back(&*store2_dyn, author_kind_query(), &want, "author+kind scan"),
    );

    // ── Proof 2: offline-first reads (global scan, no relay, served from OPFS) ─
    report.record(
        "offline_first_reads",
        read_back(
            &*store2_dyn,
            global_kind_query(),
            &want,
            "global kind:1 scan",
        ),
    );

    // ── Proof 1 (runtime layer): the SECOND-launch runtime's reducer store
    //    hydrates the durable corpus with no relay, and the runtime renders a
    //    snapshot frame over it. ─────────────────────────────────────────────
    {
        // `inject_store` hands this exact Arc to the kernel reducer verbatim
        // (composition pointer-identity, proven in nmp-browser-runtime's native
        // `inject_store_reaches_reducer_with_pointer_identity` test). So building
        // the runtime over `store2_dyn` and then querying `store2_dyn` is querying
        // the RUNNING reducer's own store — proving it serves the hydrated rows
        // offline (no relay was ever configured).
        let mut handle2 = compose_runtime(Arc::clone(&store2_dyn), "conformance");
        report.record(
            "runtime_reducer_hydrates_no_relay",
            read_back(
                &*store2_dyn,
                author_kind_query(),
                &want,
                "injected reducer store scan (no relay)",
            ),
        );
        report.record(
            "second_launch_runtime_frame",
            match handle2.next_frame(true) {
                SnapshotOutcome::Frame(bytes) if !bytes.is_empty() => Ok(format!(
                    "second-launch runtime frame over hydrated store; {} bytes",
                    bytes.len()
                )),
                SnapshotOutcome::Frame(_) => {
                    Err("second-launch runtime produced an empty frame".to_owned())
                }
                SnapshotOutcome::Degraded { reason, .. } => {
                    Err(format!("snapshot degraded: {reason}"))
                }
                SnapshotOutcome::Panic(msg) => Err(format!("snapshot panic: {msg}")),
            },
        );
    }

    // ── Proof 3: durable offline-publish-queue survival ───────────────────────
    report.record(
        "offline_publish_queue_survives",
        (|| {
            let pubstore =
                DomainPublishStore::open(Arc::clone(&store2_dyn)).map_err(|e| format!("{e:?}"))?;
            let pending = pubstore.load_pending().map_err(|e| format!("{e:?}"))?;
            let Some(rec) = pending.iter().find(|r| r.handle == PUB_HANDLE) else {
                return Err(format!(
                    "queued publish '{PUB_HANDLE}' missing after reopen; pending={pending:?}"
                ));
            };
            if rec.per_relay.iter().any(|(url, st)| url == PUB_RELAY && matches!(st, PerRelayState::Pending)) {
                Ok("offline publish record survived close+reopen as Pending (resume_from_store will re-dispatch)".to_owned())
            } else {
                Err(format!("publish record reopened in wrong state: {:?}", rec.per_relay))
            }
        })(),
    );

    report
}

/// A minimal durable pending publish: one signed event, one relay, `Pending`.
fn offline_publish_record(handle: &str, relay: &str) -> PublishRecord {
    PublishRecord {
        handle: handle.to_owned(),
        event: SignedEvent {
            id: format!("{:064x}", 0xDEADBEEFu64),
            sig: "11".repeat(64),
            unsigned: UnsignedEvent {
                pubkey: AUTHOR_HEX.to_owned(),
                kind: 1,
                tags: vec![],
                content: "offline-queued chirp".to_owned(),
                created_at: 1_700_000_500,
            },
        },
        target: PublishTarget::Auto,
        per_relay: vec![(relay.to_owned(), PerRelayState::Pending)],
        pending_retries: vec![],
        relay_reasons: vec![],
    }
}

fn author_kind_query() -> StoreQuery {
    StoreQuery::AuthorKind {
        author: AUTHOR_BYTES,
        kinds: vec![1],
        since: None,
        until: None,
    }
}

fn global_kind_query() -> StoreQuery {
    StoreQuery::KindTime {
        kinds: vec![1],
        since: None,
        until: None,
    }
}

/// Run `query` against `store`, assert the returned ids match `want` exactly
/// (newest-first), and produce a pass/fail detail.
fn read_back(
    store: &dyn EventStore,
    query: StoreQuery,
    want: &[String],
    label: &str,
) -> Result<String, String> {
    match store.query(&query, 100) {
        Ok(rows) => {
            let got: Vec<String> = rows.into_iter().map(|s| s.raw.id.clone()).collect();
            if got == want {
                Ok(format!(
                    "{label}: {} events, newest-first, exact match",
                    got.len()
                ))
            } else {
                Err(format!("{label}: got {got:?}, want {want:?}"))
            }
        }
        Err(e) => Err(format!("{label} query failed: {e}")),
    }
}

/// Dedicated-Worker entry point.
///
/// Invoked from `web/worker.js` inside a dedicated Web Worker. Resolves with the
/// JSON [`Report`] when every assertion passes; **rejects** with the same JSON
/// when any assertion fails, so the host page renders the exact failing step and
/// the headless runner exits non-zero. The harness never reports a false pass: a
/// thrown panic also rejects (via `console_error_panic_hook`).
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
