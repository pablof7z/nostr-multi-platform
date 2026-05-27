# 27 — Doc/code discrepancies (orchestrator queue)

> Status: **SHIPS** · Audience: agents. The running register of places where
> docs/specs claim more than the code on master delivers today. Aggregated
> from the verification pass every writer agent ran while drafting §00–§26
> (each cite checked at master tip). **Most rows are not bugs** — they are
> milestone-not-landed or deliberate scope deferrals. Read the `status` and
> `owner` columns before acting.

## A. Substantive discrepancies (claim > code)

| # | Claim | Evidence (master tip) | Status | Owner | Sev |
|--:|---|---|---|---|---|
| 1 | Outbox routing is automatic by default (D3); planner routes per-author REQs to NIP-65 write relays | Closed by T105 (`167d4bc + 5c5d417 + e74247c + 0849fd2 + fada22b`): `kernel/outbox.rs` resolver, `OutboundMessage::relay_url`, `req_for_relay()`; `maybe_open_timeline` / `author_requests` / `profile_claim_request` now consume `partition_authors_by_write_relays` + `author_write_relays`; A1 `Trigger::Nip65Arrived` re-emits on resolved relays; URL-keyed transport pool `HashMap<String, RelayControl>` dispatches `send_outbound` by `message.relay_url`. Residuals (separate concerns, not D3 follow-feed): `thread.rs:133/154` hydration on bootstrap (R1 — `#e`/id queries need ids→authors lookup), `firehose_requests` profile.rs:187 on bootstrap (R2 — inbox-side, not outbox), `startup_requests` mod.rs:87-117 on bootstrap (correct — these ARE discovery). See `docs/perf/codex-reviews/t105-167d4bc-5c5d417.md`. | DONE (T105) for follow-feed + author + claim + publish on the live wire; R1/R2 follow-ups remain | T105 LANDED; R1 → T-thread-outbox, R2 → T-firehose-inbox | **HIGH (closed)** |
| 2 | `LmdbEventStore` is a working feature-gated skeleton | Every `EventStore` method returns `not_enabled()` (`crates/nmp-core/src/store/lmdb.rs:57-62`); only `open()` is feature-gated and merely creates dirs (`:38-53`). Non-functional past `open()` even with `--features lmdb-backend` | LANDED status correct; "skeleton" undersells incompleteness | M3 phase 2 | MED |
| 3 | UniFFI is the FFI surface (ADR-0010: `#[derive(uniffi::Enum)]`, `bindings/{swift,kotlin,typescript}/`) | `crates/nmp-codegen/src/generate.rs:108-138` emits plain `#[derive(Clone, Debug, PartialEq)]`; no uniffi, no bindings dir. Live master FFI is hand-written raw C JSON (`crates/nmp-core/src/ffi.rs`); the active transport target is FlatBuffers payloads plus UniFFI lifecycle/bindings, not UniFFI hot payload records. | M14 PLANNED; FlatBuffers transport in progress under F-10 | M14 / F-10 | MED |
| 4 | Generated `FfiApp` is the live FFI app | `generate.rs:158` `dispatch()` bumps `rev`, returns `KernelUpdate::Diagnostics` — a stub. Live actor is reached via the separate raw-C FFI in `ffi.rs` | Two distinct surfaces today | M14 | MED |
| 5 | `nmp gen modules` / `nmp init` are CLI commands | ~~No `nmp` binary~~: `nmp` binary ships in `crates/nmp-cli/` (`Cargo.toml [[bin]] name = "nmp"`); `init`/`gen`/`add`/`update` all wired in `main.rs`. Templates at `crates/nmp-cli/templates/`. `nmp init` scaffolds a Rust workspace (not an iOS/Android project). | DONE | — | LOW |
| 6 | iOS path proven by deleted historical app scaffolds | Chirp is the only active iOS product proof. Podcast and Highlighter shells were removed until Chirp is complete. | Current scope | App proofs deferred | LOW |
| 7 | `framework-magic.md:24-72` index marks C2/3/4/9 `[PENDING M3]`, C5/6/8 `[PENDING M2]`, C7/11 `[PENDING M6]`, C12 `[PENDING M8]` | `crates/nmp-testing/tests/framework_magic_contract.rs:1-25` declares all 14 tests active, zero `#[ignore]`; M2/M4/M6/M8 DONE. The `contract_surface_complete` meta-test only checks structural row↔test-name correspondence, not status text — stale `[PENDING]` slipped past CI | Design-doc status text lag | docs maint. | MED |
| 8 | `framework-magic/capabilities.md:45-47` narrates C13 `[PARTIAL]/[PENDING M2/M3]` | C13 behavior test active and un-ignored (`framework_magic_contract/c5_c8_c13.rs:237-238`) | Bullet/test ship; prose chapter lags | docs maint. | LOW |
| 9 | RMP bible / `docs/aim.md:31` models the actor as a `flume` channel + tokio runtime | Shipped actor (`crates/nmp-core/src/actor/mod.rs`, `relay_worker.rs`) uses `std::sync::mpsc` + `std::thread` + blocking tungstenite. No `flume`, no tokio runtime in the kernel path | TEA contract identical; transport primitives differ | aim.md is reference model (no change needed) | LOW |
| 10 | `m8-subscription-lifecycle.md:21` + PLAN echo "ten/10 recompilation triggers"; plan refs `ConnectionPool::send_publish` | `crates/nmp-core/src/subs/trigger.rs:66-67` ships "eleven canonical triggers" (A1–A11); `subs/pool.rs:34-54` has `send`/`deferred_count`/`drain_deferred`/`mark_connected` — no `send_publish` | Source-plan-doc lag; guide written to match shipped code | docs/plan maint. | LOW |

