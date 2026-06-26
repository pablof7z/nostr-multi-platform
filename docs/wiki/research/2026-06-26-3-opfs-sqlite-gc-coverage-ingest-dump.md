---
type: research-record
date: 2026-06-26
session: 2b86015b-6b6b-44e9-a870-3b16c0763d7f
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/2b86015b-6b6b-44e9-a870-3b16c0763d7f.jsonl
source_lines: 1302-1330
agent_attribution: a586b67326b26d1da
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "OPFS-SQLite GC/coverage/ingest/dump implementation: all required gates passing (wasm, native, doctrine-lint, artifact verify)"
captured_at: 2026-06-26T18:42:27Z
---

OPFS-SQLite GC/coverage/ingest/dump implementation: all required gates passing (wasm, native, doctrine-lint, artifact verify)

---

User: [Agent task result: Agent "PR-5 gc/coverage/ingest/dump" finished]
Let's think about this long-term — the work is done and every gate is green. Here's the summary.

📋 SUMMARY: Shipped PR-5 of #1007 — the write/GC/export half of the wasm OPFS-SQLite engine — as PR #2159 against master.

🔍 ANALYSIS: The engine cannot depend on `nmp-store` (Cargo cycle), so I built crate-local mirror types and inherent methods returning `SqliteWasmError`, exactly as PR-3 did. Two judgment calls earned their keep: SQLite's secondary indexes let me compute `interaction_counts` on-read (no denormalized counter table, no insert-path hook — and it still honours NIP-10 marker precedence, which a flat tag count couldn't), and `wasm32-unknown-unknown` has no usable `Instant`, so GC is bounded purely by the event budget, which is more D7-clean than the LMDB clock-gated loop.

⚡ ACTIONS: Read the full PR-3 engine and the `nmp-store`/LMDB reference implementations; added the PR-5 schema tables in a distinct block; implemented gc, coverage ledger, F-TTL freshness, ingest-log read/trim, interactions, domain rows, `delete_by_filter`, and streaming dump; split files to stay under the LOC cap; ran every required gate.

✅ RESULTS: All gates green — wasm check, native check + 13 unit tests, doctrine-lint (0 findings), `nmp-store --all-features`, release-manifest, sqlite-wasm artifact, and doctrine_lint_smoke (98 passed). Every touched/new file is under 300 LOC.

📊 STATUS: PR open at https://github.com/pablof7z/nostr-multi-platform/pull/2159 (branch `worktree-agent-a586b67326b26d1da`, commit `73fbae142`, base `master`). Working tree scoped to `crates/nmp-sqlite-wasm/`. EventStore trait impl remains deferred.

📁 CAPTURE: The gc⇄coverage backstop lowers the matching ledger row in the **same** `with_txn` as the evicting delete (`coverage::lower_guards_in_txn`), so coverage never outlives its events. Ingest-log `latest_seq` reads `sqlite_sequence` (survives trim); append-time trim reads volatile claims from a table to avoid touching PR-3's `insert.rs`. PR-4 (#2157) landed in parallel; our `lib.rs`/`schema.rs` additions are in disjoint blocks, so a merge-time rebase is mechanical.

➡️ NEXT: Supervisor runs the workspace-wide gate at merge; the follow-up PR is `impl EventStore for OpfsSqliteEventStore` in `nmp-store` (mapping crate-local types + `SqliteWasmError → StoreError`, including the new `Migration` variant → `SchemaTooNew`/`MigrationFailed`).

📖 STORY EXPLANATION:
1. PR-5 adds the write/GC/export side of the wasm SQLite store as inherent methods only — no trait impl, no stubs, per the zero-hacks rule.
2. New schema tables (LRU access, coverage, freshness, domain, retention claims) plus a `reason` column went into one distinct DDL block, conflict-free with PR-4.
3. GC runs three phases — NIP-40 reap, LRU eviction, throttled tombstone purge — each bounded by the event budget since wasm has no monotonic clock to read.
4. The K3 Stage-D3 backstop is the load-bearing correctness piece: an evicted covered event lowers its ledger row atomically, so the ledger can never claim a backfill range it no longer holds.
5. The ingest log gained the four read/claim methods, explicit-Gap scanning, and append-time trim that keeps it bounded right after every write.
6. Interaction counts are computed on-read through the existing tag index — the architecturally right SQLite translation of LMDB's denormalized counter, with identical results.
7. Domain rows, `delete_by_filter`, and a deterministic JSONL dump round out the surface, building on PR-3's tombstone/remove helpers rather than duplicating them.
8. Every required gate passes and PR #2159 is open against master, with the EventStore wrapper honestly deferred to its own PR.

🎯 COMPLETED: PR #2159 shipped — GC, coverage ledger, ingest log, dump; all gates green.

