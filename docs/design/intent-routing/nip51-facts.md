# Intent-classed routing + NIP-50 search §5 — NIP-51 fact stream

> Parent: `docs/design/intent-routing.md`.
> Cross-refs: planner integration in `planner.md` (§4); diagnostics in
> `diagnostics.md` (§6).

## 5. NIP-51 fact stream

| NIP-51 kind | Class / role               | Resolver method consumes it            | Encrypted? | Per-author? |
|-------------|----------------------------|----------------------------------------|------------|-------------|
| 10006       | blocked (global filter)    | `blocked_relays()`                     | no         | no          |
| 10007       | search relays (higher-order) | `SearchRelayListProjection` in `nmp-nip51`; consumed by `nmp-nip50` directly — NOT routed via `class_relays_personal` | no | no |
| 10013       | Draft relay list (NIP-37)  | future owner-declared class            | **yes** (nip44 to self) | no |
| 10102       | Wiki relay list (NIP-54)   | future owner-declared class            | no         | **yes**     |
| 10050       | DM (NIP-17)                | decoded only; routing deferred         | no         | no          |
| 30002       | named — see §5.1           | not consumed in v1                     | n/a        | n/a         |

Kind:10007 parsing/projection are owned by `nmp-nip51` via
`SearchRelayListProjection`. The planner/resolver does NOT consume kind:10007
— this is the higher-order search model. `nmp-nip50` reads from
`SearchRelayListProjection` directly, applies app-default fallback, and
subtracts blocked relays before building the relay set for any NIP-50 REQ.

### 5.1 Kind:30002 named relay sets — deferred

Named sets are addressable per `d` tag. No canonical convention maps `d`
values to `EventClass` variants. v1 doesn't consume them; apps that need
named-set routing use `PublishTarget::Explicit` after reading the list
themselves via the existing nmp-nip51 view. A future ADR may add a
runtime `(d_value, EventClass)` binding API.

### 5.2 Fact-stream wiring

```rust
pub struct Nip51RoutingFacts {
    // NOTE (2026-06-22): kind:10007 search relays are NOT included here.
    // They are projected by SearchRelayListProjection in nmp-nip51 and
    // consumed directly by nmp-nip50. This struct covers only the relay
    // facts needed by the core routing resolver.

    pub blocked: BTreeSet<RelayUrl>,                      // from kind:10006

    /// From kind:10013 (NIP-37 draft relays). Encrypted; the resolver
    /// surfaces it only after NIP-44 self-decryption succeeds.
    pub drafts: Vec<RelayUrl>,

    /// From kind:10050. Decoded only — `class_relays_personal(&DM)`
    /// does not consume this yet. Field exists so the future DM ADR
    /// can land without re-plumbing the fact stream.
    pub dm: Vec<RelayUrl>,

    /// Per-author kind:10102 lists. Lazy-populated when a class-routed
    /// Publisher-keyed interest first names the author. Evicted when the last
    /// such interest ends.
    pub wiki_per_author: HashMap<Pubkey, Vec<RelayUrl>>,
}
```

Wiring steps:

1. Register the kinds with the `nmp-nip51` decoder — add 10006, 10013,
   10050, 10102 to `ALL_KINDS` for core routing. Kind:10007 is decoded
   by `nmp-nip51`'s `SearchRelayListProjection` independently.
2. **Subscribe to the personal lists** (10006, 10013, 10050) as
   part of the active-account boot sequence, alongside the existing
   kind:10002 NIP-65 fetch. These are replaceable, so each is exactly one
   tailing subscription. Kind:10007 subscription is managed by `nmp-nip50`
   or `SearchRelayListProjection` — not by the core routing resolver.
3. **For kind:10013 only**, decrypt `.content` via the active signer's
   NIP-44 self-decryption. The decrypted blob contains the `"relay"`
   tags. Parsing is identical post-decryption.
4. **Per-author 10102 fetches** happen lazily, driven by the planner's
   `case_g_class_routed` partition (`planner.md` §4.4). The resolver owns the
   per-author subscription lifecycle.
5. Project the decoded relay URLs into the `Nip51RoutingFacts` slice.

The hot path (planner partition) reads from `Nip51RoutingFacts`
allocation-free (D8); projection from raw events happens at fact-
ingestion time.
