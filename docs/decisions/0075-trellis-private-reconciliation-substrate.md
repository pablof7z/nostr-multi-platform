# ADR-0075: Trellis as private reconciliation substrate

## Decision

NMP may use Trellis as private reconciliation machinery below typed read
sessions.

```text
Trellis owns generic mechanics.
NMP owns Nostr and product meaning.
```

Trellis primitives must not become app-facing, native-facing, web-facing, or
builder-guide programming concepts. Public callers keep opening NMP typed
sessions, dispatching NMP typed actions, receiving NMP typed outputs, and
closing NMP handles.

ADR-0075 distinguishes the **app surface** from a **diagnostic surface**:

- **App surface:** production app/native/web APIs, builder docs, examples, and
  product shells. This surface never exposes Trellis vocabulary.
- **Diagnostic surface:** a dev-build-only `nmp-devtools` crate may depend on
  Trellis trace/audit data to answer framework-debugging questions. It is
  tooling, not app API: release builds do not link it, app code does not depend
  on it, and it does not redefine NMP resource or product semantics.

NMP owns resource identity semantics even when Trellis owns resource identity
mechanics. NMP defines which facts make two demands equivalent, which commands
open, replace, or close those demands, how route provenance works, and what host
feedback means.

## Context

Typed sessions need explicit dependency tracking, collection diffs, resource
ownership, scoped teardown, output clear/rebaseline sequencing, and deterministic
replay tests. Trellis provides generic mechanics for those problems. Feed
session acquisition now applies Trellis resource plans as the authoritative
interest mutation source; the old full-recompute path remains only as a
reverse-shadow test oracle.

The risk is public leakage: Trellis must not become a second app lifecycle model
beside NMP typed sessions or a place where Nostr/product meaning is defined.
The diagnostic surface exists to inspect private reconciliation receipts without
relaxing that rule for product code.

## Ownership

Trellis owns graph transactions, dependency identity mechanics, collection diff
mechanics, resource bookkeeping, scope teardown mechanics, output frame
lifecycle mechanics, and trace/oracle hooks.

NMP owns resource key taxonomy, resource command payload semantics, Nostr event
truth, replaceable rules, relay policy, routing, provenance, store/cache
semantics, admission, projection schemas, public APIs, actual I/O, and actor
integration.

## Consequences

NMP can reuse mature reconciliation mechanics while keeping the public API and
Nostr semantics stable. The first production use proved equivalence against the
existing path before promotion; subsequent production adapters must keep a
repo-local oracle or contract test until their bespoke machinery is retired.

NMP may also build dev-only tooling over those mechanics. Diagnostic tools may
read Trellis transactions, traces, and audit records, but their purpose is to
explain NMP-owned facts: which typed read/session, scope, projection owner,
interest, relay, or teardown changed and why. They are not metrics dashboards,
general log aggregation, or a product API for applications.

NMP must maintain explicit resource taxonomy and command types. Arbitrary
Trellis keys at app call sites would move product meaning out of NMP and are not
allowed.

## Boundaries

Permitted:

- private adapters below typed read/session APIs;
- focused internal contract tests and equivalence tests;
- validation helpers in `nmp-testing`;
- dev-build-only diagnostic tooling in `nmp-devtools`;
- private substrate crates after a real slice proves the split.

Forbidden:

- exported Trellis types in app/native/web APIs;
- builder docs that teach apps to assemble Trellis graphs;
- product shells or release builds linking `nmp-devtools`;
- app-owned product identifiers built as raw Trellis string keys;
- Nostr event-kind, relay, projection, signer, privacy, or fallback policy in
  Trellis core;
- deleting existing NMP reactivity code before equivalent behavior is proven.

## Enforcement

Doctrine tests and public API checks reject raw Trellis primitives in
app/native/web-facing NMP surfaces. Builder docs must continue to teach NMP
typed sessions and handles.

The only public-surface exception is the diagnostic surface rooted at
`crates/nmp-devtools/src`. Doctrine gates allow Trellis vocabulary there and
nowhere else in app/native/web-facing Rust APIs or builder-facing docs/examples.
That exception is narrow: it authorizes dev tooling to inspect reconciliation
receipts, not applications to consume Trellis as a lifecycle API.

Equivalence tests must pass before bespoke NMP reconciliation machinery is
deleted or demoted from authority. Feed-session's reverse-shadow tests apply
Trellis deltas and compare the resulting authority state against the old full
recompute. Those tests cover source expansion, source shrink, empty-source
fail-closed behavior, scoped teardown, stale host feedback, output
baseline/delta/rebaseline/clear, and replay.

## Read-layer reconciler adoption (#3115/#3116)

