# ADR-0075: Trellis is a private reconciliation substrate for NMP sessions

## Status

Current.

## Context

ADR-0070 makes typed read sessions the app-visible owner of product reads:
acquisition demand, replay, admission, output, status, source changes, and close
belong to one session contract. The implementation still contains several
framework-local mechanisms for explicit dependency tracking, collection diffs,
resource ownership, scoped teardown, output clear/rebaseline sequencing, and
deterministic replay tests.

Trellis provides generic reconciliation mechanics that map well onto those
problems. The risk is not semantic fit. The risk is accidentally adding a second
public lifecycle model beside NMP typed sessions, or letting Trellis identifiers
become the place where Nostr/product meaning is defined.

Merged PR #2620 is validation evidence: it proved Trellis contracts can model
NMP-shaped read-session behavior from crates.io-safe `trellis-core` and
`trellis-testing` packages. That PR is not a production migration. It uses
Trellis only in `nmp-testing` dev-dependencies.

## Decision

NMP may use Trellis as a private reconciliation substrate under typed
read/session internals.

```text
Trellis owns generic mechanics.
NMP owns Nostr and product meaning.
```

Trellis primitives must not become app-facing, native-facing, web-facing, or
builder-guide programming concepts. Public callers keep opening NMP typed
sessions, dispatching NMP typed actions, receiving NMP typed outputs, and
closing NMP handles. No public NMP API should expose raw Trellis `Graph`,
`ScopeId`, `ResourceKey`, `ResourcePlan`, `OutputFrame`, transaction, or result
types.

NMP owns resource identity semantics even when Trellis owns resource identity
mechanics. A Trellis resource key can carry a stable opaque identity, but NMP
defines which facts make two demands equivalent, which commands open/replace/
close those demands, how route provenance works, and what host feedback means.

## Ownership Table

| Concern | Owner |
| --- | --- |
| Graph transactions | Trellis |
| Explicit node/dependency identity mechanics | Trellis |
| Collection diff mechanics | Trellis |
| Resource ownership bookkeeping | Trellis |
| Scope teardown mechanics | Trellis |
| Output frame lifecycle mechanics | Trellis |
| Trace/replay/oracle hooks | Trellis |
| Resource key taxonomy and constructors | NMP |
| Resource command payload semantics | NMP |
| Nostr event truth and replaceable rules | NMP |
| Relay policy, routing, and provenance | NMP |
| Store/cache/admission semantics | NMP |
| Projection schemas and typed output contracts | NMP |
| Public app/native/web APIs | NMP |
| Actual I/O and host actor integration | NMP actor/runtime |

## Boundaries

Allowed Trellis usage:

- private adapter modules below typed read/session APIs;
- focused internal contract tests and equivalence tests;
- validation or oracle helpers in `nmp-testing`;
- future private substrate crates only after the boundary earns that split.

Forbidden Trellis usage:

- exported app/native/web API types;
- builder-guide examples that teach apps to assemble Trellis graphs;
- app-owned product identifiers hand-built as Trellis string keys at call sites;
- moving Nostr event-kind, relay, projection, signer, privacy, or fallback
  policy into Trellis core;
- deleting existing NMP reactivity code before an equivalent Trellis-backed path
  is proven against old/new/full-recompute behavior.

## Migration Sequence

Adoption is staged and deletion-driven:

1. Define NMP-owned Trellis resource identity and command semantics.
2. Create the smallest private NMP/Trellis adapter boundary needed by one real
   consumer.
3. Migrate one vertical session path behind unchanged public NMP APIs.
4. Prove old path, Trellis-backed path, and full-recompute oracle agree for
   source expansion, source shrink, empty-source fail-closed behavior, scoped
   teardown, stale host feedback, output baseline/delta/rebaseline/clear, and
   replay.
5. Delete the bespoke NMP reconciliation machinery proven equivalent by that
   slice.
6. Add ratchets preventing Trellis primitive leakage into public surfaces.
7. Choose the next session family only after the first deletion lands.

This order matters. Trellis should reduce permanent duplicate mechanics, not add
another layer that every future read model must understand.

## Consequences

Positive:

- NMP can reuse mature reconciliation mechanics without turning Trellis into the
  app programming model.
- Dynamic source changes, resource sharing, close semantics, and replay tests
  can move toward one generic substrate.
- D4 and D8 get stronger: one owner per fact, explicit dependencies, bounded
  wakeups, and machine-checkable equivalence before deletion.

Negative/tradeoffs:

- The first production slice must build an adapter and equivalence harness
  before it can delete code.
- NMP must maintain explicit resource taxonomy and command types instead of
  letting arbitrary sessions invent Trellis keys.
- Public API ratchets are required because a private dependency can otherwise
  leak through convenience exports.

## Fitness Functions / Enforcement

- `cargo test -p nmp-testing --test doctrine_lint_smoke` must continue to pass
  after Trellis-related changes.
- Public API checks or doctrine tests must reject raw Trellis primitives in
  app/native/web-facing NMP surfaces.
- The first production consumer must preserve existing public session API shape.
- Equivalence tests must pass before deleting any bespoke reactivity machinery.
- Builder docs must continue to teach NMP typed sessions and handles, not raw
  Trellis graph assembly.

## Linked Work

- #2626: Trellis private-substrate epic.
- #2627: this ADR issue.
- #2629: NMP-owned resource identity and command taxonomy.
- #2630: private NMP/Trellis adapter boundary.
- #2631: first real feed-session vertical slice.
- #2632: old/new/full-recompute equivalence tests.
- #2633: deletion of proven-equivalent bespoke reconciliation code.
- #2634: public API leakage ratchets.
- #2635: second session family selection.
- PR #2620 / #2628: crates.io-safe validation proof.
- ADR-0070: typed read sessions own app-visible read lifecycles.
