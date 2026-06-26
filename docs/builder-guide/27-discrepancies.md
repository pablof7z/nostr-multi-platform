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
| 2 | Older §09 text said `LmdbEventStore` was only a feature-gated skeleton | Current code lives in `crates/nmp-store`: feature-off `LmdbEventStore::open()` returns an explicit feature error; feature-on `lmdb/open.rs` opens a shared env + NMP sub-dbs and `lmdb/store_impl.rs` implements the real `EventStore` methods. | DONE — §09/FAQ corrected to current feature-gated implementation | docs maint. | MED |
| 3 | UniFFI is the FFI surface (ADR-0010: `#[derive(uniffi::Enum)]`, `bindings/{swift,kotlin,typescript}/`) | Live master uses `crates/nmp-ffi` for the raw C/JNI lifecycle/action/capability ABI and FlatBuffers `UpdateFrame` for hot update payloads. `crates/nmp-codegen/src/main.rs` retains host/runtime emitters (`gen swift`, `gen typed-decoders`, `gen projection-cache`, `gen builtin-keys`); the old generated Rust module path is gone. UniFFI remains the M14 lifecycle/binding target, not the payload format. | M14 PLANNED; update transport is FlatBuffers-only today | M14 | MED |
| 4 | Generated `FfiApp` is a stub | The generated `FfiApp` and the `nmp gen modules` scaffolder were deleted by ADR-0046. A generated `FfiApp` never called `register_defaults` and produced a non-functional Nostr app. Composition is now a library call inside the app-core composition root: downstream shells call `<app>_core::register(app)`, and that function calls `nmp_defaults::register_defaults(app)` once. | DONE — deleted by ADR-0046 / #1114 | — | LOW |
| 5 | `nmp gen modules` / `nmp init` are CLI commands | `nmp` binary ships in `crates/nmp-cli/` (`Cargo.toml [[bin]] name = "nmp"`). `nmp init` is wired and scaffolds a thin Rust shell (`<name>-core` + `examples/shell.rs`). `nmp gen modules` was deleted by ADR-0046; `gen swift` / `gen typed-decoders` remain live CI-gated emitters. | DONE | — | LOW |
| 6 | iOS path proven by deleted historical app scaffolds | Chirp is the only active iOS product proof. Podcast and Highlighter shells were removed until Chirp is complete. | Current scope | App proofs deferred | LOW |
| 7 | `framework-magic.md:24-72` index marks C2/3/4/9 `[PENDING M3]`, C5/6/8 `[PENDING M2]`, C7/11 `[PENDING M6]`, C12 `[PENDING M8]` | `crates/nmp-testing/tests/framework_magic_contract.rs:1-25` declares all 14 tests active, zero `#[ignore]`; M2/M4/M6/M8 DONE. The `contract_surface_complete` meta-test only checks structural row↔test-name correspondence, not status text — stale `[PENDING]` slipped past CI | Design-doc status text lag | docs maint. | MED |
| 8 | `framework-magic/capabilities.md:45-47` narrates C13 `[PARTIAL]/[PENDING M2/M3]` | C13 behavior test active and un-ignored (`framework_magic_contract/c5_c8_c13.rs:237-238`) | Bullet/test ship; prose chapter lags | docs maint. | LOW |
| 9 | `docs/aim.md:31` models the actor as a `flume` channel + tokio runtime | Shipped actor (`crates/nmp-core/src/actor/mod.rs`, `relay_worker.rs`) uses `std::sync::mpsc` + `std::thread` + blocking tungstenite. No `flume`, no tokio runtime in the kernel path | TEA contract identical; transport primitives differ | aim.md is reference model (no change needed) | LOW |
| 10 | `m8-subscription-lifecycle.md:21` + PLAN echo "ten/10 recompilation triggers"; plan refs `ConnectionPool::send_publish` | `crates/nmp-core/src/subs/trigger.rs:66-67` ships "eleven canonical triggers" (A1–A11); `subs/pool.rs:34-54` has `send`/`deferred_count`/`drain_deferred`/`mark_connected` — no `send_publish` | Source-plan-doc lag; guide written to match shipped code | docs/plan maint. | LOW |
| 11 | §02/§05a/§05b/§19a/§19b/§20 describe a 5-trait-family extension architecture: `DomainModule`, `ViewModule`, `IdentityModule`, `ModuleRegistry`, and a step-machine `ActionModule` with `reduce()`/`ActionPlan`/`ActionTransition` | `crates/nmp-core/src/substrate/mod.rs` states explicitly: *"Two further v2 traits — `ViewModule` and `IdentityModule` — were removed… no `ViewRegistry` or identity-dispatch runtime ever shipped. A previous iteration shipped a `ModuleRegistry` that… only collected `(namespace, family, type_name)` strings — nothing in the kernel, the actor, or codegen ever read them back. It has been removed; it was documentation theater."* `DomainModule` is not in `substrate/`. The real `ActionModule` (`substrate/action.rs:56`) has `fn start() -> Result<(), ActionRejection>` + `fn execute(&self, action, correlation_id, send)` — no `Step`, no `Output`, no `reduce`, no `ActionPlan`, no `ActionTransition`. The v1 extension model uses: `register_action(module)` + `register_snapshot_projection()` + `register_live_event_tap()` for live-only read models or `open_observed_projection()` for hydrating scoped read models on `NmpApp`. | DONE — guide sections corrected to current v1 seams | docs maint. | **HIGH** |
| 12 | §05b annotated `fixture-todo-core` walkthrough (5-family impl, `ModuleRegistry`, `ViewDependencies::default()` as ViewModule method, step-machine ActionModule) | `apps/fixture/` and `fixture-todo-core` were deleted by ADR-0046. §05b now uses `microblog-core` as the annotated walkthrough: `NoteActionModule` implementing the real `ActionModule`, app-owned `Arc<Mutex<Vec<NoteRecord>>>` store, `register()` fn calling `app.register_action(NoteActionModule)` + `app.register_live_event_tap(...)` + `app.register_snapshot_projection(FEED_SNAPSHOT_KEY, closure)`. No `DomainModule`, no `ViewModule`, no `ModuleRegistry`. | DONE — §05b rewritten for `microblog-core` | docs maint. | **HIGH** |
| 13 | §19a/§19b microblog walkthrough: code blocks using `DomainModule`, `ViewModule`, `DomainRegistry`, `ModuleRegistry`, `ActionPlan`, `ActionTransition` | None of these types exist in `nmp-core`. The walkthrough now uses the real v1 `ActionModule` (`start` + `execute`) and the read seams (`register_action`, `register_live_event_tap` or `open_observed_projection`, `register_snapshot_projection`). | DONE — §19a/§19b corrected | docs maint. | **HIGH** |
| 14 | §20 protocol module recipe: `register_domain()`, `register_view()`, `register_all()`, claims nmp-nip29 has `domain/mod.rs` + `view/mod.rs` with `register_all()` | `crates/nmp-nip29/src/` contains: `action/`, `cache/`, `group_id.rs`, `interest.rs`, `kinds.rs`, `lib.rs`, `projection/`, `register.rs`. No `domain/` or `view/` directories. Registration is `register_actions(app: &mut NmpApp)` calling `app.register_action(ModuleValue)` per action module. | DONE — §20 corrected | docs maint. | **HIGH** |
| 15 | §15 says codegen generates a per-app FFI crate; §23 glossary cites `ViewModule — substrate/view.rs:37`, `ModuleRegistry — substrate/mod.rs:38` as existing types | The `nmp gen modules` per-app FFI generator and `apps/fixture` were deleted by ADR-0046. `nmp-codegen` still emits host bindings (`gen swift`, `gen typed-decoders`). `ViewModule` and `ModuleRegistry` were removed. | DONE — §15 rewritten as "bindings + FFI surface"; §23 not in builder-guide scope here | docs maint. | MED |

