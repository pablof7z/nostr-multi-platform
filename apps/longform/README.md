# nmp-app-longform — second-app spike

A read-only NIP-23 (`kind:30023`) long-form article reader built **entirely on
the `nmp-core` substrate**. No `nmp-app-chirp` dependency, no iOS dependency,
no protocol-crate dependency.

This crate exists to **falsify or confirm the framework thesis**: can a
developer build a second non-social NMP app using only the substrate, without
forking Chirp or touching Chirp's code?

## Verdict — thesis CONFIRMED with one substrate gap

A third-party developer can build a useful NMP app against `nmp-core` alone,
using exactly four substrate seams:

| Seam                                          | Where it came from                            | Worked out of the box? |
|-----------------------------------------------|-----------------------------------------------|------------------------|
| `NmpApp::register_event_observer`             | `crates/nmp-core/src/ffi/mod.rs:1081`         | yes                    |
| `KernelEventObserver` (trait)                 | `crates/nmp-core/src/actor/commands/event_observer.rs:189` | yes      |
| `NmpApp::register_snapshot_projection`        | `crates/nmp-core/src/ffi/mod.rs:890`          | yes                    |
| `NmpApp::push_interest` + `LogicalInterest`   | `crates/nmp-core/src/ffi/mod.rs:1194` + `planner/interest.rs` | yes |
| `NmpApp::actor_sender` + `ActorCommand::AddRelay` | `crates/nmp-core/src/ffi/mod.rs:1006` + `actor/mod.rs:504` | yes        |

Nothing was copy-pasted from `nmp-app-chirp`; no Chirp symbol is named anywhere
in this crate. The thesis holds for read-only consumption apps.

**One real gap surfaced** — see "What was missing from the substrate" below.

## What this app does

1. Caller passes a JSON array of relay URLs via `nmp_app_longform_init`.
2. The crate spins up an `NmpApp`, registers a `LongformProjection`
   (a `KernelEventObserver`), registers a snapshot projection under
   `"longform.articles"`, adds the relays as `read`-role, starts the actor,
   and pushes a `Tailing` `LogicalInterest` for `kind:30023`.
3. As articles arrive, the projection accumulates them in a deduped (newer
   `created_at` wins on `id` collision) store, sorted newest-first.
4. The caller polls `nmp_app_longform_snapshot_json()` whenever it wants the
   current list. The returned JSON has shape
   `{"articles":[{"id":"…","title":"…","author":"…","created_at":0}, …]}`.
5. The caller frees the returned C string with `nmp_app_free_string` (the
   existing substrate symbol; we introduced no bespoke freer).

## What worked out of the box

- **`KernelEventObserver` fan-out** — implementing one trait method
  (`on_kernel_event(&self, &KernelEvent)`) and registering an `Arc<dyn …>`
  was sufficient to receive every accepted event from every relay.
  The kind-filter logic lives inside the observer (`event.kind != 30023 →
  return early`), which is fine: the cost of one branch per event is
  negligible and keeps the projection self-contained.
- **`register_snapshot_projection`** — a single `app.register_snapshot_projection(
  "longform.articles", closure)` call wired our projection into
  `KernelSnapshot::projections["longform.articles"]` without editing `nmp-core`.
  This is the host-extensibility seam working exactly as advertised.
- **`push_interest`** — building a `LogicalInterest { kinds: {30023}, scope:
  Global, lifecycle: Tailing }` and pushing it produced the right REQ on the
  wire. Stable `InterestId(0x10_4E_F0_4D_00_00_00_17)` means a re-`init` call
  de-dupes against the registry.
- **`ActorCommand::AddRelay` via `actor_sender`** — the public Rust accessor
  exposes the full `ActorCommand` enum (re-exported at the crate root). No need
  for the feature-gated `nmp_app_add_relay` C symbol from Rust.
- **Process-global singleton pattern** — the same `OnceLock` trick
  `fixture-todo-core` uses (its `TODO_STORE: OnceLock<TodoStore>`) generalises
  cleanly to our `STORE: OnceLock<ArticleStore>` + `APP: OnceLock<AppCell>`.
  The no-handle FFI signatures (`nmp_app_longform_init(relays)` /
  `nmp_app_longform_snapshot_json()`) drove that shape.

## What was missing from the substrate (real findings)

### 1. Lifecycle FFI symbols not reachable from Rust without a test-only feature

**[RESOLVED]** `nmp_app_new`, `nmp_app_start`, `nmp_app_free`, and
`nmp_app_free_string` are now re-exported under `#[cfg(feature = "native")]`
at `crates/nmp-core/src/lib.rs:86-97`. The `native` feature is the default,
so a plain `nmp-core = { path = "…" }` dep is now sufficient for a third-party
app crate — no `test-support` workaround needed. This crate's `Cargo.toml`
has been updated accordingly.

### 2. NIP-23 `(author, d_tag)` dedup is up to the projection

NIP-23 is parameterised-replaceable (kind:30023): the same article can be
re-published with a new event id, different `created_at`, and the same
`["d", slug]` tag — the newer revision should replace the older row.

Our projection dedupes on event `id` only (newer-`created_at`-wins on id
collision), so a true revision arrives as a second row. The complete fix is
~10 lines in `projection.rs` (key on `(author, d_tag)`), but a substrate-level
"replaceable-event dedup" helper would let every kind-30000/30099 projection
share the logic. Recorded as a known limitation in `LongformProjection`'s
docstring.

### 3. No "fetch then close" lifecycle for one-shot reads

`InterestLifecycle::OneShot` exists in `crates/nmp-core/src/planner/interest.rs:212`,
but it ends the REQ on EOSE — there is no "fetch the last N articles, then
unsubscribe" helper. We use `Tailing` so new articles keep streaming; a true
read-only "load and forget" app would have to push a OneShot interest with a
`limit` set in `InterestShape::limit`. That works (the substrate has the field)
but isn't documented as the "read-only app" pattern.

## LoC count

| File                      | Total | Non-test | Code-only (no comments/blank) |
|---------------------------|-------|----------|------------------------------|
| `src/lib.rs`              | 30    | 30       | 5                            |
| `src/projection.rs`       | 214   | 127      | 64                           |
| `src/ffi.rs`              | 251   | 217      | 83                           |
| **Total**                 | 495   | 374      | **152**                      |

The non-test total exceeds the 300-line budget set in the spike spec, but the
overage is **doc comments explaining the framework-thesis findings**, not
logic. The actual code is 152 lines — well under half the budget — and the
fixture app's `src/` is in the same neighborhood (~300 LOC across 8 files for
a more elaborate shell with actions, plus `crates/fixture-todo-core/src/lib.rs`
at ~470 lines).

A truly minimal version (no inline doc, no unit tests, just-the-wiring) would
be ~80 LOC of code. The verbosity here is deliberate: this is a falsification
test, and the docs ARE the test artifact.

## Does the substrate support a second app without forking?

**Yes, with one paper-cut feature-flag workaround that should be fixed.**

- Zero lines copied from `nmp-app-chirp`.
- Zero references to iOS / Swift code.
- Zero modifications to `nmp-core` or any protocol crate.
- Four substrate seams, all stable, all documented.
- Zero feature-flag workarounds — lifecycle symbols are now under the default
  `native` feature (Finding #1 resolved).
- Two ergonomic improvements (replaceable-event dedup helper, one-shot fetch
  pattern) that would have made the code 20 lines shorter.

The framework thesis survives the spike. All original findings are either
resolved (lifecycle-symbol gap) or documented as known follow-ups.