> **Superseded posture, per [ADR-0077](0077-doctrines-are-guardrails-not-dogma.md):**
> #3116 ran under a "default = migrate every hand-rolled reconciler onto
> Trellis" posture, recorded below as the historical record of that sweep.
> That default no longer applies going forward: Trellis remains a supported,
> often-good substrate for a keyed set-difference reconciler, but using it is
> a per-case engineering choice, not a blanket requirement. Hand-rolling a
> keyed open-added/close-removed diff is allowed again when it's the better
> fit for the case at hand. The migrations this section describes
> (`demand_set`, `feed_author_refs`, `nmp-nip17`'s peer-set) are unaffected —
> they stay on Trellis because that's still the right call for them, not
> because a rule compels it.

#3116 audited the read layer under that now-superseded default. That sweep is
recorded here for history:

- **`replace_dependent_interest_set` — deleted, not migrated** (#3119). It was
  `#[allow(dead_code)]` with no production sender; its Trellis-fed twin
  `apply_dependent_interest_delta` was already the live authoritative path.
- **`demand_set` — migrated** (#3121, `nmp-read-session/src/demand_set.rs`).
  `nmp-nip29`'s per-relay discovery reconcile now feeds
  `nmp_core::trellis_reconciler::KeyedReconciler<String, ReadDemand>` the full
  desired member map every call; the shared type-erased reducer identity that
  predates #3116 is unchanged.
- **`feed_author_refs` — migrated** (#3122,
  `nmp-core/src/kernel/feed_author_refs.rs`). The kernel's in-tick,
  `&mut self` auto-resolve reconcile now feeds a persistent
  `KeyedReconciler<(String, String), ()>` a composite `(consumer_id,
  author_key)` key every tick; in-tick execution is preserved exactly (the
  reconcile is still called from `Kernel::make_update`, before typed
  projections are emitted).
- **The reusable core** (`nmp_core::trellis_reconciler::KeyedReconciler<K,
  C>`, factored once during #3121) owns the Trellis `Graph` + diff and
  returns an ordered `Vec<ResourceCommand<C>>`; each caller applies it in
  `Vec` order (LIFO close-vs-close ordering is substrate-guaranteed, so the
  caller only owns applying in order, never re-sorting) and, where the
  reconciler composes with a typed-session executor, drains `resource_plan`
  before `output_frames` (the cross-lane ordering the substrate does not
  impose on the host). The same core underlies both shapes of the new public
  primitive — `nmp_read_session::demand_set` (shape a: one shared
  reducer/output across a keyed member set) and
  `nmp_read_session::KeyedReadCollection` (shape b: N independent per-key
  live resources) — see [ADR-0078](0078-keyed-live-read-collection.md).

**Phase-A ratchet widened.** `nmp-core` already depended on `trellis-core`
(the feed-session adapter); `nmp-read-session` now also depends on it
directly (`demand_set.rs`, `keyed_collection.rs`) to consume
`KeyedReconciler` and to plumb `ResourceCommand`/`ResourceKey` through its own
executor. `trellis-core` is an external leaf dependency with no NMP-graph
edge, so this adds no cycle. The one tradeoff this ADR records (not an
exception): both crates' consumers now carry `trellis-core`'s transitive
compile weight — seen and accepted, consistent with the original feed-session
adoption.

**Family unified (as of the #3116 sweep).** At the close of that sweep, zero
hand-rolled open-added/close-removed reconcilers remained in the read layer:
`demand_set`, `feed_author_refs`, `nmp-nip17`'s `dm_runtime` peer-set, and the
new `KeyedReadCollection` primitive all shared one Trellis-backed core, one
`FullRecomputeCheck` leak oracle, and one apply-in-order executor contract.
That snapshot is not a standing requirement — see the superseded-posture note
above — but nothing here was reverted; a new hand-rolled reconciler
introduced later is a normal engineering choice, not a regression.

**Recorded exception: feed-session's adapter stays off the shared core.**
`nmp-feed-session::FeedSessionTrellisAdapter` (`trellis_adapter.rs`)
re-implements the same demand-input/`map_collection`/open-replace-close
planner shape as `KeyedReconciler::new`, but it additionally fuses a
`materialized_output` node into the SAME scope so a session close emits its
output `Clear` frames and its resource-teardown plan in one atomic Trellis
commit — a capability the minimal `KeyedReconciler` core deliberately does
not express (the core is resource-plan-only; see "What this core does NOT
own" in `trellis_reconciler.rs`). This is a deliberate, documented exception
per [ADR-0077](0077-doctrines-are-guardrails-not-dogma.md)'s
guardrails-not-dogma posture, not an oversight: writing the diff/planner
mechanics twice (once generic in the core, once fused with output emission
in the adapter) is the accepted cost of that atomicity. Growing the core an
optional output-attachment so the adapter could converge onto it is a
possible future refactor, not a current divergence to chase.

## Related

- [ADR-0070](0070-typed-read-sessions.md) - app-visible read sessions.
- [ADR-0076](0076-app-facing-feed-helpers.md) - feed helpers over sessions.
- [ADR-0078](0078-keyed-live-read-collection.md) - the keyed live
  read-collection primitive built on the reusable `KeyedReconciler` core.
- #2626 - Trellis private-substrate epic.
- #2627 - Trellis/NMP boundary.
- #2746 - ADR current-only cleanup.
- #2809 - diagnostic surface amendment.
- #2858 - X-Ray diagnostic surface epic.
- #3115 - keyed live read-collection primitive.
- #3116 - read-layer reconciler consolidation epic (per-reconciler
  migrate-or-justify sweep).
- #3119 - deleted dead `replace_dependent_interest_set`.
- #3121 - `demand_set` migration + reusable `KeyedReconciler` core.
- #3122 - `feed_author_refs` migration (composite key, in-tick preserved).
