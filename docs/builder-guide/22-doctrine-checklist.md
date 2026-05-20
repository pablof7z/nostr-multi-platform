# 22 — Doctrine compliance checklist

> **Status: SHIPS.** Audience: agents (and human reviewers). This is the
> operational checklist; the *why* lives in
> [03 — Doctrine D0–D10 end-to-end](03-doctrine-d0-d8.md). Canon:
> [`docs/product-spec/doctrine.md`](../product-spec/doctrine.md),
> [`docs/product-spec/overview-and-dx.md`](../product-spec/overview-and-dx.md)
> §1.5. Resolve conflicts in listed order (D0 > D1 > … > D10).

## How this is consumed

Every PR self-asserts against this list. After each merge to master, a codex
review runs and records findings in
[`docs/perf/codex-reviews/`](../perf/codex-reviews/) (cadence tracked in
[`docs/perf/orchestration-log.md`](../perf/orchestration-log.md)). The
machine-enforced subset runs in CI:
`cargo run -p nmp-testing --bin doctrine-lint` (D0/D6/D7 grep gates,
`.github/workflows/doctrine-lint.yml`), plus the gates in
[`docs/plan/ci-hygiene.md`](../plan/ci-hygiene.md)
(`cargo test --workspace`, `reactivity-bench --fail-on-gate`). File-size
ceiling (`AGENTS.md`: ≤300 LOC) is its own gate (`file-size-gate.yml`).

## The checklist (≥1 item per doctrine)

Copy into the PR description. Every box must be checked or have an inline ADR
reference waiving it.

**D0 — kernel/extension boundary**

- [ ] No new app/domain noun (`Highlight`/`Episode`/`Group`/`Project`/…) added to `crates/nmp-core`.
- [ ] No app-specific business logic added to Swift / Kotlin / TS shell code.
- [ ] FFI enums stay open to module-contributed variants (no closed-enum regression).
- [ ] `spawn_actor` and other test surface still `#[cfg(any(test, feature = "test-support"))]` gated.
- [ ] `cargo run -p nmp-testing --bin doctrine-lint` D0 gate passes.

**D1 — best-effort rendering**

- [ ] Every new view-payload display field is non-`Option` (or `Placeholder<T>`, ADR-0017).
- [ ] No `if has_x { render } else { spinner }` / `if missing { hide }` gate introduced.
- [ ] Late-arriving authoritative data updates the payload in place (no flicker / no re-fetch gate).

**D2 — negentropy first, REQ second**

- [ ] New `(filter, relay)` touches go through the coverage gate, not a raw REQ scan.
- [ ] NIP-77 support is probed + cached per relay, never assumed.

**D3 — outbox routing automatic**

- [ ] No relay URL appears in any app-facing view-open / send / publish surface.
- [ ] Publishes resolve via `OutboxResolver` / planner Stage-1, not a hardcoded constant.
- [ ] Private (gift-wrap) events fail closed when recipient inbox is unknown.

**D4 — single writer per fact**

- [ ] Exactly one writer for each new fact; all downstream state derives mechanically.
- [ ] No app-side cache parallel to `AppState` / the event store.
- [ ] Account switch is a state transition, not a tear-down/rebuild.

**D5 — snapshots bounded by what's open**

- [ ] New snapshot fields are small + screen-shaped + scoped to open views.
- [ ] The event store never crosses FFI; closing a view evicts its payload.

**D6 — no errors across FFI**

- [ ] No `Result<T,E>` / exception crosses the FFI boundary.
- [ ] Every failure has ≥1 observable state field (toast / `busy` clears / diagnostic record).
- [ ] `doctrine-lint` D6 gate passes.

**D7 — capabilities report, never decide**

- [ ] No native code decides retry / recoverability / relay / cipher / resulting state.
- [ ] Capability start/stop/restart is idempotent N times; no state beyond OS handles.
- [ ] `doctrine-lint` D7 gate passes.

**D8 — reactivity contract**

- [ ] No per-event allocation on the hot path after warmup.
- [ ] Idle ticks with no state change do **not** emit (`changed_since_emit()` guard intact).
- [ ] `reactivity-bench --fail-on-gate` is green; no view exceeds 60 Hz.

**D9 — kernel owns time**

- [ ] Every new "now" read goes through the injected `Clock`, never a raw `SystemTime::now()` on a reducer / replay path.
- [ ] Any new `created_at` consumer (replaceable resolution, NIP-40 expiration, ordering) is bounded against the kernel clock, never relay-trusted.
- [ ] Future-dated inbound events are still rejected at the all-kinds chokepoint (`MAX_FUTURE_SECONDS` gate intact).

**D10 — provenance**

- [ ] No new path forwards a received event to a relay other than the one that delivered it without explicit user intent.
- [ ] Every kind:1059 gift-wrap publish targets the recipient's DM/inbox relays only — never a public or recipient-unknown fallback set.
- [ ] Private (gift-wrap) publish fails closed on unknown recipient inbox.

**Cross-cutting**