| 16 | §13 cites `nmp-nip77/src/run_sync.rs` (ActionPlan/ActionInput step machine) and `capability_domain.rs` | Neither file exists in `crates/nmp-nip77/src/` (which has: `codec.rs`, `filter.rs`, `lib.rs`, `messages.rs`, `reconciler.rs`, `runtime.rs`). The §13 code block shows the old step-machine `ActionPlan`/`ActionInput` API that was removed. | FIXME — §13 needs audit against current nmp-nip77 source | docs maint. | MED |

## B. Cite-drift register (fixed in place by writers; recorded for audit)

| Brief cite | Corrected to | Note |
|---|---|---|
| `api-surface.md:193-228` | `:192-229` | §6.5 heading at 192; end at 229. Agreed independently by §00/§16/§24 writers |
| `nmp-nip77/src/lib.rs:25-44` | `:23-34` | doctrine-map block |
| `nmp-nip29/src/lib.rs:11-19` | `:10-16` | D0 boundary statement |
| `publish/mod.rs:1-40` | `:11-31` | doctrine-map block tightened (`:1-40` still valid as module range) |
| `nmp-core/src/lib.rs:1-50` | `:23-56` (`37-56`) | `test-support` gate region |
| `ffi.rs:44-310` | `:44-275` | 275+ is `#[cfg(test-support)]` injection helpers, not the production C FFI |
| `microblog-core/src/lib.rs` | see [19a](19a-walkthrough-microblog.md) | `fixture-todo-core` deleted; microblog walkthrough is the canonical reference |
| `podcast-core/src/lib.rs:1-30` | `:1-2` | the verbatim D0 boundary comment is only L1–2 |
| `kernel/types.rs` `KernelUpdate` | `:306-326` | 18 top-level fields (briefs implied 16) |
| `framework-magic/replaceable.md`, `lmdb-schema.md:229-238` | — | These design docs line-cite `kernel/ingest.rs:NNN`; `kernel/ingest` is now a **directory** (`ingest/mod.rs`). Stale cites in upstream design docs (not builder-guide) — flagged for a docs sweep |

