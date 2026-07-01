# Architecture Decision Records

This directory holds NMP's live architecture decisions. ADRs preserve context,
but they are not immutable monuments: when current architecture changes, the ADR
that owns the concept must be edited, folded, or retired so it does not teach a
competing rule.

## Current Spine

ADR-0069 through ADR-0073 are the active redesign spine for #2313, #2316, and
#2320:

- **0069**: explicit feature composition and app-owned product policy.
- **0070**: typed read sessions, with `open_interest`, `ObservedProjection`, and
  source reconciliation treated as private/substrate machinery.
- **0071**: publish intents, composable event drafts, signing/publishing
  separation, and route provenance.
- **0072**: runtime, capability, and shell boundaries.
- **0073**: ADR reset discipline and rolling architecture ratchets.

ADR-0075 and ADR-0076 extend ADR-0070 without competing with it: Trellis may
provide private reconciliation mechanics below typed read sessions, and
app-facing feed APIs are feed-shaped helpers over typed read sessions. NMP
continues to own all public APIs and Nostr/resource semantics.

Older ADRs remain authoritative only where they do not conflict with that spine.
If an older ADR teaches a superseded public API, read its status line and current
disposition first.

## How The Spine Fits Together

The intended developer model is:

```text
app Rust crate declares installed features and product policy
  -> shell opens typed read sessions and dispatches typed actions
  -> NMP acquires, stores, projects, signs, routes, publishes, and reports status
  -> shell renders typed output and executes raw capabilities only
```

An app developer should see product-shaped APIs: "open this article feed",
"open this group", "reply to this event", "publish this article to this group".
They should not have to assemble relay interests, observed sinks, projection
sidecars, replay order, dynamic source replacement, snapshot ticks, signer
parking, route planning, or teardown by hand.

Internally, those lower-level pieces are still allowed when justified:

- `open_interest` is acquisition machinery, not a production screen API.
- `ObservedProjection` / `ObservedProjectionSink` are scoped event-delivery and
  replay machinery behind typed read sessions.
- `ReducedSource` names dynamic source reconciliation behind a session, not a
  concept every app author must learn.
- projection emission and incremental frames are output mechanics, not separate
  app subscriptions.
- publish planning, signer capabilities, and relay provenance are one
  actor-owned write workflow, not shell policy.

This means the right simplification metric is not "delete every internal
mechanism". Some complexity is paying for real Nostr correctness. The target is
fewer public doors, fewer duplicate lifecycle recipes, fewer shell policy sites,
and fewer permanent concepts an app author must understand.

ADR-0069..0073 are the decision spine: they carry rationale, migration history,
and the "why" behind each constraint. They are not the fast developer
architecture guide.

For the canonical developer-facing overview of the clean-break architecture —
how an app is structured, how data flows, and what the public surface looks like
— see
[`docs/architecture/high-level-app-architecture.md`](../architecture/high-level-app-architecture.md).
That document is the narrative companion to this ADR spine.

## Status Terms

- **Current**: still owns a live invariant.
- **Proposed**: records a ratified direction or target shape whose public API
  migration is not complete yet.
- **Amended**: still useful, but current guidance is narrowed by the listed ADR.
- **Folded**: the surviving rule is now owned by a later ADR or durable doc.
- **Retired**: historical or superseded; do not use as current architecture.

## Classification

