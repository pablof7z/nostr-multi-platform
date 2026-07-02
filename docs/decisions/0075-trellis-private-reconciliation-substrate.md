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

NMP owns resource identity semantics even when Trellis owns resource identity
mechanics. NMP defines which facts make two demands equivalent, which commands
open, replace, or close those demands, how route provenance works, and what host
feedback means.

## Context

Typed sessions need explicit dependency tracking, collection diffs, resource
ownership, scoped teardown, output clear/rebaseline sequencing, and deterministic
replay tests. Trellis provides generic mechanics for those problems.

The risk is public leakage: Trellis must not become a second app lifecycle model
beside NMP typed sessions or a place where Nostr/product meaning is defined.

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
Nostr semantics stable. The first production use must prove equivalence against
the existing path before deleting bespoke machinery.

NMP must maintain explicit resource taxonomy and command types. Arbitrary
Trellis keys at app call sites would move product meaning out of NMP and are not
allowed.

## Boundaries

Permitted:

- private adapters below typed read/session APIs;
- focused internal contract tests and equivalence tests;
- validation helpers in `nmp-testing`;
- private substrate crates after a real slice proves the split.

Forbidden:

- exported Trellis types in app/native/web APIs;
- builder docs that teach apps to assemble Trellis graphs;
- app-owned product identifiers built as raw Trellis string keys;
- Nostr event-kind, relay, projection, signer, privacy, or fallback policy in
  Trellis core;
- deleting existing NMP reactivity code before equivalent behavior is proven.

## Enforcement

Doctrine tests and public API checks reject raw Trellis primitives in
app/native/web-facing NMP surfaces. Builder docs must continue to teach NMP
typed sessions and handles.

Equivalence tests must pass before bespoke NMP reconciliation machinery is
deleted. Those tests cover source expansion, source shrink, empty-source
fail-closed behavior, scoped teardown, stale host feedback, output
baseline/delta/rebaseline/clear, and replay.

## Related

- [ADR-0070](0070-typed-read-sessions.md) - app-visible read sessions.
- [ADR-0076](0076-app-facing-feed-helpers.md) - feed helpers over sessions.
- #2626 - Trellis private-substrate epic.
- #2627 - Trellis/NMP boundary.
- #2746 - ADR current-only cleanup.
