# ADR-0021 — Relay roles: Indexer + AppRelay

> **Status:** Accepted (2026-05-18, design phase — no code yet).
> **Companion:** `docs/design/relay-roles.md` (full spec).
> **Prerequisite for:** ADR-0020 (intent-classed routing + NIP-50 search).
> ADR-0020's `ClassRouted` lane references this ADR's promoted
> `RoutingSource::Indexer` variant; both ADRs land together in the same
> rollout sequence.
> **Research basis:** prior research at `docs/research/relay-lifecycle-and-pools.md`,
> `docs/research/ndk/`, `docs/research/applesauce/`; two flat-file
> agent reports (`ndk-app-relay-model.md`, `applesauce-app-relay-model.md`)
> produced during this design session were wiped before commit and may
> need to be regenerated for full traceability.
> **Reconciliation note:** This ADR adds NEW variants to the planner-layer
> `RoutingSource` enum. It does NOT modify the worker-layer `RelayRole`
> enum (`crates/nmp-core/src/relay.rs:57`) — those are different
> abstraction levels (worker = transport-lane diagnostic bucketing;
> planner = "why this relay was chosen"). See `docs/design/relay-roles.md`
> §0 for the per-lane mapping.

## Context

NMP's existing `UserConfiguredCategory::Indexer` is **substitutive
fallback only** — it fires when an author's NIP-65 mailbox is unknown
or for no-author interests. This shape was inherited from NDK
(`outboxRelayUrls` defaults to `wss://purplepag.es/` + `wss://nos.lol/`,
used only for metadata discovery) and from applesauce
(`lookupRelays`, opt-in fallback on the AddressLoader's miss path).

The parallel research (`docs/research/ndk-app-relay-model.md`,
`docs/research/applesauce-app-relay-model.md`) showed that **neither
library implements the relay-role architecture NMP needs**:

- NDK has **no real indexer** — its `outboxRelayUrls` is a
  metadata-fetch pool, not a routing destination. NDK's
  `explicitRelayUrls` does four jobs at once (bootstrap, cold-start
  fallback, publish top-up, kind:0 broadcast) — root cause of
  [issue #175](https://github.com/nostr-dev-kit/ndk/issues/175) where
  zap requests overflow HTTP header limits because *every* connected
  relay URL gets serialized in.
- applesauce gets closer with `lookupRelays` and the
  applesauce-invented kind:10086, but **explicitly ships an asymmetric
  read-only indexer**. The literal comment at
  `packages/common/src/helpers/relay-list.ts:6` says
  *"Indexer / lookup relays: where to fetch or publish kinds 0 and
  10002"* — but the runtime only fetches. `UpdateProfile`'s code
  comment confesses: *"No outboxes to publish to since this is
  probably a new user."*

Kind:10086 has since been formalized as **NIP-51 PR #1985**:

> | Indexer relays | 10086 | Where to download/send kinds 0 and 10002 | `"relay"` (relay URLs) |

The PR scopes indexer relays to *"download/send kinds 0 and 10002"* —
explicitly **R+W symmetric**, validating the user's stated NMP model
against the applesauce gap.

NMP needs three things neither library provides:

1. **Indexer as a first-class routing role**, always-on for a
   designated kind set, R+W symmetric.
2. **AppRelay as a distinct fallback role** for authors lacking
   NIP-65 mailboxes, applied per-author with session lifetime.
3. **Per-purpose relay subsetting** — every relay in a plan carries
   its lane(s); no global pool that can leak across roles.

## Decision

Ten decisions form the v1 contract. The companion design doc spells
out types, planner integration, and rollout.

**(1) Promote `Indexer` to a top-level `RoutingSource` variant.**
Remove from `UserConfiguredCategory`; add as a sibling lane to
`Nip65 / Hint / Provenance / UserConfigured / ClassRouted`. This is a
behavior change: today's `UserConfiguredCategory::Indexer` is
substitutive fallback only; the new `RoutingSource::Indexer` is
always-additive for kinds in scope.

**(2) Add `AppRelay` as a top-level `RoutingSource` variant.**
Distinct from Indexer. Used as per-author fallback when an author
has no known NIP-65 mailbox.

**(3) Indexer kind-gate scope.** Indexer relays union into every
plan whose interest touches **kind:0, kind:3, or kind:10000–19999
(inclusive)**. This is broader than NIP-51 PR #1985's literal scope
("kinds 0 and 10002") — NMP applies kind:10086 lists to a wider
universal-data range as a policy choice. The kind:10086 event shape
remains spec-compatible; only the consumption rule is broader. See
§Consequences for the divergence note.

**(4) Indexer is R+W symmetric.** Reads AND writes of kinds in scope
union the indexer set with NIP-65 outbox. This explicitly closes the
applesauce gap (`UpdateProfile` publishes kind:0 to outboxes only;
NMP publishes to outboxes ∪ indexers).

**(5) Indexer source chain.** The active set is:
`operator_default ∪ user_kind_10086`. Operator default is configured
at kernel construction. User's published kind:10086 (NIP-51 PR
#1985) is consumed as a fact stream — same wiring as NIP-65
(kind:10002). When the user has no kind:10086, only operator default
applies. Default operator value ships as `["wss://purplepag.es/"]`
per the PR's recommended default.

**(6) AppRelay is per-author substitutive fallback.** For any
interest with `authors=[a₁, …, aₙ]`, the planner partitions by
author. Authors with a known NIP-65 mailbox use their outbox; authors
without route through the AppRelay set. Granularity matches ADR-0020's
wiki per-author partition (`case_a_authors`).

**(7) AppRelay session lifetime.** AppRelay remains active for the
entire session — it is not a cold-start-only transient. An author
who never publishes kind:10002 routes through AppRelay for the whole
session; when their kind:10002 lands, they graduate to their own
outbox on the next compile.

**(8) AppRelay source chain.** Same pattern as Indexer:
`operator_default ∪ user_settings_override`. User override is stored
in client-local settings (not published to Nostr) for v1 — distinct
from Indexer's kind:10086 publication path. The asymmetry is
deliberate: kind:10086 is now spec-blessed for indexer preferences;
no equivalent NIP exists for "app relay preferences" and inventing
one is out of scope.

**(9) Per-purpose relay subsetting (anti-#175 invariant).** Every
relay URL in every compiled plan carries its `RoutingSource` lane(s)
in `RelayPlan::role_tags`. The kernel maintains a per-purpose view
of "why is this relay connected"; no code path iterates `pool.relays.keys()`-
style over a god-set. Operations that need "all connected relays" must
specify which lanes count. This invariant is enforced by an
assertion at compile time: `RelayPlan::role_tags` is never empty.

**(10) No silent signer-relay merging.** If a signer (NIP-46 remote,
NIP-07 browser extension, hardware) advertises preferred relays via
its `relays()` API, the kernel does NOT auto-merge them into routing.
The application must surface signer-suggested relays to the user as
an explicit settings change (proposed AppRelay entries, or
indexer additions). This rejects NDK's `autoConnectUserRelays`
default and follows applesauce's stricter composition stance.

## Consequences

### Wins

- **Closes applesauce's asymmetric-indexer gap.** Kind:0 / kind:3 /
  NIP-51 lists land on indexers when published, not just when read.
- **Closes NDK's god-set bug class (#175).** Every relay carries its
  lane; no operation iterates the union accidentally.
- **Cold-start works without app-developer intervention.** New users
  with no NIP-65 hit AppRelay for content fetches and Indexer for
  kind:0/3/list fetches — both default-on, no app-side wiring.
- **Operator-visible relay sources.** Replaces NDK's three implicit
  sources (`explicitRelayUrls`, signer relays via `autoConnectUserRelays`,
  hard-coded `DEFAULT_OUTBOX_RELAYS`) with explicit kernel-construction
  config plus user-settings override. Every source appears in
  diagnostics.
- **NMP's kind:10086 consumption is spec-aligned** (event shape per
  NIP-51 PR #1985) with broader application scope (decision 3) —
  no incompatibility with other clients adopting the PR.

### Costs / risks

- **`RoutingSource` enum expansion.** Two new top-level variants
  plus the removal of `UserConfiguredCategory::Indexer`. Every
  consumer of `RoutingSource` (diagnostics view, test fixtures,
  audit-gate assertion) updates. Variant migration is mechanical
  but touches a lot of files.

- **Diagnostic lane count grows to seven.** NIP-65 / Hint /
  Provenance / UserConfigured / ClassRouted / Indexer / AppRelay.
  Existing four-lane test fixtures
  (`docs/design/subscription-compilation/diagnostics.md` §5)
  expand to seven-lane assertions. ADR-0020 already pushed to
  five lanes; this ADR adds the two additional lanes simultaneously
  in P3 of the joint rollout.

- **Kind scope divergence from NIP-51 PR #1985.** NMP applies
  kind:10086 relays to the broader `0 ∪ 3 ∪ 10000–19999` range
  rather than the PR's literal `0 ∪ 10002`. If a future NIP
  tightens or clarifies the kind:10086 scope incompatibly,
  NMP's behavior may need adjustment. Mitigation: the kind set
  is a single const (`INDEXER_KINDS`) in one module; changing it
  is a one-line patch. Documented as known divergence; surfaced
  in the kernel diagnostic so operators can spot it.

- **AppRelay-routed events have weaker privacy.** An author without
  kind:10002 sees every interest naming them routed through the
  app-configured AppRelay set. Apps targeting privacy-sensitive
  audiences must surface this in their settings UI ("authors
  without published relay preferences are fetched from
  `relay.myapp.com`"). Same dynamic exists in NDK / applesauce
  with their default fallbacks but is now explicit.

- **Two indexer source mechanisms (operator + kind:10086).**
  User publishes kind:10086 → that union with operator-default
  becomes their indexer set. If the user wants to *remove* a
  default the operator shipped, kind:10086 cannot express
  subtraction (it's purely additive — list of preferred relays).
  Mitigation: operator defaults are also user-overridable via
  client settings (same path as AppRelay). Documented as known
  limitation.

- **Indexer always-on multiplies background traffic for users with
  many active NIP-51 lists / replaceable kinds.** Each subscribed
  kind in `INDEXER_KINDS` adds the indexer set to its routing plan
  in addition to NIP-65. For a session with active kinds
  {0, 3, 10000, 10002, 10003, 10006, 10007, 10013, 10050, 10102},
  every one of those subscriptions also lands on every indexer
  relay. Probably fine in practice — indexer sets are typically
  1–3 relays — but quantifiable load test required in P3 gate.

### Non-decisions (deferred)

- **User-published "AppRelay preferences" event kind.** No
  equivalent of kind:10086 exists for app-relay user choice.
  v1 keeps user override in client-local storage. Future ADR may
  propose a new NIP-51 entry if cross-device sync becomes
  important.
- **NIP-66 relay liveness integration.** NDK PR #385 / #387 are
  adding NIP-66 liveness filtering to outbox. NMP defers — when
  NIP-66 lands as a stable NIP, a future ADR addresses how it
  prunes Indexer / AppRelay / Outbox plans.
- **Subtraction semantics for kind:10086.** PR #1985 is purely
  additive; cannot express "exclude operator-default X". v1 uses
  client-settings override for subtraction; future NIP may add
  this primitive.

## Alternatives considered

- **Keep `UserConfiguredCategory::Indexer` as-is** (substitutive
  fallback only). Rejected. The user's framing was explicit: indexer
  relays should be always-on for universal-data kinds. The current
  shape is the NDK mistake (an indexer that only fires when other
  routing failed); fixing it later is more invasive than promoting
  the variant now.

- **One `OperatorConfiguredRelays` lane** covering both Indexer and
  AppRelay. Rejected. The two roles have different application
  rules (kind-gated always-on vs per-author NIP-65-miss fallback)
  and different source chains (kind:10086 vs client-settings).
  Collapsing them obscures both invariants. NDK's `explicitRelayUrls`
  is the cautionary tale.

- **Skip kind:10086 entirely; use operator config only.** Rejected.
  PR #1985 is the spec-blessed path for the user-preference layer.
  Adopting it costs us a single fact-stream wiring and lets users
  carry their indexer preferences across clients.

- **Apply kind:10086 literally (kinds 0 + 10002 only).** Rejected.
  The user's mental model treats universal-data kinds as a class —
  profiles, follow lists, ALL NIP-51 lists — and NMP's planner can
  enforce that class consistently. Restricting indexer to PR
  #1985's literal scope would leave kind:10007 search-relay reads
  going only to NIP-65 outbox, which often fails for users without
  mailboxes. Broader scope, same event shape; no
  spec-incompatibility on the wire.

- **AppRelay only fires at cold-start, drops out after NIP-65
  fetched.** Rejected per user decision (whole-session).
  Authors who never publish kind:10002 would have nowhere to
  route after the "cold-start window" closed.

- **Auto-merge signer-supplied relays.** Rejected. NDK does this
  by default; the research surfaced it as a footgun (operators
  don't know which sources are merged). NMP requires explicit
  operator/user config.

## References

- ADR-0020 — intent-classed routing + NIP-50 search (this ADR
  is its prerequisite; ADR-0020's `ClassRouted` lane needs the
  promoted `RoutingSource::Indexer` and `RoutingSource::AppRelay`
  to exist first).
- ADR-0012 — `relay_pin` and the third routing lane (unchanged;
  remains the mechanism for NIP-29 GroupMessage).
- `docs/design/relay-roles.md` — full design.
- `docs/research/ndk-app-relay-model.md` — NDK analysis.
- `docs/research/applesauce-app-relay-model.md` — applesauce analysis.
- `docs/research/SYNTHESIS-app-relays.md` — cross-cutting synthesis.
- [NIP-51 PR #1985](https://github.com/nostr-protocol/nips/pull/1985)
  — kind:10086 (Indexer relays).
- [NDK issue #175](https://github.com/nostr-dev-kit/ndk/issues/175)
  — the canonical god-set-leak failure mode this ADR prevents.
