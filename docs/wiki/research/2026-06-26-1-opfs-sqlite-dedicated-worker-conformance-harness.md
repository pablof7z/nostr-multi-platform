---
type: research-record
date: 2026-06-26
session: 2b86015b-6b6b-44e9-a870-3b16c0763d7f
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/2b86015b-6b6b-44e9-a870-3b16c0763d7f.jsonl
source_lines: 633-678
agent_attribution: ac4fcc4e11dae24c1
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "OPFS-SQLite dedicated-Worker conformance harness evaluation — five test assertions pass in real Chrome"
captured_at: 2026-06-26T18:29:48Z
---

OPFS-SQLite dedicated-Worker conformance harness evaluation — five test assertions pass in real Chrome

---

User: [Agent task result: Agent "PR-6 worker conformance vehicle" finished]
The CI gate "OPFS dedicated-Worker conformance (headless Chromium)" registered and is pending — the workflow triggered correctly on this PR.

---

📋 SUMMARY: Built and shipped PR-6 of #1007 — a dedicated-Worker OPFS-SQLite conformance vehicle for `nmp-sqlite-wasm`, verified genuinely passing in real Chrome, with a blocking CI gate. PR: https://github.com/pablof7z/nostr-multi-platform/pull/2153

🔍 ANALYSIS:
- My worktree was stale (predated the PR-2 merge); I rebased it onto `origin/master` (`46954afb3`) so the harness builds against the real engine (`OpfsSqliteStore::open`, `conn()`, and the public `shim` API).
- Load-bearing finding: `nmp-sqlite-wasm`'s dependency graph is pure JS/Rust interop with **no `secp256k1-sys`**. That is the difference that lets this be a *real blocking* wasm32 gate, unlike `browser-runtime.yml`'s non-blocking job which is C-toolchain-blocked.
- The vendored shim's `import "./sqlite3.mjs"` sibling is **not** auto-copied by wasm-bindgen (it doesn't parse JS imports). This makes `wasm-bindgen-test-runner` unviable (no injection hook); a custom `--target web` + explicit sibling-copy + Playwright runner is the correct, controllable path.

⚡ ACTIONS:
- New crate `crates/nmp-sqlite-wasm-conformance` (cdylib, wasm32-only behavior) — production `nmp-sqlite-wasm` untouched, fully disjoint from in-flight PR-3.
- JS glue: `web/build.sh` (cargo→wasm-bindgen→stage sqlite engine→copy Worker/host), `web/worker.js` (dedicated Worker), `web/index.html`, `web/run-conformance.mjs` (headless driver), `web/package.json` (+ lockfile).
- Registered crate in `Cargo.toml` members and `release/nmp-release.toml` as a private package.
- New blocking workflow `.github/workflows/sqlite-wasm-conformance.yml` (wasm-bindgen-cli pinned to `Cargo.lock`).
- Ran the harness locally against system Chrome; ran all three required gates.

✅ RESULTS:
- Local run in real Chrome: all five assertions PASS, exit 0 — `open_store`, `create_table`, `insert_event`, `read_back_event`, `reopen_persisted` (real OPFS durability across close+reopen).
- `ci/check-release-manifest.sh` green; `ci/verify-sqlite-wasm-artifact.sh` green; `cargo test -p nmp-testing --test doctrine_lint_smoke` green (incl. the D20 wasm-reachable check on the new crate).
- File-size gate: only a soft warning on `release/nmp-release.toml` (a data manifest, +5 lines, not splittable) — no hard failures.
- CI gate registered and pending on the PR.

📊 STATUS: PR #2153 open against master; CI running. Harness proven to actually execute.

📁 CAPTURE: Key worktree paths — harness crate `crates/nmp-sqlite-wasm-conformance/src/lib.rs` (the `run()` assertion sequence that PR-3/4/5 extend), `web/build.sh` (the sibling-copy that makes the vendored sqlite reachable), `web/run-conformance.mjs` (headless driver), `.github/workflows/sqlite-wasm-conformance.yml` (blocking gate). The no-`secp256k1` → blocking-gate insight is the reusable lesson for this crate.

➡️ NEXT:
- Watch the CI "OPFS dedicated-Worker conformance" job on PR #2153; if headless OPFS is unavailable on the runner (local evidence says it is available), flip to the documented non-blocking fallback.
- When PR-3 merges, swap the raw `exec`/`prepare` steps in `run()` for the typed `OpfsSqliteEventStore` insert/point-read methods, then add scan + gc-bound steps.

📖 STORY EXPLANATION:
1. The OPFS SyncAccessHandle pool VFS only works inside a dedicated Web Worker — `createSyncAccessHandle()` doesn't exist on the page main thread, so the repo's existing `run_in_browser` wasm tests structurally cannot cover this backend.
2. I found my worktree was based before the PR-2 merge, so I reset it to current master to build the harness against the real engine API (and restored the main checkout I'd accidentally touched back to master).
3. I confirmed the engine's public surface: async `OpfsSqliteStore::open`, `conn()`, and the `shim` module's `SqliteConn`/`SqliteStmt` — enough to drive a full SQL round-trip without depending on the parallel PR-3.
4. The decisive architectural call: because this crate has no `secp256k1-sys`, wasm32 builds with the stock toolchain, so this gate can genuinely run and block — a strength the broader browser-runtime job lacks.
5. wasm-bindgen won't copy the vendored shim's `./sqlite3.mjs` sibling, so I built a custom `--target web` pipeline with an explicit sibling-copy step rather than the test-runner, giving deterministic control.
6. I wrote a dedicated Worker entry that instantiates the wasm and calls `run_conformance()`, a host page that surfaces the result, and a Playwright/headless-Chrome driver that serves over localhost (a secure context for OPFS) and asserts every step.
7. I ran it for real in Chrome: open + DDL + bound insert + typed select + persistence-across-reopen all passed with exit 0 — compiling was never treated as proof.
8. I registered the crate, added a blocking CI workflow with wasm-bindgen-cli pinned to the lockfile, kept all required gates green, committed, pushed, and opened PR #2153 — never pushing to master directly.

🎯 COMPLETED: OPFS dedicated-Worker conformance vehicle shipped; five assertions pass in Chrome.

Environment note: the voice notification server at `localhost:8888` was offline throughout, so spoken notifications were skipped (attempted, best-effort).
