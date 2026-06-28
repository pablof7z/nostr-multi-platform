# nmp-sqlite-wasm-conformance

Dedicated-Worker OPFS conformance vehicle for `nmp-sqlite-wasm` (issue #1007, PR-6).

## Why this exists

`nmp-sqlite-wasm` runs SQLite over the OPFS **SyncAccessHandle pool** VFS
("opfs-sahpool"). The synchronous file primitive it is built on,
`createSyncAccessHandle()`, **only exists inside a dedicated Web Worker** — it is
absent on the page main thread. The repository's existing wasm tests
(`crates/nmp-browser-runtime/tests/`, `wasm_bindgen_test_configure!(run_in_browser)`) run on
the **main thread**, so they structurally cannot exercise this backend. This
crate is the missing vehicle: a `wasm-bindgen --target web` cdylib whose single
entry point is invoked from inside a dedicated Worker and driven by a headless
browser. It is the sole end-to-end proof that the novel sync-over-OPFS path
actually executes.

## What it proves today

Built against PR-2's shim (on master); does **not** depend on PR-3. Each
assertion is an independent recorded step:

1. `open_store` — sqlite module init + opfs-sahpool VFS install + database open.
2. `create_table` / `insert_event` / `read_back_event` — a raw SQL round-trip
   (DDL, a bound INSERT over text/int64/blob params, a SELECT with typed column
   reads asserted against the inserted values).
3. `reopen_persisted` — close the store, reopen the same OPFS database, and
   confirm the row survived: real OPFS durability, the backend's whole point.

## How it grows as the engine lands

The harness shape (Worker entry, host page, headless runner, CI) is fixed. As
PR-3/4/5 add inherent methods to the store (`insert`, point reads, filter scans,
gc), the raw `exec`/`prepare` steps in `src/lib.rs::run` are replaced — assertion
for assertion — by calls to those typed methods, and new steps slot in without
touching the plumbing. Only the body of `run` tracks the engine surface.

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