## B. Cite-drift register (fixed in place by writers; recorded for audit)

| Brief cite | Corrected to | Note |
|---|---|---|
| `api-surface.md:193-228` | `:192-229` | §6.5 heading at 192; end at 229. Agreed independently by §00/§16/§24 writers |
| `nmp-nip77/src/lib.rs:25-44` | `:23-34` | doctrine-map block |
| `nmp-nip29/src/lib.rs:11-19` | `:10-16` | D0 boundary statement |
| `publish/mod.rs:1-40` | `:11-31` | doctrine-map block tightened (`:1-40` still valid as module range) |
| `nmp-core/src/lib.rs:1-50` | `:23-56` (`37-56`) | `test-support` gate region |
| `ffi.rs:44-310` | `:44-275` | 275+ is `#[cfg(test-support)]` injection helpers, not the production C FFI |
| `fixture-todo-core/src/lib.rs:1-304` | `:1-305` | trailing-line shift |
| `podcast-core/src/lib.rs:1-30` | `:1-2` | the verbatim D0 boundary comment is only L1–2 |
| `kernel/types.rs` `KernelUpdate` | `:306-326` | 18 top-level fields (briefs implied 16) |
| `framework-magic/replaceable.md`, `lmdb-schema.md:229-238` | — | These design docs line-cite `kernel/ingest.rs:NNN`; `kernel/ingest` is now a **directory** (`ingest/mod.rs`). Stale cites in upstream design docs (not builder-guide) — flagged for a docs sweep |

## C. Doc/doc reconciliation follow-ups

The former builder-guide `PLAN.md` was a writer-dispatch artifact and has been
deleted. The numbered section files are now the source of truth for this guide.
Future guide-wide reconciliation work belongs in this section or
`docs/BACKLOG.md`, not in a parallel plan file.

## Anti-patterns

- **Treating every row as a bug.** Rows 3–6 are milestone-not-landed; row 9 is a reference-model vs implementation distinction. Filing fix-its against them wastes cycles and risks scope creep beyond the owning milestone.
- **Silently editing the spec to match incomplete code.** Row 1's fix is to *wire the planner into the REQ path* (M2/M8-subs work), not to delete the D3 "outbox automatic" claim from the spec.
- **Silently expanding code beyond milestone scope** to "close" a row (e.g. implementing LMDB now to clear row 2 — that is M3 phase 2, not opportunistic).
- **Citing a corrected (§B) line range from memory** without re-reading at the current tip — master advances continuously; ranges drift again.
- **Promoting a `pub(crate)` symbol** (`RelayRole`, `subs::*` internals) to public API to make an example compile — cite the public re-export instead.

## Deliverables

1. The §A 5-column register (claim · evidence · status · owner · severity) — the orchestrator triage queue; HIGH/MED rows become milestone tasks, not code markers.
2. The §B cite-drift table — an audit trail proving every builder-guide `path:line` was verified at master tip, with corrections applied in place.
3. The §C note — the builder-guide dispatch plan was removed so guide state does
   not live in two places.

See also: [00 — How to read this guide](00-how-to-read.md) · [03 — Doctrine D0–D10 end-to-end](03-doctrine-d0-d8.md) · [07 — Subscription planner](07-subscription-planner.md) · [09 — Persistence (LMDB) + watermarks](09-persistence-lmdb.md) · [10 — Outbox routing (NIP-65)](10-outbox-routing.md) · [15 — Codegen — `nmp gen modules` + per-app FFI crate](15-codegen-and-ffi.md) · [21 — The framework-magic contract](21-framework-magic.md) · [22 — Doctrine compliance checklist](22-doctrine-checklist.md). This register is cross-cut by every section.
