---
type: research-record
date: 2026-06-26
session: 2b86015b-6b6b-44e9-a870-3b16c0763d7f
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/2b86015b-6b6b-44e9-a870-3b16c0763d7f.jsonl
source_lines: 1192-1219
agent_attribution: a3758d815f5180c20
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "OPFS-SQLite scan/query implementation with LMDB tag-index parity: 10 conformance steps PASS, 18 native tests green"
captured_at: 2026-06-26T18:42:27Z
---

OPFS-SQLite scan/query implementation with LMDB tag-index parity: 10 conformance steps PASS, 18 native tests green

---

User: [Agent task result: Agent "PR-4 scans + tag-index parity" finished]
The voice server is unavailable in this environment, but the work is complete and pushed.

📋 SUMMARY: PR-4 of #1007 ships the OPFS-SQLite scan/query read paths with LMDB tag-index parity, verified by a real headless-Chrome conformance pass.

🔍 ANALYSIS: The crate carries no `nmp-store`/`nostr` dependency, so I used crate-local types (`char` tag letters, an `EngineQuery` mirror of `StoreQuery`, `Vec<StoredEngineEvent>` results). The hardest call was the cross-author `(kind, d_tag)` scan: the existing `idx_events_dtag` leads with `pubkey` and cannot seek it, and routing through `event_tags` would miss param-replaceable events with an implicit empty `d` — so a dedicated `idx_events_kind_dtag` index was the architecturally correct fix.

⚡ ACTIONS: Read the full engine (schema/store_impl/insert/conv/shim) plus the `nmp-store` `EventStore` trait, LMDB/mem reference semantics, and the cache-serve budget driver; implemented pure SQL builders + the wasm bind/step face TDD-first; ran the native, wasm, doctrine, file-size, release, and artifact gates; then ran the dedicated-Worker harness in headless Chrome.

✅ RESULTS: All 10 conformance steps PASS over real OPFS, including the three required new assertions (single-author ordering, multi-author global ordering, AND/OR index-served tag scan) plus a `query_visit` budget proof. 18 native tests, doctrine-lint 0 findings, doctrine_lint_smoke 98 pass, both verify scripts exit 0, file-size gate exit 0.

📊 STATUS: PR open at https://github.com/pablof7z/nostr-multi-platform/pull/2157 (branch `worktree-agent-a3758d815f5180c20`, commit `e6dde9cf3`, base `master`). Working tree clean — build artifacts cleaned.

📁 CAPTURE: scan_by_tags stays index-served via an `event_tags` subquery (`OR` within/across letters + `GROUP BY event_id HAVING COUNT(DISTINCT tag_name)=N`), with author/kind/time pushed INTO the subquery (atci/ktci seek) since each tag row carries the event's redundant pubkey/kind/created_at. `query_visit` budget = SQL `LIMIT`, one unit per visitor call, early `ControlFlow::Break`.

➡️ NEXT: The `impl EventStore for OpfsSqliteEventStore` wrapper in `nmp-store` (mapping `StoreQuery → EngineQuery`, `Vec<StoredEngineEvent> → Box<dyn EventIter>`, `SqliteWasmError → StoreError`) is the follow-up PR. Merge requires the supervisor's workspace-wide gate.

📖 STORY EXPLANATION:
1. PR-4 adds the read side of the wasm OPFS-SQLite store: six materializing `scan_by_*` methods plus a streaming `query_visit`, all inherent methods, no trait impl, no stubs.
2. Every scan maps onto an existing composite index so ordering (`created_at DESC, id ASC`) is an index walk, never a sort — `aci`/`akci`/`kci`/`ci` for the primary table.
3. The cross-author `(kind, d_tag)` scan needed a new `idx_events_kind_dtag` index because the old one leads with pubkey; I added it to the always-applied DDL (no version bump needed, no production dbs).
4. `scan_by_tags` achieves LMDB tag parity index-served: candidates come from `event_tags` (tci/atci/ktci), AND across letters via a distinct-letter `HAVING` count, OR within values via `IN`, author/kind/time pushed into the indexed subquery.
5. `query_visit` is an O(log n) seek + O(budget) step loop that consumes one budget unit per visitor call and honors early `Break`, mirroring the kernel's cache-serve tick budget.
6. I kept the pure SQL builders target-agnostic in `query/sql.rs` so the trickiest logic is unit-tested on native (12 builder tests), and split the tests into a child file to stay under the 500-LOC hard cap.
7. The conformance harness gained a 9-event corpus and four new assertions; it ran in real headless Chrome inside a dedicated Web Worker over OPFS — compiles-equals-works is not assumed in this repo.
8. All gates are green and the PR is open at #2157 against master, ready for the supervisor's pre-merge workspace gate.

🎯 COMPLETED: PR-4 scan and query read paths shipped; headless Chrome conformance passing.
