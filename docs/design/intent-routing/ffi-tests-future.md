# Intent-classed routing + NIP-50 search §9–§11 — FFI, tests, future work

> Parent: `docs/design/intent-routing.md`.
> Cross-refs: type surface in `types.md` (§3); cache-side search in
> `cache-search.md` (§7). (There is no §8 in this design; section numbers are
> preserved from the original document.)

## 9. FFI / app-developer ergonomics

```swift
// Searching users — one call, streaming view (nmp-nip50 higher-order API).
// nmp-nip50 reads kind:10007 from SearchRelayListProjection (nmp-nip51),
// applies app-default fallback, subtracts blocked relays, and fans out.
// The core planner sees only an InterestShape { search: Some("satoshi"), .. }
// with relay URLs already resolved — no EventClass::Search involved.
let view = kernel.openSearch(.init(
    query: "satoshi",
    scope: .users,
    targets: .userPreferred,
    limit: 50
))
for await delta in view.deltas {
    // render new hits as they arrive
}

// Publishing a draft — no app-side relay knowledge.
kernel.publish(event: draftEvent, target: .auto)
// Kernel: kind 31234 → owner-declared draft class (personal family)
//                    → class_relays_personal(<draft class>)
//                    → user's decrypted kind:10013 list
//                    → subtract blocked_relays() → dispatch.

// Publishing a checkpoint — same class, same routing.
kernel.publish(event: checkpoint, target: .auto)
// Kernel: kind 1234 → owner-declared draft class → same kind:10013 relays.

// Publishing a wiki entry — publisher-keyed routing.
kernel.publish(event: wikiEvent, target: .auto)
// Kernel: kind 30818 → owner-declared wiki class (publisher-keyed family)
//                    → class_relays_for_author(<wiki class>, signer.pubkey)
//                    → my kind:10102 list (publishing as self)
//                    → subtract blocked_relays() → dispatch.

// Reading multi-author wikis — per-author partition.
let interest = LogicalInterest {
    shape: InterestShape {
        kinds: [30818], authors: [bob, alice], ..
    }, ..
}
// Kernel: splits the interest. Bob's events route via bob's 10102,
// alice's via alice's 10102. Each author's 10102 fetched lazily.
```

The "app authors forget" failure mode the user flagged is closed by:

- `Auto` being class-aware from day one — apps opt out, not in.
- `blocked_relays` enforced kernel-side on every target.
- `EventClass::from_kind` being a static table — apps never look up
  "which relay class does kind 31234 belong to?" themselves; the parent
  draft (31234) and its checkpoints (1234) share routing automatically.

## 10. Test surface (M-gate criteria)

- **Unit:** `EventClass::from_kind` covers every kind in the codebase's
  `kind` constants. **Checkpoint↔parent class equivalence:**
  `from_kind(1234) == from_kind(31234) == <draft class>`. Rule 10
  merge refusal. Blocked-relay subtraction. `RoutingFamily` mapping for
  every variant. NOTE: `EventClass::Search` is no longer a valid variant;
  search tests belong in `nmp-nip50`, not in the core planner test suite.
- **NIP-44 decryption gating:** kind:10013 surfaces only when a signer
  is attached and self-decrypt succeeds.
- **Per-author publisher-keyed partition:** interest with `authors=[bob, alice],
  kinds=[30818]` compiles into two distinct sub-shapes, one routed via
  bob's 10102 and one via alice's. Property-test for N authors.
- **Lazy 10102 lifecycle:** first interest naming `bob` triggers a
  one-shot kind:10102 fetch; second interest hits the cache; closing
  the last interest evicts bob from `wiki_per_author`.
- **Fail-loud blocked:** plan with every relay in `blocked_relays()`
  returns `PlannerError::AllRelaysBlocked`; `PublishEngine` maps to
  `PublishOutcome::AllRelaysBlocked`.
- **Search relay selection (nmp-nip50 test):** `SearchTargets::UserPreferred`
  reads from `SearchRelayListProjection` (nmp-nip51), applies fallback, and
  subtracts blocked relays; all assertions live in `nmp-nip50` tests. The
  real-relay integration test against `search.nos.lol` for kind:0 and
  kind:30023 is a mirror of `crates/nmp-testing/tests/real_relay_outbox.rs`
  but registered in `nmp-nip50`.
- **Diagnostic:** five-lane assertion fixture covering one example per
  lane plus one blocked-relay subtraction and one `AllRelaysBlocked`
  failure path.

## 11. Future work

Things deliberately out of scope for v1 but documented as known
extension points:

- **DM routing (NIP-17 / kind:10050).** Variant reserved; fact stream
  decoded. Own ADR.
- **Named relay sets (kind:30002).** No class binding in v1. Future
  ADR may add a runtime `(d_value, EventClass)` registration API.
- **NIP-72 communities, NIP-90 DVMs.** Default to `EventClass::Other`
  today. Future ADRs if usage demands.
- **Good wiki authors (kind:10101).** Author allowlist, not relay
  routing. Publisher-keyed views may consume independently of this design.
- **Cross-account routing for personal-class lists.** Currently only
  Publisher-keyed classes use publisher-keyed routing. If a future NIP defines
  a per-author search or personal-class list, the trait already supports it —
  add a new `EventClass` and its `RoutingFamily::PublisherKeyed`
  mapping.
- **Cache-side full-text inverted index** (issue #1811) — replaces linear
  scan once corpus exceeds the migration baseline (~10k profiles, ~1k
  articles). Deliverables: a visitor-style `text_search_visit` store seam,
  LMDB FTS sub-databases, and a crate-registered scope registry so `nmp-nip50`
  and future protocol crates own their FTS scope without touching core.
- **NIP-11 probing** of search relays if blind fanout proves too noisy
  in practice.
