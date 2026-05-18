# Synthesis — App / indexer relay model across NDK + applesauce

> Cross-cutting analysis of the two parallel research efforts.
> Source docs: `ndk-app-relay-model.md`, `applesauce-app-relay-model.md`.
> Date: 2026-05-18.

## TL;DR

The two libraries take **opposite design stances** on every dimension that
matters here. Neither implements the user's stated NMP model
(indexer always-on for kind:0/3/1xxxx with **R+W symmetry**, distinct
app-relay fallback). NMP has a clean opportunity to do this right.

| Dimension                       | NDK                              | applesauce                          | NMP target                                          |
|---------------------------------|----------------------------------|-------------------------------------|-----------------------------------------------------|
| Outbox routing                  | Opt-out (default on)             | Opt-in (default uniform fan-out)    | **Always-on, never bypassable**                     |
| Relay defaults at pool layer    | `explicitRelayUrls` in ctor      | None — pool is connection-only      | **None at transport; all roles at planner**         |
| Indexer concept                 | Effectively absent (mis-named)   | `lookupRelays` (read-only)          | **First-class, kind-gated, R+W symmetric**          |
| App-fallback concept            | Conflated with explicit relays   | `setFallbackRelays` (opt-in)        | **First-class, distinct from indexer**              |
| NIP-51 routing (10007/10013/10102) | None                          | Cast classes only; no router       | **First-class — already designed in ADR-0020**     |
| Routing API shape               | God-object methods               | Composable RxJS operators           | **Composable planner partition cases**              |
| Per-purpose relay subsetting    | No (causes #175 zap bug)         | Yes (loader scopes)                 | **Yes — every relay carries its lane**              |
| User-published indexer list     | Not modelled                     | Kind 10086 (applesauce-invented)    | **Defer — operator config is the floor for v1**     |
| Symmetric R+W indexer           | N/A                              | **Explicitly broken** (see §3.3)    | **Yes — kind:0/3/1xxxx publishes hit indexers too** |

## 1. Where NDK + applesauce converge (even by accident)

- **Both treat NIP-65 outbox as the primary routing input.** Neither models
  kind ranges or universal-data kinds as a special class. Both will route a
  kind:0 fetch through the author's NIP-65 read relays.
- **Both have hard-coded purplepag.es-style defaults.** NDK:
  `DEFAULT_OUTBOX_RELAYS = ["wss://purplepag.es/", "wss://nos.lol/"]`.
  Applesauce docs example: `lookupRelays: ["wss://purplepag.es/", "wss://index.hzrd149.com/"]`.
  Both treat these as the cold-start safety net.
- **Both lack a write-side indexer.** NDK never writes to its outbox-pool
  (it's a metadata-fetch pool only). Applesauce's `UpdateProfile` action
  has the literal comment *"No outboxes to publish to since this is
  probably a new user"* but does not publish to lookup relays either —
  it punts to the app developer.
- **Neither has a NIP-51 router.** Both know the kind numbers
  (10007, 10006, 10050); neither has a router that consumes them.
  Drafts (NIP-37 kind:31234) and wikis (NIP-54 kind:30818) are not
  routed by either library.

## 2. Where they diverge (and why each is wrong for NMP)

| | NDK | applesauce |
|---|---|---|
| Architecture | "Smart by default" — outbox auto-fetched, signer-relays auto-merged, blacklist soft-enforced | "Compositional" — every routing step is opt-in, app must wire `OutboxModel` + `includeFallbackRelays` + `ignoreBlacklistedRelays` |
| Failure mode | Footgun farm: 4 implicit relay sources at cold-start invisible to operator (issue #175, #149, #141, etc.) | Forgottenness farm: outbox routing is opt-in, so app devs who don't know to call `outboxSubscription` get uniform fan-out by default |
| What NMP rejects | "Smart default" that hides multiple sources from the operator | "Opt-in correctness" that lets app devs miss the safe path |

NMP wants **correct-by-default with every source operator-visible**. That
means:
- Outbox routing is always on (rejects applesauce's opt-in).
- Every relay carries its lane / role (rejects NDK's god-set).
- App devs cannot accidentally route through a wrong lane (rejects NDK's `explicitRelayUrls`'s four-jobs-in-one shape).

## 3. The applesauce code comment that crystallises NMP's contribution

`packages/common/src/helpers/relay-list.ts:6`:

```ts
/** Indexer / lookup relays: where to fetch or publish kinds 0 and 10002 (NIP-51 `relay` tags). */
export const LOOKUP_RELAY_LIST_KIND = 10086;
```

That comment says **"fetch or publish"** — but applesauce's runtime only
fetches. NMP's promise is to deliver on both sides of that comment:
every kind:0 / kind:3 / kind:10000–19999 read AND write goes to the
indexer set, in addition to NIP-65 outbox. This is the single biggest
correctness win NMP has over both libraries.

## 4. Primitives worth porting (applesauce → NMP, Rust idiom)

Direct algorithmic ports — applesauce already has well-tested logic:

```
applesauce TypeScript              NMP Rust equivalent
─────────────────────────────────  ────────────────────────────────────────
selectOptimalRelays(users, opts)   planner::selection::select_optimal
                                   (greedy coverage with per-user cap;
                                    already partially in case_a_authors)
groupPubkeysByRelay(pointers)      planner::outbox_map::build
                                   (HashMap<RelayUrl, Vec<Pubkey>>)
createFilterMap(outboxMap, filter) planner::compiler::project_per_relay
                                   (already the compiler's output shape)
setFallbackRelays(users, fallback) planner::partition::case_apprelay
                                   (NEW — cold-start substitution)
removeBlacklistedRelays(users, bl) compiler::post_filter::blocked
                                   (already in ADR-0020 §4.3)
```

`selectOptimalRelays` is the one NMP should benchmark against
applesauce's test suite — coverage-maximising greedy selection with
per-user relay cap is non-trivial and applesauce has reference behavior.

## 5. Six routing lanes (NMP's final shape)

After both reports, NMP's `RoutingSource` enum should be:

```rust
pub enum RoutingSource {
    /// Lane 1 — per-author NIP-65 outbox/inbox (already exists).
    Nip65 { direction: Direction },
    /// Lane 2 — relay hint from event tag (already exists).
    Hint,
    /// Lane 3 — provenance from prior event (already exists).
    Provenance,
    /// Lane 4 — user-configured (active-account read/write, debug).
    /// Indexer is REMOVED from this enum and promoted to its own lane.
    UserConfigured(UserConfiguredCategory),
    /// Lane 5 — NIP-51 class routing (search/draft/wiki — ADR-0020).
    ClassRouted { class: EventClass, via: ClassRoutingPath },
    /// Lane 6 — operator-configured indexer relays.
    /// ALWAYS-ON for kind:0, kind:3, kind:10000–19999. R+W symmetric.
    /// Distinct from UserConfigured(Indexer) which it replaces.
    Indexer,
    /// Lane 7 — operator-configured app relays.
    /// Fallback substitution when author has no NIP-65 mailbox.
    /// Additive at session login alongside NIP-65 read relays.
    AppRelay { mode: AppRelayMode },
}

pub enum AppRelayMode {
    /// Substitutes for NIP-65 when the author's mailbox is unknown.
    ColdStartFallback,
    /// Adds to NIP-65 at session login.
    LoginAdditive,
}
```

Plus the subtractive global filter (blocked relays from kind:10006),
which is a post-pass, not a lane.

## 6. Anti-patterns to refuse (from the research)

1. **One giant `pool.relays.keys()` set** (NDK #175). Every relay must
   carry the role(s) it serves; lane-aware iteration only.
2. **Hard-coded default relays in transport/pool layer.** Defaults live
   in operator config, surfaced in diagnostics.
3. **Opt-in correctness.** Outbox + class routing + blacklist filter
   are all default-on. App devs opt OUT explicitly, not in.
4. **Asymmetric R+W indexer.** If we read kind:0 from indexer, we
   publish kind:0 to it. No applesauce-style silent gap.
5. **Multi-job magic config fields.** NDK's `explicitRelayUrls` does
   four jobs; NMP splits cleanly: `IndexerRelays`, `AppRelays`,
   `DefaultRelayLists` (per-NIP-51-class fallback from ADR-0020).
6. **Auto-merging signer-supplied relays** (NDK `autoConnectUserRelays`).
   If the signer has relay opinions, they go through explicit operator
   configuration, not silent injection.

## 7. Open design choices for NMP

These need user input before the ADR can land:

**(a) Is the indexer set user-mutable?**
The user described "settings could show a list of relays and their
roles." That implies the operator ships defaults but the user can
override via app settings. Two sub-choices:
- (a1) Settings UI only — kernel exposes `set_indexer_relays(Vec<RelayUrl>)`,
  persisted in client-local store; never published to Nostr.
- (a2) Adopt applesauce's kind 10086 — user's indexer preferences
  published as a Nostr event, kernel consumes both operator default
  AND kind:10086 (union? user-overrides?). Closer to NIP-51 spirit
  but kind:10086 isn't a ratified NIP.

**(b) Indexer kind-gate scope.**
The user said "kind:0, kind:3, kind:1xxxx". Need to confirm:
- kind:1xxxx ≡ all 10000–19999, OR
- kind:1xxxx ≡ specific replaceable kinds (10000, 10002, 10003, …)?
  i.e. include the NIP-51 lists but exclude kind:13/14/etc.?
Practical risk: blindly routing every 10000–19999 kind through indexer
means DM-relay-list (kind:10050) goes through public indexers, which
is fine for that kind, but is there any 1xxxx kind we explicitly
want to KEEP off indexer relays?

**(c) AppRelay's "additive at login" duration.**
"Additive at login" could mean:
- (c1) Only the initial cold-start REQs go to AppRelay; once user's
  NIP-65 is fetched, AppRelay drops out.
- (c2) AppRelay stays in the union for the whole session, alongside
  NIP-65 reads.
Choice (c1) is closer to the "fallback when unknown" framing; (c2) is
closer to "the app developer's preferred relay always sees this user's
subscriptions."

**(d) Per-author AppRelay fallback granularity.**
When subscribing to many authors, some with NIP-65 mailboxes and some
without:
- (d1) AppRelay fires only for the no-mailbox authors (per-author check).
- (d2) AppRelay fires if any author in the interest lacks a mailbox
  (interest-level check).
- (d3) AppRelay always fires regardless of NIP-65 state.

**(e) Document split: one ADR or two?**
ADR-0020 currently covers intent-classed routing + NIP-50 search.
Adding Indexer + AppRelay roles is meaningful new scope:
- (e1) Fold into ADR-0020 — one document, expanded title to
  "Relay routing architecture v2".
- (e2) Spin out ADR-0021 — relay roles get their own ADR, with
  ADR-0020 updated to reference the new `RoutingSource` variants.

Recommended: **(e2)**. The two concerns are at different abstraction
levels (relay infrastructure vs intent semantics), and ADR-0021 is a
prerequisite to ADR-0020's full design. Splits cleanly along the
existing P3 boundary in the rollout plan.

## 8. Recommended decision shorthand

If the user picks "obvious" answers for all of (a)–(e):

- (a1) Settings UI only; kind:10086 deferred to v2.
- (b) All 10000–19999 (including DM-relay-list 10050 — public anyway).
- (c1) AppRelay drops out once NIP-65 fetched.
- (d1) Per-author fallback (most precise routing).
- (e2) Separate ADR-0021.

These five choices give NMP the cleanest design and the smallest
v1 surface area. Each is also defensible by analogy: (a1) keeps the
kernel small, (b) maps the user's stated kind range literally, (c1) /
(d1) keep AppRelay's role tightly scoped to "fix cold-start, not
augment steady-state," and (e2) preserves the existing ADR-0020
scope and enables independent rollout.