## C. Doc/doc reconciliation follow-ups

The former builder-guide `PLAN.md` was a writer-dispatch artifact and has been
deleted. The numbered section files are now the source of truth for this guide.
Future guide-wide reconciliation work belongs in this section or
GitHub Issues, not in a parallel plan file.

## Anti-patterns

- **Treating every row as a bug.** Rows marked DONE are historical guardrails;
  row 3 is milestone-not-landed, and row 9 is a reference-model vs implementation
  distinction. Filing fix-its against those wastes cycles and risks scope creep
  beyond the owning milestone.
- **Silently editing the spec to match incomplete code.** Row 1's residuals are
  scoped follow-ups, not a reason to delete the D3 "outbox automatic" claim from
  the spec.
- **Silently expanding code beyond milestone scope** to "close" a row (e.g. implementing UniFFI now to clear row 3 — that is M14, not opportunistic).
- **Citing a corrected (§B) line range from memory** without re-reading at the current tip — master advances continuously; ranges drift again.
- **Promoting a `pub(crate)` symbol** (`RelayRole`, `subs::*` internals) to public API to make an example compile — cite the public re-export instead.

## Deliverables

1. The §A 5-column register (claim · evidence · status · owner · severity) — the orchestrator triage queue; HIGH/MED rows become milestone tasks, not code markers.
2. The §B cite-drift table — an audit trail proving every builder-guide `path:line` was verified at master tip, with corrections applied in place.
3. The §C note — the builder-guide dispatch plan was removed so guide state does
   not live in two places.

See also: [00 — How to read this guide](00-how-to-read.md) · [03 — Doctrine D0–D10 end-to-end](03-doctrine-d0-d8.md) · [07 — Subscription planner](07-subscription-planner.md) · [09 — Persistence (LMDB) + watermarks](09-persistence-lmdb.md) · [10 — Outbox routing (NIP-65)](10-outbox-routing.md) · [15 — Codegen: bindings + FFI surface](15-codegen-and-ffi.md) · [21 — The framework-magic contract](21-framework-magic.md) · [22 — Doctrine compliance checklist](22-doctrine-checklist.md). This register is cross-cut by every section.
