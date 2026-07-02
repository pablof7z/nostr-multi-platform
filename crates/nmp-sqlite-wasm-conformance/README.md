# nmp-sqlite-wasm-conformance

Dedicated-Worker OPFS conformance vehicle for `nmp-sqlite-wasm` (issue #1007, PR-6).

## Why this exists

`nmp-sqlite-wasm` runs SQLite over the OPFS **SyncAccessHandle pool** VFS
("opfs-sahpool"). The synchronous file primitive it is built on,
`createSyncAccessHandle()`, **only exists inside a dedicated Web Worker** — it is
absent on the page main thread. The repository's existing wasm tests
(`crates/nmp-browser-runtime/src/wasm/*_tests.rs`) run on
the **main thread**, so they structurally cannot exercise this backend. This
crate is the missing vehicle: a `wasm-bindgen --target web` cdylib whose single
entry point is invoked from inside a dedicated Worker and driven by a headless
browser. It is the sole end-to-end proof that the novel sync-over-OPFS path
actually executes.

## What it proves today

As of #1007 PR-5 the harness drives the store's full typed surface end to end,
inside the dedicated Worker. Each assertion is an independent recorded step:

1. `open_store` — sqlite module init + opfs-sahpool VFS install + database open.
2. `schema_ready` — `open()` auto-migrated the full events schema (fresh db has
   a queryable, empty `events` table; no hand-rolled DDL).
3. `insert_event` — a structurally-valid kind:1 event through the typed
   `OpfsSqliteStore::insert`, asserting an `Inserted` outcome.
4. `read_back_event` — typed `OpfsSqliteStore::get_by_id` point read with
   field-level value assertions.
5. `reopen_persisted` — close the store, reopen the same OPFS database, and
   confirm the event is still readable: real OPFS durability, the backend's
   whole point.
6. `insert_scan_corpus` — nine events across three authors and two kinds.
7. `scan_by_author_kind` — single-author newest-first ordering.
8. `scan_by_authors_kind_global_order` — **global** `(created_at desc)` merge
   across authors (interleaved, not grouped by author).
9. `scan_by_tags_index_served` — AND-across-`#e`/`#p`, OR-within-`#e`-values
   tag intersection, proving the `event_tags` index-served LMDB parity.
10. `query_visit_budget` — the streaming visitor honours the per-call budget
    (budget 2 visits exactly the two newest rows).
11. `relay_kind_privacy_gate` — relay-kind coverage/count hides private
    NIP-04/17/59 kinds even though SQLite derives the projection from
    provenance rows.

## How it grows as the engine lands

The harness shape (Worker entry, host page, headless runner, CI) is fixed. Each
assertion is an independent recorded `Step` in `src/lib.rs::run`. As the engine
gains new typed surface, new steps slot in without touching the Worker/host/CI
plumbing — only the body of `run` tracks the engine surface.

## Layout

| Path | Role |
| --- | --- |
| `src/lib.rs` | `run_conformance()` — the wasm-bindgen entry, plus the assertion sequence. wasm32-only; compiles to nothing on native. |
| `web/build.sh` | cargo build (wasm32) → `wasm-bindgen --target web` → stage the vendored `sqlite3.mjs`/`sqlite3.wasm` next to the copied shim snippet → copy the Worker entry + host page into `pkg/`. |
| `web/worker.js` | The dedicated Worker: instantiates the wasm and calls `run_conformance()`. The OPFS engine MUST live here, not on the main thread. |
| `web/index.html` | Host page: spawns the Worker and surfaces the result for the runner. |
| `web/run-conformance.mjs` | Headless-browser driver: serves `pkg/` on `http://127.0.0.1` (a secure context for OPFS), loads the page, asserts every step, exits non-zero on any failure. |

## Run it locally

```bash
cd crates/nmp-sqlite-wasm-conformance/web
npm install                       # Playwright (JS package)
npx playwright install chromium   # or use system Chrome (see below)
npm test                          # build.sh + run-conformance.mjs
```

To drive the system Google Chrome instead of the Playwright-bundled Chromium
(handy when only Chrome is installed):

```bash
PLAYWRIGHT_BROWSER_CHANNEL=chrome npm test
```

## CI

`.github/workflows/sqlite-wasm-conformance.yml` runs this as a **blocking** gate
in headless Chromium. It can be blocking — unlike `browser-runtime.yml`'s
non-blocking wasm32 job — because this crate's dependency graph has no
`secp256k1-sys`, so `wasm32-unknown-unknown` builds with the stock toolchain (no
clang-wasm / emcc needed). `wasm-bindgen-cli` is pinned to the `wasm-bindgen`
version in `Cargo.lock` so a lock bump cannot silently break the schema match.
