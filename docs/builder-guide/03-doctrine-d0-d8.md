# 03 — Doctrine D0–D10 end-to-end

> **Status: SHIPS.** Audience: both. The doctrine is the framework's identity:
> every API decision answers to at least one of these eleven principles, and
> conflicts resolve **in listed order** (D0 wins over D1, D1 over D2, …).

Canonical text: [`docs/product-spec/doctrine.md`](../product-spec/doctrine.md)
(D0–D10 full statements).

## Two kinds of doctrine

There are **exactly 11 doctrines, D0–D10**. They split into two review classes
(`doctrine.md:7`):

- **D0–D5 + D10 — policy.** Govern user-facing semantics: what the framework
  *promises* and *forbids*. Policy review flags "this API choice violates a
  user-facing principle."
- **D6–D9 — substrate invariants.** Govern how the runtime may be
  *implemented*: what crosses FFI, how state propagates, what the hot path may
  do, how time is decided. Substrate review flags "this implementation will
  leak across FFI / hide policy natively / degrade reactivity / trust a value
  the kernel must own."

Both kinds are equally binding. The split only changes the *kind of review*,
not the binding strength of the rule.

## The 11-row doctrine table

| D | Statement (one line) | What it forbids | Enforced today by | Regression test |
|---|---|---|---|---|
| **D0** | No app nouns in `nmp-core`; protocol/app modules contribute typed variants; test surface gated behind `test-support`. | `Highlight`/`Episode`/`Project`/`Group` types in `nmp-core`; app business logic in Swift/Kotlin/TS; closed FFI enums; exporting `spawn_actor` from prod builds. | ADR-0009; `doctrine-lint` D0 grep gate (`.github/workflows/doctrine-lint.yml`, `crates/nmp-testing/bin/doctrine-lint/`); `nmp-nip29/src/lib.rs:10-16` boundary statement. | `crates/nmp-core/tests/substrate_registry.rs`; `doctrine-lint` D0 gate in CI. |
| **D1** | Best-effort rendering — render now, refine in place. Every view payload field carries a value, not a loading status. | Hiding a post until kind:0 loads; replacing cached metadata with a spinner; `if has_profile { render } else { spinner }`; profile-pic flicker. | View payload **types**: display fields non-`Option`; placeholders are part of the type contract (`Placeholder<T>` newtype, ADR-0017). | `c13_view_payload_uses_placeholders_then_refines_in_place` (`framework_magic_contract/c5_c8_c13.rs`). |
| **D2** | Negentropy first, REQ second. Every `(filter, relay)` is a tracked sync target with a watermark. | Defaulting to REQ scans for historical gaps; treating NIP-77 as an opt-in feature; assuming all relays speak NIP-77. | `nmp-nip77` coverage gate (`coverage_gate::decide_strategy`); per-relay `supports_nip77` probe cache. | `c10_watermark_gates_backfill_and_authoritative_miss` (`framework_magic_contract/c10.rs`). |
| **D3** | Outbox routing is automatic; manual relay selection is the opt-out. | Posts to relays the author hasn't declared; DMs on public relays; hand-rolled fan-out; relay URLs in view-open / send APIs. | Planner Stage-1 outbox (`crates/nmp-core/src/planner/`); `PublishTarget::Auto` → `OutboxResolver` (`publish/mod.rs:12-13`). | `c6_authors_subscription_routes_to_per_author_write_relays` + `c7_publish_routes_outbox_and_private_fails_closed`. |
| **D4** | Single writer per fact; caches derive. One writer, five derived layers; no public cache-invalidation concept. | Two writers for one fact; app-side cache parallel to the store; manual cache invalidation; account switch as tear-down/rebuild. | Per-(event,relay) status owned by the publish engine (`publish/mod.rs:14-15`); insert-path supersession in the store. | `c1`/`c2`/`c3` replaceable supersession; `c12_account_switch_rebinds_views_without_imperative_dance`. |
| **D5** | Snapshots bounded by what's open. FFI carries the projection through open views, never the event store. | Crossing the event store over FFI; `AppState` growing beyond open-view projections; payloads that don't evict on view close. | `AppState = small screen data + map of `ViewId → ViewPayload` for open views only; closing a view evicts its payload. | Architectural (payload struct shape; no single named test — covered by snapshot-shape assertions in view tests). |
| **D6** | Errors never cross FFI as exceptions. Failures surface as `toast` state + `busy` flags. | `Result<T,E>` / exceptions across FFI; `do { try }` / `try {} catch` around framework calls; per-op error-type proliferation; silent failure. | `doctrine-lint` D6 grep gate; `publish::engine::engine_error_to_failure` maps `PublishEngineError` → `RecentFailure` (`publish/mod.rs:18-24`). | `doctrine-lint` D6 gate in CI; publish-engine unit tests in `crates/nmp-core/src/publish/`. |
| **D7** | Capabilities report; never decide policy. Bridges run platform APIs and report raw events. | Native deciding retry / recoverability / which relay / which cipher; capability holding state beyond OS handles; non-idempotent start. | `doctrine-lint` D7 grep gate; `RelayDispatcher` returns descriptive `RelayAck`; `classify_ack` (`publish/state.rs`) is the only ack→policy mapper (`publish/mod.rs:25-28`). | `doctrine-lint` D7 gate in CI; `c7_publish_routes_outbox_and_private_fails_closed` (fail-closed policy is Rust's). |
| **D8** | Reactivity contract: composite reverse index · ≤60 Hz/view · working-set bounded · **no polling at any layer**. | Per-event hot-path allocations after warmup; wakeups ∝ event volume; memory ∝ history depth; emitting when nothing changed; `sleep`+poll loops anywhere in the stack (Rust channels, iOS timers, test helpers). | Composite reverse index in the actor; idle-tick emit gated on `kernel.changed_since_emit()` (`doctrine.md:89`). ADRs 0001–0004. | `crates/nmp-testing/bin/reactivity-bench/` (run 002 passed all gates); idle-tick D8 regression guard. |
| **D9** | The kernel owns time. Signing, replaceable resolution, and NIP-40 expiration are kernel decisions read through the injected `Clock`. | Trusting a relay's word on "newer"/"expired"; reducers reading wall-clock directly (breaks replay). | Injected `Clock` trait — `SystemClock` / `FixedClock` — in `crates/nmp-core/src/kernel/clock.rs`. | `crates/nmp-testing/tests/store_insert_path.rs` (replaceable supersession by `created_at`); `FixedClock`-driven reducer tests; deterministic `kernel/replay.rs`. |
| **D10** | Provenance — private events never escape to public relays; received events are not laundered between relays. | Republishing a privately-delivered event to a public relay; a kind:1059 gift-wrap leaking onto a non-DM relay or a recipient-unknown fallback; cross-relay forwarding as a side effect of caching. | Per-event `ProvenanceEntry` (32-entry LRU) in both store backends (`store/lmdb/provenance.rs`, `store/mem/mod.rs`); kind:1059 gift-wrap routed to explicit DM-relay targets (`nmp-marmot/src/interest.rs`, `projection/publish.rs`); private publish fails closed on unknown inbox (D3 planner). | `crates/nmp-testing/tests/store_provenance_merge.rs`; `c7_publish_routes_outbox_and_private_fails_closed` (`framework_magic_contract/c7_c11.rs`); `m2_p_tag_inbox_routing.rs` (`p_tag_unknown_inbox_fails_closed_no_indexer_fallback`). |

D0 vs everything: when a real app needs a domain noun in `nmp-core`, the
*boundary* is wrong — fix the boundary, do not add the noun (D0 outranks
convenience). When D5 (small snapshot) and D1 (render everything now) appear to
conflict, D1 wins on what is *renderable from the projection*; D5 wins on what
*crosses FFI* — they are orthogonal in practice.

## In-crate doctrine-map comments

Shipping crates carry a `//! Doctrine map:` block in their crate root tying
each touched doctrine to the concrete mechanism that discharges it. The
canonical example is [`crates/nmp-core/src/publish/mod.rs:11-31`](../../crates/nmp-core/src/publish/mod.rs)
(D3/D4/D5/D6/D7/D8). See also
[`crates/nmp-nip77/src/lib.rs:23-34`](../../crates/nmp-nip77/src/lib.rs)
(D2/D6/D8) and the D0 boundary statement at
[`crates/nmp-nip29/src/lib.rs:10-16`](../../crates/nmp-nip29/src/lib.rs).

