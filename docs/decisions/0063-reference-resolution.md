# ADR-0063 — Reference resolution: a unified keyed, shape-demanded `RefResolver` primitive

- **Status:** Accepted (owner-decided via [#1671](https://github.com/issues/1671); parent of [#1635](https://github.com/issues/1635)).
- **Date:** 2026-06-21
- **Decides:** Collapse the three-way profile surface
  (`claimed_profiles` / `mention_profiles` / `resolved_profiles`) and the legacy
  event-claim / `claimed_events` surface into ONE kernel-owned, refcounted,
  deduped **reference-resolution primitive** — `RefResolver` — exposing a single
  origin-blind seam `resolve_ref(namespace, key, consumer_id, shape, liveness)` /
  `release_ref(...)`, and emitting ONE keyed push-updating projection per namespace
  (`refs.profile`, `refs.event`). Builds #1635's typed keyed profile read API on top.
- **Supersedes / amends:**
  - **ADR-0042** (profile-resolution residue). ADR-0042 §4 deliberately kept
    profile claims and event claims on a bespoke refcounted-one-shot lifecycle
    ("explicitly NOT done here — claim_* keeps its bespoke lifecycle") and §5
    routed author/thread profile data through the `claimed_profiles` tier. This ADR
    finishes that deferred migration: the claim surface and the
    claimed/mention/resolved tiering are folded into `resolve_ref`, and the
    `claimed > mention` precedence merge is deleted.
  - **aim.md §4.2** ("reactive models / derived views") — the profile view is now
    one keyed resolver output, not a three-tier precedence stack.
- **Extends:**
  - **ADR-0053** (host-declared projection subscriptions) — three declared Tier-2
    keys (`claimed_profiles`, `mention_profiles`, `resolved_profiles`) +
    `claimed_events` become ONE declared key per namespace (`refs.profile`,
    `refs.event`). The declared-set seam, drift gate, and `Narrow`/`All` tri-state
    are unchanged; only the key inventory shrinks.
  - **ADR-0055** (incremental projection emission). This ADR **realizes ADR-0055's
    deferred intra-projection row-delta rung (D8 / open question 4)**, scoped to
    `refs.*` first: a per-**key** rev inside the resolver so the ADR-0055 manifest
    carries `Changed` / `Cleared` at the **row grain within** `refs.profile` /
    `refs.event`, not whole-map. Every ADR-0055 correctness invariant (D1-D8, HA-1
    through HA-6) is preserved; the `refs.*` row-grain gates are re-stated below as
    mandatory gates.
- **Reaffirms:** **ADR-0032** (raw-data projection doctrine). The resolved-ref
  projections carry raw 64-char lowercase hex pubkeys / hex event-ids / `kind:pubkey:d`
  coordinates only — **no `npub` / `nevent` / bech32 forms**. Shells encode bech32 at
  the render layer. (This corrects #1635's "npub forms" wording — see §"#1635 correction".)
- **Doctrines touched:** D0 (substrate names no app noun), D4 (single writer per
  fact / kernel owns truth), D5 (snapshots bounded by what is open / declared), D6
  (errors never cross FFI; fail closed), D8 (no polling; observer-driven reactivity).

---

## Context

### One concept wearing a three-tier costume

Today the same fact — a user's `kind:0`-derived `ProfileCard` — is produced via two
parallel projection accessors and then reconciled with a precedence merge:

- `claimed_profiles()` (`kernel/update/projections.rs:252`) — `pubkey → ProfileCard`
  for every explicitly **claimed** pubkey, driven off the `profile_claims` refcount.
- `mention_profiles()` (`projections.rs:237`) — **returns an empty map** since
  V-112 / ADR-0042 deleted its `author_view` / `thread_view` item sources. It is a
  **dead tier**, still emitted (not absent) only to preserve the D1 presence contract.
- `resolved_profiles()` (`projections.rs:315`) — the pre-merged map every consumer
  reads, computed as `claimed_profiles` (highest) → `mention_profiles` (only-if-absent,
  i.e. never, since it is empty).

So the live structure is **one real tier wearing a three-tier costume**: a pubkey
resolves *iff* it was explicitly `claim_profile`-d. The precedence merge
(`projections.rs:321-335`) reconciles a populated map against a permanently-empty one.
Two code paths, one concept — a D4 ("single writer per fact") smell.

### Legacy event claims were already the second instance of the same machine

Profile claims (`kernel/requests/profile.rs`) and legacy event claims
(`kernel/requests/event.rs`) were **already two instances of one shape**:

1. **refcounted consumer ownership** — `profile_claims: HashMap<pubkey, BTreeSet<consumer_id>>`
   / `event_claims: HashMap<primary_id, BTreeSet<consumer_id>>`, bounded
   (`MAX_CLAIMS_PER_PUBKEY` / `MAX_EVENT_CLAIMS_PER_KEY`), drop-newest on overflow (D6);
2. **kernel-owned fetch/routing/cache policy** — both register a `LogicalInterest`
   through the same `InterestRegistry` chokepoint with NIP-65 discovery, author/shape
   coalescing, indexer fallback, store-first cache-serve, and hint seeding;
3. **a push-updating keyed projection** — `claimed_profiles` / `claimed_events`,
   each with a per-source rev counter already wired into ADR-0055's manifest
   (`source_versions.profile_claims_ver` / `claimed_event_content_ver`);
4. **release** — symmetric profile/event release paths decrement the refcount and
   drop the registry interest on the last owner.

The historical module docs described this symmetry directly. The two surfaces were
not two features; they were one primitive instantiated twice. This ADR names the
primitive.

### The coverage hole

Because resolution is *only* claim-driven and `mention_profiles` is dead, an author
that merely surfaces in a feed / notification / search row resolves **only if the
shell remembers to `claim_profile` it**. The framework provides no path that makes
"this pubkey is on screen" automatically equal "this pubkey is resolving." This is
the failure mode that silently gutted `mention_profiles`; it is currently latent only
because shells hand-claim authors.

### Apps re-derive read models

Olas (and others) mirror the snapshot's profile maps into native `[String: ProfileCard]`
dictionaries and key-lookup themselves — a D4 violation (native caching hydrated facts
the kernel owns) that #1635 exists to delete. There is also no demand-driven field
granularity: a feed avatar pays the full ~16-field `ProfileCard` even when it renders
only name + picture.

---

## Decision

A kernel-owned **`RefResolver`** primitive: a refcounted, deduped, origin-blind
reference resolver that generalizes the existing profile + event claim
machinery rather than inventing a new machine.

### D1 — One seam, origin-blind, refcounted + deduped

```
resolve_ref(namespace, key, consumer_id, shape, liveness)
release_ref(namespace, key, consumer_id)
```

- **`consumer_id`** is the refcount owner (a view, a renderer, a feed-author helper).
  Distinct call sites for the same `(namespace, key)` share one deduped interest slot;
  the last `release_ref` tears it down — exactly the
  `profile_claims` / `event_claims` refcount + `InterestRegistry` owner-set semantics
  that exist today (`profile.rs:223-286`, `event.rs:274-297`).
- **Origin-blindness is the single-codepath requirement.** "Explicitly claimed" vs
  "incidental feed/notification author" stops being a consumer-visible split and
  becomes an internal **liveness** detail (D3). One call retrieves an entity keyed by
  id, regardless of why it is on screen.

### D2 — Namespaces are a closed, typed set: `profile` + `event` ONLY

Ship exactly two resolvers. `profile` is the first; legacy event claims / `claimed_events`
fold into `refs.event`.

**We do NOT generalize to `zap_total` / reply-counts / relay-names.** Those are
*aggregate queries over event **sets*** — a different primitive with different
invalidation, coverage, and cache semantics (a count is invalidated by *any* matching
event arriving anywhere; a ref is invalidated only by a newer replacement of *that one
entity*). Folding aggregates into `resolve_ref` would make the abstraction dishonest:
one seam pretending two incompatible invalidation models are the same. Both shipped
namespaces already existed as claim families; nothing here is speculative. (This is the
adversarial scope limit from codex's review; it is honored as a hard boundary.)

### D3 — Shapes are closed, namespace-owned enums, NOT per-field masks

`shape` is a small **closed enum owned by each namespace**, naming *what bytes the
consumer needs*:

- `profile.ref { pubkey, display_name, picture_url }` — the feed-avatar shape.
- `profile.card { full ~16-field ProfileCard }` — the profile-screen shape
  (today's `ProfileCard`: nip05, about, lnurl, banner, website, lud16/lud06, …).
- `event.embed` — the render-an-embed-card shape.
- `event.raw` — the canonical signed NIP-01 event for the referenced event,
  serialized from the store-owned `RawEvent` / equivalent signed-event authority
  and including `id`, `pubkey`, `created_at`, `kind`, `tags`, `content`, and
  `sig`. This is exposed only through the generic `refs.event` row for
  `EventShape::Raw`; protocol projections such as NIP-29 group chat must carry
  event ids/render fields, not per-crate `raw_json` fields or reconstructed
  unsigned JSON.

`event.embed` stays the light render-card shape; `event.raw` must round-trip as
a signed event with the original signature.

**Per-field masks are rejected.** A mask (`{display_name, banner}` arbitrary subset)
would: (a) leak schema internals into the wire/API; (b) create a combinatorial
explosion of cache / rev / fetch states (one rev stamp per mask-combination per key);
(c) fight ADR-0053's key-granular declaration and ADR-0055's per-row rev (which assume
a finite, named set of projection shapes). A `ProfileCard` is only a few hundred bytes,
and ADR-0055 **already omits unchanged rows on the wire** — so the marginal cost of
sending the card rather than a sub-field is the bytes of the fields the consumer
didn't ask for, paid only on the ticks that key actually changes. The bet (codex
scope risk #3): **2-3 named shapes per namespace cover the real surface**; widening a
shape or adding one is a cheap additive change, while a mask is a one-way complexity door.

### D4 — Liveness (`CacheOk` | `Live`) is SEPARATE from shape

`liveness` is an **internal freshness policy kept strictly separate from `shape`**:

- **`shape` = what bytes are needed.**
- **`liveness` = how fresh the resolver keeps the entity.**

`CacheOk` serves from the store and, on a miss, does a single one-shot fetch that
closes on EOSE (no tailing sub). `Live` keeps a tailing sub open so replacements
(a new `kind:0`, an updated addressable event) arrive reactively. This is exactly
today's `ProfileLiveness` (`profile.rs:64-96`): `CacheOk → InterestLifecycle::OneShot`,
`Live → Tailing`.

The two axes must not be collapsed: a feed card wants `profile.card`-worth of bytes
*only when claimed* but at merely `CacheOk` freshness (it should not hold a live sub
per avatar row); an open profile screen wants `profile.card` at `Live`. **If shape
implied freshness the API would over-subscribe** (every detailed view forced live, or
every live view forced detailed). They are orthogonal and stay orthogonal.

**Dedup + liveness merge:** the same `key` from all consumers shares **one** interest
slot; **`Live` wins** (a `Live` claim upgrades a `CacheOk` slot in place via `set_sub`,
and the slot stays `Tailing` until the last owner releases — the existing
`live_profile_claims` "Tailing wins" rule, `profile.rs:236-248`). The slot carries the
**widest shape any live consumer demanded** (D5).

### D5 — Output: one keyed projection per namespace carrying the widest demanded shape

Replace the three profile projections (and `claimed_events`) with **one keyed
projection per namespace**:

- `refs.profile` — `key -> row payload` (`pubkey -> ProfileCard`), emitted as a
  generic `NRRD` row-delta batch whose changed-row payload is the `KPRF`
  profile codec.
- `refs.event` — `key -> row payload` (`primary_id -> ClaimedEvent`), emitted as
  a generic `NRRD` row-delta batch whose changed-row payload is a single-entry
  `KCEV` event codec.

Each row value carries the **widest shape any currently-live consumer of that
key demanded**. If any consumer wants `profile.card`, the row carries card
bytes; if all want `profile.ref`, it carries ref bytes. This is the
typed-per-namespace output (invariant iv): the namespace's row payload codec is
concrete, **never** an app-visible `namespace: String -> Value` registry.
`KCEV` remains a row codec only; it is not a live `claimed_events` projection.

### D6 — Full per-key reactivity, NOW (owner decision, overriding codex's defer)

Both layers land in this work, scoped to `refs.*`:

**(a) Wire-transport row-delta.** A per-**key** rev stamp lives inside the resolver, so
the ADR-0055 manifest classifies `Changed` / `Cleared` at the **row grain within**
`refs.profile` / `refs.event`. Only the changed pubkey's row crosses FFI on a `kind:0`
update — not the whole map. This pulls ADR-0055's deferred intra-projection row-delta
rung (its D8 / open question 4, previously "deferred-to-measurement,
targeting `nmp-feed`") **forward, scoped to `refs.*` first**, under the five invariants
below.

**(b) Host-side per-key observable slots.** The NMP-generated `ProjectionCache`
(`ProjectionCache.generated.swift` / `.kt`, the ADR-0055 HA-3 interposer) exposes
per-**key** observable handles, so exactly one `AvatarView(pubkey:)` re-renders when
that one pubkey's `kind:0` updates — **no app cache, no polling** (D8). This is the
generated cache-merge layer extended from per-projection-key slots to per-row-key slots
within `refs.*`; app accessors (`model.profile(pubkey)`) are byte-identical, so the
delta mechanics remain invisible to app code (the ADR-0055 "impossible to get wrong"
property).

> **Owner-vs-reviewer tension (recorded honestly).** codex (adversarial reviewer)
> recommended **deferring (a)** to ADR-0055's measured later rung and shipping only
> projection-level apply for `refs.*` now. The owner chose **full per-key now**. The
> resulting risk concentration is the per-key wire correctness; the five invariants in
> §"Invariants" are therefore **mandatory test gates**, not nice-to-haves — they are
> the agreed risk mitigation for taking the un-measured path. See
> Alternatives → "Defer per-key reactivity to measurement."

### D7 — Coverage hole dissolves: every rendered reference is a `resolve_ref`

Because resolution is now *only* `resolve_ref`-driven and the dead `mention_profiles`
tier is gone, feed / notification / search authors resolve through the **same path** as
explicitly-opened entities. To prevent the "shell forgets to resolve → row goes blank"
regression (the exact failure that gutted `mention_profiles`, now load-bearing
everywhere), the framework ships:

- **A host-helper that auto-resolves authors carried by a feed projection** — a feed
  row that carries an author pubkey gets that pubkey `resolve_ref`-d at
  `profile.ref` / `CacheOk` automatically, so a shell cannot silently forget.
- **A debug-build guardrail** that warns when a rendered pubkey has no live ref
  (a loud signal in dev, mirroring ADR-0053's `Undeclared` debug-assert discipline).

### D8 — One cut, no compat aliases; upgrade all consumers

Per the repo's no-compat-alias rule (downstream pins by git rev), this is a single
coordinated break — see §Migration. No `claim_profile` / `claimed_profiles` shims
survive.

---

## Invariants (mandatory test gates)

These are the correctness gates for the per-key wire row-delta; they restate the
ADR-0055 invariants (D3, HA-1, HA-4) at the **row grain within** `refs.*` and add the
typed-output / kernel-ownership requirements. Each is a test, not a hope.

1. **Absence on the wire == `Unchanged`, NEVER `Cleared`.** A `refs.*` key omitted
   from a frame means "this entity is unchanged at the host's last applied per-key rev."
   Clearing (a resolved entity going away — last consumer released) is **always
   explicit** (`state = Cleared`, a payload-less row). This is the ADR-0055 D3 footgun
   removal applied per row. *Test:* a frame omitting key K never drops K from the host
   cache.

2. **Decode-before-commit (fail closed, D6).** A row is **decoded successfully**
   before it replaces the cached slot for its key. A malformed row keeps the prior
   cached entry, does NOT advance that key's `applied_rev`, latches `needsResync`, and
   never corrupts the cache (other keys in the frame still commit). This is ADR-0055
   HA-4 at row grain.

3. **Full baseline / resync repairs any host cache state.** An epoch / session
   re-baseline (account switch, restart, schema change, host resync request)
   reconstructs the **complete** `refs.*` set from scratch, regardless of prior
   per-key history. *Property test (the core gate):* the incremental per-key stream
   applied == the full snapshot of the final state, for arbitrary drop / reorder / gap /
   epoch-reset sequences.

4. **Resolver output is typed per namespace.** `refs.profile` / `refs.event` decode to
   `ResolvedProfileRef` / `ResolvedEventRef` concrete types. There is **no app-visible
   `namespace: String → Value` registry** and no stringly-typed shape dispatch in app code.

5. **Lifecycle + projection truth stay kernel-owned (D4).** Native **declares**
   (`resolve_ref`) and **releases** (`release_ref`) refs and **renders** the typed
   state the kernel pushes; it MUST NOT cache hydrated profile/event facts (no native
   `[String: ProfileCard]`). The kernel owns the fetch, the merge, the rev, and the
   projection.

---

## Code reality (what the implementation generalizes)

The ADR cites the symbols the implementation renames/generalizes, so the design is
grounded in master, not aspiration:

- **Refcount + registry:** the profile claim/release path
  (`crates/nmp-core/src/kernel/requests/profile.rs`) and the event claim/release path
  (`crates/nmp-core/src/kernel/requests/event.rs`) become the two
  `RefResolver` instances behind `resolve_ref` / `release_ref`. The `profile_claims` /
  `event_claims` refcount maps, the `MAX_*_PER_*` bounds, drop-newest+counter (D6),
  and the `InterestRegistry` owner-set dedup are retained as the resolver internals.
- **Liveness:** `ProfileLiveness { CacheOk, Live }` (`profile.rs:64-96`) is the
  namespace-agnostic `liveness` axis; `event.rs` gains the same axis (today event
  claims are `OneShot`-only; `Live` addressable-event tailing is the event-namespace
  liveness it currently lacks).
- **Per-key rev home:** the resolver's per-key rev must live in
  `crates/nmp-core/src/kernel/projection_rev/source_versions.rs`. Today
  `profile_claims_ver` and `claimed_event_content_ver` are **whole-projection**
  counters (`profile.rs:182`, `event.rs:198/361`) folded into the manifest via
  `BUILTIN_PROJECTION_DEPENDENCIES` (`projection_rev/mod.rs`). Realizing per-**key**
  row-delta requires replacing the single `profile_claims_ver` / `claimed_event_content_ver`
  scalars with a **per-key rev map** for `refs.profile` / `refs.event` (a
  `HashMap<key, u64>` bumped at each entity's mutation site), and teaching the manifest
  + `make_update` encoder to emit per-row `Changed` / `Cleared` instead of
  whole-projection presence. This is the concrete extension point ADR-0055 left open.
- **Output schemas:** `crates/nmp-core/schema/{claimed_profiles,mention_profiles,resolved_profiles}.fbs`
  are deleted. `ref_rowdelta.fbs` (`NRRD`) carries the outer `refs.profile` /
  `refs.event` row-delta batch. `claimed_events.fbs` is retained only as the
  single-entry `KCEV` row-payload codec for one `refs.event` row; it is not a
  live whole-map projection. `profile_card.fbs` / `profile.fbs` are retained as
  the profile row payload codec.
- **Declared keys:** `KERNEL_BUILTIN_PROJECTION_KEYS`
  (`kernel/update/builtin_projection_keys.generated.rs`) loses the four keys and gains
  `refs.profile` / `refs.event`; `nmp-codegen`'s `SNAPSHOT_PROJECTIONS` registry and
  the `declared_projections` drift gate (ADR-0053) follow. The `permits(...)` gates in
  `kernel/update/projections.rs:193-219` collapse to two.

---

## #1635 correction (raw hex only)

#1635 ("Expose typed profile projection for app profile caches") lists "npub forms" in
the needed surface. That **violates ADR-0032**: projections carry raw protocol data;
bech32 / short-npub encoding belongs in the shell render layer
(`claimed_profiles()` already dropped `to_npub` per V-115, `projections.rs:258-260`).
The `refs.profile` projection carries **raw hex pubkeys only**; #1635's surface is
corrected to drop "npub forms." The shell encodes bech32 at render time.

---

## Alternatives considered

- **Per-field masks instead of closed shape enums.** Rejected (D3): masks leak schema
  internals, create combinatorial cache/rev/fetch states, and fight ADR-0053's
  key-granular declaration + ADR-0055's per-row rev. A card is a few hundred bytes and
  unchanged rows are already omitted; named shapes are cheaper and additive.
- **A profile-specific keyed read API without generalizing to a primitive.** Rejected:
  legacy event claims were **already** the second instance of this exact machine (refcount →
  registry → keyed projection → release). Shipping a profile-only API would leave the
  event twin un-unified and re-derive the same shape a third time the next entity-type
  that needs it. The honest move is to name the primitive that already exists twice.
- **Generalize to all entity namespaces now (a generic "reference system").** Rejected
  (D2 / codex scope risk #4): `zap_total` / reply-counts / relay-names are aggregate
  queries over event *sets* with a fundamentally different invalidation model; one
  abstraction over both is dishonest. Bounded to `profile` + `event` (both already
  existed as claim families), nothing speculative, until a real third namespace proves it.
- **Defer per-key reactivity to measurement (codex's recommendation).** codex
  recommended doing only projection-level apply for `refs.*` now and deferring the wire
  row-delta (a) to ADR-0055's measured later rung. **The owner chose full per-key now.**
  Recorded as an explicit tension: the owner judged the per-row win load-bearing for the
  avatar-re-render goal and accepted the per-key wire complexity up front, with the five
  Invariants as the agreed risk mitigation (the property test "incremental stream ==
  full snapshot of final state" is the gate that makes the un-measured path safe).
- **Keep the `claimed > mention` precedence merge / the three-tier structure.**
  Rejected: `mention_profiles` is a dead tier (empty since ADR-0042) and the merge
  reconciles against a permanently-empty map — one real tier in a three-tier costume,
  a D4 smell. One keyed projection per namespace is the honest shape.
- **Native `[String: ProfileCard]` caches (status quo in Olas).** Rejected (D4 /
  invariant v): native must not cache hydrated facts the kernel owns. `model.profile(pubkey)`
  reads the kernel-pushed typed row directly; the native dict is deleted (#1635).

---

## Migration (one cut, no compat aliases — upgrade every consumer)

**Delete:**
- `claimed_profiles.fbs`, `mention_profiles.fbs`, `resolved_profiles.fbs`
  + their generated Rust/Swift/Kotlin bindings, and the host-visible
  `claimed_events` projection bindings. The KCEV `claimed_events.fbs` codec
  remains only as the per-row `refs.event` payload format.
- The `claimed_profiles()` / `mention_profiles()` / `resolved_profiles()` /
  `claimed_events()` accessors and the precedence merge (`projections.rs:237-338`).
- `MentionProfilePayload` (`kernel/types.rs`) and its typed sidecar
  (`typed_projections/mention_profiles_fb.rs`, `builtins_profiles.rs`).
- The four keys from `KERNEL_BUILTIN_PROJECTION_KEYS` + their `SNAPSHOT_PROJECTIONS`
  / declared-set entries; shells' `claimedProfiles` / `mentionProfiles` /
  `resolvedProfileCards` accessors; Olas's native `[String: ProfileCard]` `profileCache`.

**Add:**
- The `RefResolver` trait + `resolve_ref` / `release_ref` seam (C-ABI + JNI),
  `namespace` + `shape` closed enums, `liveness` axis on both namespaces.
- `refs.profile` / `refs.event` projection keys whose typed payload is the
  generic `ref_rowdelta.fbs` (`NRRD`) row batch. The namespace-specific row
  payload codecs are `KPRF` for profile rows and single-entry `KCEV` for event
  rows (`profile.ref` / `profile.card`, `event.embed` / `event.raw` shape
  semantics live in the resolver, not in an app-visible projection fork).
- Per-key rev in the resolver (replacing the whole-projection
  `profile_claims_ver` / `claimed_event_content_ver` scalars); per-row `Changed` /
  `Cleared` manifest + encoder support; per-key observable slots in the generated
  `ProjectionCache`.
- The feed-author auto-resolve host-helper + debug-build guardrail (D7).

**Rename / fold:**
- Profile claim/release → the `profile` resolver behind `resolve_ref`.
- Event claim / `claimed_events` → the `event` resolver / `refs.event`.

**Upgrade in the same cut:** all shells (iOS / Android / desktop / TUI) swap claim
calls for `resolve_ref` and read the single keyed accessor per namespace
(feed cards: `profile.ref` / `CacheOk`; profile screens: `profile.card` / `Live`;
search & discover: `profile.ref` / `CacheOk`; notifications: `profile.ref` / `CacheOk`).
Olas renders from `model.profile(pubkey)` and deletes its dict.

---

## Consequences

- **One kernel-owned resolution path** keyed by entity; the
  `claimed` / `mention` / `resolved` tiers and the precedence merge are gone (D4).
- **Coverage hole closed (D7):** any pubkey surfaced in any open view (feed / profile /
  search / notification) resolves through the same path; the feed-author auto-resolve
  helper + debug guardrail make "forgot to resolve" a loud bug, not a silent blank row.
- **ADR-0053 declared set shrinks** by 3 (and folds `claimed_events`): one declared key
  per namespace. The tri-state, drift gate, and `Narrow` / `All` discipline are unchanged.
- **ADR-0055's row-delta rung is realized for `refs.*`** under the five Invariants;
  one `AvatarView` re-renders when one pubkey's `kind:0` updates, no app cache, no
  polling (D8). This is the first per-row delta on the wire; the floor it proves can
  later graduate the feed (the rung ADR-0055 originally targeted).
- **ADR-0032 reaffirmed:** raw hex only on the wire; bech32 is shell-side. #1635's
  "npub forms" wording is corrected.
- **#1635 satisfied:** Olas renders from `model.profile(pubkey)`; the native
  `profileCache` is deleted.
- **Risks** (from #1671, carried for the implementor): (1) per-key wire correctness is
  the riskiest, highest-value piece — review concentrates on Invariants 1-3; (2)
  coverage regression at the cut is mitigated by the auto-resolve helper + guardrail;
  (3) `shape` is a one-way door on the wire — widen `ref`, add a shape, or eat the
  card (adding shapes is cheap); (4) scope creep into a generic reference system is
  bounded by shipping only `profile` + `event`.

---

## References

- [#1671](https://github.com/issues/1671) — the decided design (parent issue; owner
  decisions, codex scope limits, the per-key-now-vs-defer call).
- [#1635](https://github.com/issues/1635) — the consumer-facing acceptance (Olas
  deletes its native `profileCache`; "npub forms" corrected here).
- ADR-0042 — M2 open-interest migration (deferred the `claim_*` lifecycle this finishes).
- ADR-0053 — host-declared projection subscriptions (extended; key inventory shrinks).
- ADR-0055 — incremental projection emission (extended; row-delta rung realized for `refs.*`).
- ADR-0032 — raw-data projection doctrine (reaffirmed; raw hex only).
- `crates/nmp-core/src/kernel/requests/{profile.rs,event.rs}` — the two `claim_*`
  instances generalized into `RefResolver`.
- `crates/nmp-core/src/kernel/update/projections.rs` — the producers / precedence merge deleted.
- `crates/nmp-core/src/kernel/projection_rev/source_versions.rs` — where the per-key rev must live.
