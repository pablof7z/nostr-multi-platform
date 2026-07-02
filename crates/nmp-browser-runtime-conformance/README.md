# nmp-browser-runtime-conformance

Dedicated-Worker OPFS **durability** conformance vehicle for `nmp-browser-runtime`
(issue #1007, **PR-9** — the final PR of #1007).

## Why this exists

PR-2..PR-6 built and proved the OPFS-SQLite *engine*
(`nmp-sqlite-wasm-conformance` is the engine-level vehicle). PR-7/PR-8 wired that
durable store into the **real browser runtime**: `NmpWasmRuntime::prepare_store`
opens an `OpfsSqliteEventStore` and `handle_start` injects it into the kernel
reducer instead of an in-memory store.

This crate is the missing end-to-end proof that the *full durable path* works
over OPFS through the **real composition** (`BrowserAppBuilder` →
`BrowserRuntimeHandle`), not just the bare engine — and that the durable state
survives a real close+reopen of the same OPFS database. It is "coverage, not new
mechanism": the hydration replay (ADR-0070 store→projection) and
`PublishStore`/`PublishEngine::resume_from_store` already exist; this harness
exercises them over the durable OPFS-SQLite store inside a dedicated Worker (the
only context where the OPFS SyncAccessHandle pool VFS works) and asserts every
step in Rust — where it has full type access to the `EventStore` read-model and
the durable publish queue. It **never** reports a false pass: any failed
assertion (or a panic) rejects with the JSON report and the headless runner exits
non-zero.

## What it proves (one real OPFS close+reopen drives all three #1007 proofs)

| Step | Proof |
| --- | --- |
| `open_store` | Open a fresh OPFS-SQLite db inside the Worker. |
| `insert_corpus` | Ingest a small kind:1 corpus into the durable store. |
| `queue_offline_publish` | Queue an event for publish while offline — a `Pending` `PublishRecord` in the durable `DomainPublishStore`. |
| `first_launch_runtime_frame` | Compose the full `BrowserRuntimeHandle` over the durable store (the PR-7 storage gate, no relays) and render a snapshot frame. |
| `reopen_store` | Drop everything (closing OPFS) and reopen the **same** db — second launch. |
| `second_launch_events_survive` | **Proof 1** — the corpus survives the reload (author+kind durable scan, exact newest-first match). |
| `offline_first_reads` | **Proof 2** — with no relay ever configured, a global scan returns the persisted events straight from OPFS. |
| `runtime_reducer_hydrates_no_relay` | **Proof 1 (runtime layer)** — the second-launch runtime's own reducer store (the exact injected `Arc`, pointer-identity proven natively) serves the hydrated corpus with no relay. |
| `second_launch_runtime_frame` | The second-launch runtime renders a snapshot frame over the hydrated store. |
| `offline_publish_queue_survives` | **Proof 3** — the offline-queued publish survives close+reopen as `Pending`, ready for `resume_from_store` to re-dispatch. |

### Honest scope boundary

The hydration proofs assert the durable **read-model** (`EventStore`) the
projection cache-serve (ADR-0070) reads from — both directly and **through the
running runtime's reducer store** — plus that the full runtime composes and
renders a frame over the reopened store with no relay. They do not decode the
FlatBuffers projection payload in JS (no TS feed-projection decoder exists yet;
the Chirp feed UI is Items C/D, not landed). Asserting the reducer-backed durable
store + a rendered frame is the genuine full-stack durability signal available
today; it is named precisely so it never overclaims projection-content decoding.

## Layout

| Path | Role |
| --- | --- |
| `src/lib.rs` | `run_conformance()` — the wasm-bindgen entry + the recorded assertion sequence. wasm32-only; compiles to nothing on native. |
| `web/build.sh` | cargo build (wasm32) → `wasm-bindgen --target web` → stage the vendored `sqlite3.mjs`/`sqlite3.wasm` next to the copied shim snippet → copy the Worker entry + host page into `pkg/`. |
| `web/worker.js` | The dedicated Worker: instantiates the wasm and calls `run_conformance()`. The OPFS store MUST live here, not on the main thread. |
| `web/index.html` | Host page: spawns the Worker and surfaces the result for the runner. |
| `web/run-conformance.mjs` | Headless-browser driver: serves `pkg/` on `http://127.0.0.1` (a secure context for OPFS), loads the page, asserts every step, exits non-zero on any failure. |

## Run it locally

The full runtime pulls `secp256k1-sys` through `nmp-core`, so the wasm32 build
needs a C-to-wasm toolchain (clang's wasm backend + `llvm-ar`):

```bash
cd crates/nmp-browser-runtime-conformance/web
npm install                       # Playwright (JS package)
npx playwright install chromium   # or use system Chrome (see below)

CC_wasm32_unknown_unknown=/opt/homebrew/opt/llvm/bin/clang \
AR_wasm32_unknown_unknown=/opt/homebrew/opt/llvm/bin/llvm-ar \
npm test                          # build.sh + run-conformance.mjs
```

To drive system Google Chrome instead of the Playwright-bundled Chromium:

```bash
PLAYWRIGHT_BROWSER_CHANNEL=chrome \
CC_wasm32_unknown_unknown=/opt/homebrew/opt/llvm/bin/clang \
AR_wasm32_unknown_unknown=/opt/homebrew/opt/llvm/bin/llvm-ar \
npm test
```

## CI

`.github/workflows/browser-runtime-conformance.yml` runs this in headless
Chromium. Unlike `sqlite-wasm-conformance.yml` (a genuinely *blocking* gate
because `nmp-sqlite-wasm` has no C dependency), this gate is **release-gating
rather than ordinary-PR-gating**.

The honest reason is the same constraint documented in `browser-runtime.yml`'s
true wasm32 job: building the full runtime to wasm32 needs a wasm C toolchain
(`secp256k1-sys`), Playwright, headless Chromium, and a debug wasm bundle.
Installing and running that stack adds minutes to every run, so ordinary PRs
continue to rely on the faster browser-runtime compile/test gates. The full OPFS
durability harness runs where it can block shipping:

- every `nmp-v*` release tag through `release-readiness`, which calls this
  workflow against the exact tagged SHA;
- direct `browser-runtime-conformance` runs for `release/**` branch pushes and
  PRs that touch browser runtime, OPFS/store, core reducer/hydration,
  feed/reactivity compiler, or the harness itself.

The coverage is **not skipped**: it also runs nightly and can be triggered on
any branch via `workflow_dispatch`.
