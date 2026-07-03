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

## Related

- [ADR-0070](0070-typed-read-sessions.md) - app-visible read sessions.
- [ADR-0076](0076-app-facing-feed-helpers.md) - feed helpers over sessions.
- #2626 - Trellis private-substrate epic.
- #2627 - Trellis/NMP boundary.
- #2746 - ADR current-only cleanup.
- #2809 - diagnostic surface amendment.
- #2858 - X-Ray diagnostic surface epic.