- [ ] Every changed module's `//! Doctrine map:` comment still accurate.
- [ ] File-size gate green (≤300 LOC per `AGENTS.md`).
- [ ] `cargo test --workspace` green (incl. `framework_magic_contract`).

## Per-doctrine red flags (what reviewers actually catch)

**D0.** A `pub struct`/`pub enum` named after a product concept landing in
`crates/nmp-core/src/{substrate,store,planner}`. A `match` in shell code
branching on Nostr kinds. A new closed FFI enum. The most common real
violation: a kernel change "to make app X work" — that is D0's exact failure
mode and outranks every other consideration.

**D1.** A new payload field typed `Option<String>` for something a placeholder
could fill (display name, picture, timestamp). Any conditional that *withholds*
already-renderable content waiting for a fetch.

**D2.** A new code path issuing `REQ` for historical backfill without first
consulting the coverage gate. Hardcoding `supports_nip77 = true`.

**D3.** Any function signature taking `relays: Vec<RelayUrl>` on an app-facing
send/publish/view API. DM/gift-wrap paths that fall back to public relays
instead of failing closed.

**D4.** Two code sites mutating the same fact. A SwiftData/Room store mirroring
`AppState`. An account-switch implementation that destroys and rebuilds view
handles instead of re-resolving.

**D5.** A snapshot field carrying unbounded history. Any path that hands the
event store (or an iterator over it) across FFI. A payload that survives its
view's close.

**D6.** `do { try … } catch` / `try { } catch` around a framework call in
shell code (means a `Result`/exception escaped). A new per-operation error enum
plumbed through UniFFI. A failure with no observable consequence.

**D7.** Native code with an `if shouldRetry`/`if recoverable` branch, or
picking a relay, or holding cached state past an OS handle's lifetime. A
capability `start()` that breaks when called twice.

**D8.** Allocation inside `on_event_inserted` / the insert hot path. An emit on
an idle tick with no change. A reverse-index keyed on a broad single axis
forcing table scans.

**D9.** A `SystemTime::now()` call inside a reducer or any code on the replay
path (breaks deterministic replay). A new `created_at` comparison that trusts
the relay's value as a bound. A replaceable-resolution or expiration decision
made anywhere but the kernel against its `Clock`.

**D10.** A publish path that targets a relay other than the one that delivered
the event (without explicit user intent). A kind:1059 gift-wrap whose publish
target is a public relay, an indexer, or any recipient-unknown fallback. A
private-event publish that falls back to public relays instead of failing
closed when the recipient inbox is unknown.

## Doctrine-map comment — minimum fields

The full template lives in
[03 — Doctrine D0–D8](03-doctrine-d0-d8.md#in-crate-doctrine-map-comments).
Every new crate / major module root must carry at minimum:

```rust
//! Doctrine map:
//! - D0: this module owns <its nouns>; nmp-core gains zero <noun> types.
//! - D<n>: <mechanism that discharges it>, not <forbidden shortcut>.
//! - D6: public errors are internal Result; toast/busy mapping at the boundary.
//! - D8: <hot-path bound statement>.
```

One bullet per doctrine the module *touches* — omit untouched ones. Canonical
in-tree example: [`crates/nmp-core/src/publish/mod.rs:11-31`](../../crates/nmp-core/src/publish/mod.rs).

## Anti-patterns (checklist abuse)

1. **Ticking boxes mechanically.** A checked box with no enforcing test/lint is
   a lie. If there is no gate, say so and add one or file the gap in [27].
2. **Skipping D8 because "perf is fine in dev."** D8 is a contract, not a
   vibe. The bench gate is the arbiter, not your laptop.
3. **Silent doctrine waivers.** A waiver without an ADR is a violation. Inline
   `// waived` is not a waiver.
4. **"A future PR will fix it" carve-outs.** Doctrine is enforced per-PR; a
   future PR is not a gate.
5. **PRs that grow `nmp-core` to make app X work.** This is D0's signature
   failure. The boundary is the bug, not the missing noun.

## When in doubt, file an ADR

Doctrine is deliberately strict; the escape hatch is *not* a silent exception,
it is an **ADR**. If a change seems to require violating a doctrine, stop and
write `docs/decisions/00NN-*.md` stating the doctrine, the tension, the chosen
resolution, and the new invariant that replaces the broken one (see
[27 — Doc/code discrepancies](27-discrepancies.md) for logging unresolved
drift). `relay_pin` (third routing lane, ADR-0012) is the worked example of a
kernel-substrate change that survived D0 *because* it added a generic
mechanism, not an app noun. No ADR → no waiver → the box stays unchecked.

## See also

- [03 — Doctrine D0–D8 end-to-end](03-doctrine-d0-d8.md)
- [05 — Kernel substrate — the 5 trait families](05-substrate-traits.md)
- [18 — Testing — `nmp-testing`, benches, contract tests](18-testing.md)
- [27 — Doc/code discrepancies (orchestrator queue)](27-discrepancies.md)
