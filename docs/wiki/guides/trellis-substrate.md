---
title: Trellis Reconciliation Substrate
slug: trellis-substrate
topic: read-door
summary: Trellis (ADR-0075) is an in-memory, per-session, reactive read-side reconciliation substrate for dependency-graph transactions and deterministic replay that liv
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-07-03
updated: 2026-07-04
verified: 2026-07-03
compiled-from: conversation
sources:
  - session:91a86fdf-624c-446e-9b38-0fb02085121f
  - session:dcc80382-bcc0-45ea-8b9c-1a2fc741f872
  - session:fb992e80-b32b-4673-b2c2-40e8044504ee
---

# Trellis Reconciliation Substrate

## Purpose & Scope

Trellis (ADR-0075) is an in-memory, per-session, reactive read-side reconciliation substrate for dependency-graph transactions and deterministic replay that lives below typed read sessions and dies with the process by design. Trellis graphs are built fresh in `FeedSessionTrellisAdapter::new`, torn down on `close_scope`, with zero persistence in trellis-core.

Trellis is not part of the NMP programming model. NMP owns active reads and may use Trellis internally to reconcile resources, but no app-facing type, doc, or generated helper mentions Trellis. There is no Trellis vocabulary anywhere in nmp-wallet (ADR-0075 clean).

ADR-0075 does not forbid a private write-side use of Trellis in its Forbidden list, but its Decision grants Trellis only as machinery below typed read sessions, so a durable write saga would require a new ADR to extend Trellis into write lifecycles.

Trellis's replay is a derivation-consistency oracle over local inputs, while wallet recovery reconciles against a remote mint authority whose answers are not a function of local state — the same words (graph transactions, deterministic replay, trace/oracle) are false friends. ADR-0075 confines Trellis trace to dev-only tooling, while "why is my wallet this shape" is a product-surface question, making Trellis doubly moot for the trail.

On the wallet read path, Trellis applies only to acquisition (relaying and filtering per kind:10019), never to proof-set derivation. Proof-set derivation is NMP/actor-owned product meaning per ADR-0075 Ownership.

The read engine now owns dependent-demand lifecycle reconciliation generically as `DependentDemandReconciler`. The pre-engine hand-rolled `DynamicTargetProjection` in nmp-content — which compared desired shape vs current, closed old observed projections, opened new ones, and tore down recipes — is deleted as dead code (zero third-party consumers, existing twice verbatim).

InterestLifecycle threads through `ReadDemand`, `ObservedProjection`, the `OpenObservedInterest` command, dispatch arms, and `build_interest_pair`, deleting the hardcode that `open_interest` is always tailing. Every existing site defaults to `Tailing` explicitly. `InterestLifecycle` does not participate in the SubKey — it changes when a sub closes, not where it routes — so close reconstruction/dedup are unaffected and existing `Tailing` keys don't re-hash. A `OneShot` read demand's REQ is not re-emitted after EOSE evicts the wire sub, because the coverage ledger records EOSE coverage and a plan recompile retains it as an empty diff.

The following are kept as private implementation only: `ObservedProjectionSink`, source reducers, typed snapshot projection registration, the Trellis adapter, `InterestShape` / `LogicalInterest` machinery, `InterestLifecycle`, `DependentDemandReconciler`, and replay-before-live internals.

<!-- citations: [^91a86-88ac8] [^91a86-ce723] [^91a86-2fb1f] [^91a86-298b3] [^91a86-f20e5] [^dcc80-e4c80] [^91a86-c1919] [^fb992-282e3] -->