The framework-magic contract table
([`docs/design/framework-magic.md:24-63`](../design/framework-magic.md)) is the
behavioral cross-index: each C-bullet names the doctrine it discharges and the
test in `crates/nmp-testing/tests/framework_magic_contract.rs` (14 total: C1–C13
+ `contract_surface_complete` meta-test).

### Doctrine-map comment template (paste into a new module's crate root)

```rust
//! `nmp-<name>` — <one-line purpose>.
//!
//! Doctrine map:
//! - D0 (no app nouns in nmp-core): this crate owns <its nouns>; `nmp-core`
//!   gains zero <noun> types. Does NOT import any other `nmp-nip*` crate.
//! - D<n> (<short name>): <the concrete mechanism here that discharges it>,
//!   not <the forbidden shortcut>.
//! - D6 (errors never cross FFI): all public errors are internal `Result`;
//!   mapping to toast/busy happens at the actor / action boundary.
//! - D8 (reactivity): <hot-path bound — alloc/budget/dedup statement>.
//
// One bullet per doctrine the module *touches*. Omit untouched doctrines.
// If a bullet would say "N/A", the module probably shouldn't touch it.
```

## Reusable PR-review rubric

Run this against every PR. Any unchecked box is a blocking finding unless an
ADR explicitly waives it.