| ADR | Status | Current owner / note |
|---|---|---|
| 0001 | Current | Composite dependency keys remain reverse-index machinery. |
| 0002 | Folded | Per-view delta budgeting belongs in D8/perf-reactivity docs. |
| 0003 | Current | Working-set memory remains bounded by active demand. |
| 0004 | Folded | Allocation measurement belongs in perf/testing docs. |
| 0005 | Folded | Platform shadows survive as transport/cache machinery under ADR-0070. |
| 0006 | Retired | Phase-1 delivery positioning; historical only. |
| 0007 | Current | Diagnostics/non-Nostr bridge data remains a valid boundary. |
| 0008 | Retired | Initial Chirp demo target; historical only. |
| 0009 | Amended | App/kernel boundary survives; production composition is ADR-0069. |
| 0010 | Folded | Runtime registration is governed by ADR-0069 through ADR-0072. |
| 0011 | Current | NMP owns LMDB environment injection. |
| 0012 | Current | Relay pin/third routing lane survives as route provenance input. |
| 0013 | Current | NIP-29 metadata-signer trust model survives. |
| 0014 | Current | LMDB write-path policy survives. |
| 0015 | Current | Signer crate/session boundary survives. |
| 0016 | Retired | Old F-TTL ABI history unless cited from current `ffi-surface`. |
| 0017 | Folded | Raw absent-fact rule belongs under ADR-0032/doctrine. |
| 0018 | Current | ContentTree wire projection survives as typed output. |
| 0019 | Current | Failed NIP-42 AUTH remains fail-closed. |
| 0020 | Current | Intent-classed routing/search survives. |
| 0021 | Current | Relay roles survive unless replaced by a later relay ADR. |
| 0022 | Current | NMP-owned relay transport survives. |
| 0023 | Folded | Capability execution is now governed by ADR-0072. |
| 0024 | Folded | Async capability protocol is now governed by ADR-0072. |
| 0025 | Amended | Marmot exception is narrowed by ADR-0039/0070. |
| 0026 | Current | Signer NIP-44 seam survives; extended by 0050/0066. |
| 0027 | Current | Unified `ActionModule` trait survives. |
| 0028 | Folded | Liveness probe is FFI/diagnostic surface detail. |
| 0029 | Current | Actor queue observability/backpressure survives. |
| 0030 | Current | UniFFI native binding plus FlatBuffers byte transport remains binding guidance, amended by ADR-0072. |
| 0031 | Retired | `nmp-signer-broker` was deleted; NIP-46 uses actor-lane runtime. |
| 0032 | Current | Raw semantic data to shells remains doctrine. |
| 0033 | Folded | Feed viewport mechanics survive behind ADR-0070 sessions. |
| 0034 | Current | Kind-dispatched rendering registry survives. |
| 0035 | Folded | Root-indexed feed mechanics survive as private machinery under ADR-0070. |
| 0036 | Folded | Active-follow source lessons survive under ADR-0070. |
| 0037 | Current | Typed FlatBuffers sidecars survive. |
| 0038 | Folded | OP feed schema belongs to feed/protocol docs, not public read architecture. |
| 0039 | Amended | Push output survives; typed read lifecycle is ADR-0070. |
| 0040 | Current | Capability worker seam remains live, under ADR-0072 boundary rules. |
| 0041 | Folded | Relay-settings raw projection cleanup belongs under ADR-0032/docs. |
| 0042 | Folded | Raw interest surface is substrate only; product reads are ADR-0070. |
| 0043 | Current | `nmp-blossom` protocol crate survives. |
| 0044 | Current | Typed snapshot envelope fields survive. |
| 0045 | Current | Store-to-projection replay survives as internal catch-up. |
| 0046 | Folded | Composition-as-library survives under ADR-0069. |
| 0047 | Folded | Browser worker boundary is governed by ADR-0072. |
| 0048 | Current | NIP-55 external signer capability survives. |
| 0049 | Folded | Composition observability survives under ADR-0069 explicit roots. |
| 0050 | Current | Signer-session capability port survives. |
| 0051 | Current | NIP-11 relay info survives. |
| 0052 | Current | Instance-scoped extension seams survive. |
| 0053 | Folded | Projection declarations are output machinery under ADR-0070. |
| 0054 | Current | Web persistence/OPFS constraints survive under ADR-0072. |
| 0055 | Current | Incremental projection emission survives. |
| 0056 | Folded | K3 coverage ledger is implemented test/coverage process. |
| 0057 | Current | Unified accepted-event ingest chokepoint survives. |
| 0058 | Current | Pull event-log consumption survives for diagnostics/export. |
| 0059 | Current | Account lifecycle remains separate from bootstrap publish. |
| 0060 | Amended | NIP-29 admin/joined-groups survives; app-facing reads are ADR-0070. |
| 0061 | Amended | NIP-22 comments survives; publish/read surfaces follow 0070/0071. |
| 0062 | Folded | Observer catch-up survives as private read-session machinery. |
| 0063 | Amended | Reference resolution survives; typed session ownership is ADR-0070. |
| 0064 | Amended | One write doorway survives; intents/provenance are ADR-0071. |
| 0065 | Current | `ActorCommand` sub-enum collapse survives. |
| 0066 | Current | Delegated NIP-44 decrypt sessions survive as staged work. |
| 0067 | Folded | Browser runtime split is governed by ADR-0072. |
| 0068 | Folded | Native runtime split is governed by ADR-0072. |
| 0069 | Current | Redesign spine: explicit feature composition. |
| 0070 | Current | Redesign spine: typed read sessions. |
| 0071 | Current | Redesign spine: write intents and route provenance. |
| 0072 | Current | Redesign spine: runtime/capability/shell boundary. |
| 0073 | Current | Redesign spine: ADR reset and ratchets. |
| 0074 | Current | `nmp-nip09` exclusive owner of NIP-09 kind:5 deletion artifacts (#2511), widened to `a`-tag grammar and `AddressCoordinate` (#2589). |
| 0075 | Current | Trellis may be a private reconciliation substrate below typed sessions; NMP owns public APIs and Nostr/resource semantics (#2627). |
| 0076 | Proposed | App-facing feed APIs are typed read-session helpers over ADR-0070; `FeedParams` remains the descriptor, but helper APIs hide compiler/executor wiring (#1626). |

## Follow-Up Rule

When a future PR touches a folded or amended ADR's implementation area, it must
either keep the old public surface count flat/decreasing or update the owning ADR
in place. Do not add a new correction document that leaves stale current guidance
behind.