```text
DOCTRINE PR REVIEW — D0..D10 (resolve conflicts in listed order)

D0  No new app/domain noun in nmp-core? No app logic in Swift/Kotlin/TS?
    No closed FFI enum blocking module-contributed variants?
    `spawn_actor` (and friends) still test-support gated?
    -> diff `git grep` for new pub types in crates/nmp-core/src/{substrate,…}

D1  Every new view-payload display field non-Option (or Placeholder<T>)?
    No `if missing { spinner }` / `if has_x { render }` gate added?

D2  New (filter,relay) touch goes through the coverage gate, not a raw REQ?
    No "assume relay speaks NIP-77" without the probe cache?

D3  Zero relay URLs in any new view-open / send / publish *app* surface?
    Publishes resolve via OutboxResolver, not a hardcoded constant?

D4  Exactly one writer for each new fact? Downstream state derives only?
    No app-side cache mirroring AppState? Switch is a transition, not rebuild?

D5  New snapshot fields are screen-shaped + open-view-scoped?
    Nothing leaks the event store across FFI? Payload evicts on view close?

D6  No Result<T,E> / exception across FFI? Every failure has an observable
    state field (toast / busy clears / diagnostic record)?

D7  No native code deciding retry / recoverability / relay / cipher / state?
    Capability start/stop/restart idempotent N times? No state beyond OS handle?

D8  No per-event allocation on the hot path after warmup?
    Idle ticks with no change do NOT emit (changed_since_emit gate intact)?
    reactivity-bench still green?

D9  Every new time read goes through the injected Clock, not SystemTime::now()
    on a reducer/replay path? Any new created_at consumer (replaceable,
    expiration, ordering) bounded against the kernel clock, not relay-trusted?

D10 Does any new path forward a received event to a relay other than the one
    that delivered it without explicit user intent? Any kind:1059 gift-wrap
    publish target that is NOT the recipient's DM/inbox relay set?
    Private publish on unknown inbox fails closed (no public fallback)?

GATE: `cargo run -p nmp-testing --bin doctrine-lint` passes (D0/D6/D7 grep
gates); `cargo test --workspace` green; reactivity-bench --fail-on-gate green.
```

## Anti-patterns (never do these)

1. **`Result<T,E>` across FFI.** Map engine errors to `RecentFailure` /
   `toast` state inside Rust (D6). The native side never sees a Rust error.
2. **`AppState` growing beyond the open-view projection.** Snapshots are
   screen-shaped; the event store never crosses FFI (D5).
3. **Per-event hot-path allocations.** Allocations after warmup are linear in
   *active-view count*, never in cached-event count (D8).
4. **Native code deciding retry policy.** Capabilities report; `classify_ack`
   in Rust decides (D7).
5. **Growing `nmp-core` to host app nouns.** If an app needs a domain type in
   the kernel, the boundary is wrong — change the boundary, not the kernel
   (D0).
6. **Ticking a doctrine box without the enforcing test.** A doctrine without a
   regression test or `doctrine-lint` gate is a doctrine waiting to be broken.

## See also

- [02 — Mental model — kernel + extension seams](02-mental-model.md)
- [05 — Kernel substrate — traits + seams](05a-substrate-traits.md)
- [06 — Reactivity contract (D8)](06-reactivity-contract.md)
- [10 — Outbox routing (NIP-65)](10-outbox-routing.md)
- [16 — Capabilities (D7)](16-capabilities.md)
- [22 — Doctrine compliance checklist](22-doctrine-checklist.md)
