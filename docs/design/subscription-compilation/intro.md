# Subscription Compilation §1–§2 — Problem and Logical Interest Model

> Parent: `docs/design/subscription-compilation.md`.
> Read first: `docs/design/ndk-applesauce-lessons.md` §7 (subscription compilation lessons), §2 (NDK outbox lessons), §9 (NMP principles).

## 1. Problem — what is wrong with the current planner

The kernel today encodes "where a REQ goes" as a two-valued enum and resolves it at the call site of every request. Concretely:

- **Two hardcoded relays.** `crates/nmp-core/src/relay.rs:1-2` declares `CONTENT_RELAY_URL = "wss://relay.primal.net"` and `INDEXER_RELAY_URL = "wss://purplepag.es"` as module-level constants. There is no per-author routing.
- **Relay choice is a 2-variant enum, not a URL set.** `crates/nmp-core/src/relay.rs:15-39` defines `RelayRole::{Content, Indexer}` with a `.url() -> &'static str` that returns one of the two literals. This shape cannot express "this REQ should go to the union of these N authors' write relays."
- **The seam that emits REQs is parameterized by `RelayRole`.** `crates/nmp-core/src/kernel/requests.rs:530-556` (`req()`) inserts a `WireSub { role, .. }` keyed by a string sub-id and emits `OutboundMessage { role, text }`. The role *is* the routing decision; there is no relay-URL field on `WireSub` or `OutboundMessage`. Any compiler that fans an interest out across N URLs has to replace this helper.
- **Startup REQs ignore mailboxes by construction.** `crates/nmp-core/src/kernel/requests.rs:50-106` (`startup_requests`) issues six fixed REQs, each pinned to `Content` or `Indexer`. The seed-bootstrap timeline (line 65–70) fans seven hundred-author future timelines through one relay. The subscription-compilation audit requires that this fan equal the union of those authors' write relays.
- **View-open REQs ignore mailboxes too.** `crates/nmp-core/src/kernel/requests.rs:404-439` (`author_requests`) hardcodes a three-REQ shape — `author-relays-N` on Indexer, `author-profile-N` on Indexer, `author-notes-N` on Content. The author's notes are fetched from the global content relay even though by the time the view opens we may already have that author's kind:10002 in cache (see next bullet).
- **Mailbox cache exists but no consumer.** `crates/nmp-core/src/kernel/ingest.rs:209-233` (`ingest_relay_list`) already parses kind:10002 into `self.author_relay_lists: HashMap<String, AuthorRelayList>` (declared at `crates/nmp-core/src/kernel/mod.rs:269-275` and reserved at `mod.rs:313`). The cache is written; **nothing reads it for routing**. This is the bug doctrine D5 ("capabilities report, never decide") inverted: we have the data, we ignore it.
- **Profile claim path is single-relay.** `crates/nmp-core/src/kernel/requests.rs:390-402` (`profile_claim_request`) sends a kind:0 fetch to `RelayRole::Indexer` unconditionally. It cannot consult mailboxes for the claimed author.
- **No publish path exists yet.** `crates/nmp-core/src/kernel/requests.rs:30` (no occurrences of `EVENT` outbound) and `crates/nmp-core/src/relay.rs:42-45` (`OutboundMessage` carries only role + text). The first publish action (M6 `SendNote`) will hit this same `req()`-style seam. M2 must establish the planner shape before M6 builds the first user of it; the doctrine "no developer-supplied relays for a publish" (`docs/aim.md` §6 doctrine 5; `docs/product-spec/subsystems.md` §7.3 row "Publish leaked to wrong relays") needs a structural enforcement point.

The summary diagnosis: **the planner is a string formatter, not a compiler.** Every REQ is a per-call-site decision; routing is one of two literals; recompilation is impossible because nothing is compiled. The diagnostics in `crates/nmp-core/src/kernel/mod.rs:117-154` already type `RelayStatus` / `WireSubscriptionStatus` / `LogicalInterestStatus` per ADR-0007 — but the planner currently emits at most one `LogicalInterestStatus` per view kind because there is no logical-interest object to scope it against.

## 2. The logical interest model

A **logical interest** is the actor-internal, semantics-preserving description of what a view, action, or monitor wants the kernel to keep alive on the wire. It is the input to compilation. It is *not* a Nostr filter (a filter is one possible wire artifact a plan can produce — `docs/design/ndk-applesauce-lessons.md` §7 lines 89–90).

### 2.1 Formal shape

```rust
// crates/nmp-core/src/kernel/planner/interest.rs (proposed)

/// A logical interest is what a kernel-side consumer (view, action, monitor,
/// sync job, pointer loader) wants alive on the wire. The compiler turns N
/// logical interests into M ≤ N per-relay plans.
#[derive(Clone, Debug, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct LogicalInterest {
    /// Stable identity assigned by the registry on first insertion. Survives
    /// recompilation. Two interests with identical content collide on hash but
    /// keep distinct ids if they were registered by distinct claims.
    pub id: InterestId,

    /// Scope decides how mailbox resolution and indexer fallback behave.
    /// Account-scoped interests resolve via the active account's mailbox view
    /// for ambiguous filters (e.g. interests with no `authors` and no `#p`).
    pub scope: InterestScope,

    /// What the consumer wants. This is a normalised filter set, not a Nostr
    /// wire filter. Tags use sorted vec representation so equality is stable.
    pub shape: InterestShape,

    /// Routing hints the consumer wants honoured. The compiler may ignore
    /// hints if they conflict with policy (e.g. private-publish privacy).
    pub hints: Vec<RelayHint>,

    /// Lifecycle: tailing means "stay open after EOSE"; one_shot closes on
    /// EOSE. Window is the planner's intent, not necessarily the relay
    /// `since`/`until` it ends up emitting.
    pub lifecycle: InterestLifecycle,
}

pub enum InterestScope {
    /// Bound to the active account in SessionState. Re-routes on account switch.
    ActiveAccount,
    /// Bound to a specific account (multi-account UIs, M8). Re-routes on that
    /// account's mailbox refresh; ignored on account switch.
    Account(AccountId),
    /// No account context. Used for global pointer loaders, NIP-19-driven
    /// fetches, and indexer-direct probes.
    Global,
}

/// A parameterized-replaceable event coordinate: the triple that uniquely
/// identifies an addressable event (kinds 10000–19999, 30000–39999) across
/// all relays. Equivalent to the `naddr` bech32 encoding without the relay hint.
///
/// Helper constructors (proposed in `nmp-nip19`):
///   `NaddrCoord::from_naddr_bech32(s: &str) -> Result<Self, Nip19Error>`
///   `fn to_naddr_bech32(&self, relay_hint: Option<&RelayUrl>) -> String`
///
/// Used in D8 substrate invariant: the composite reverse index extends to
/// address pointers so thread/comment read models and meta-subscribe-style
/// projections share one REQ per relay.
#[derive(Clone, Debug, Hash, Eq, PartialEq, Ord, PartialOrd,
         Serialize, Deserialize)]
pub struct NaddrCoord {
    pub pubkey: Pubkey,   // author of the addressed event
    pub kind:   u32,      // addressable kind (matches BTreeSet<u32> shape)
    pub d_tag:  String,   // the `d` tag value; empty string for kind:0
}

pub struct InterestShape {
    pub authors:    BTreeSet<Pubkey>,        // empty = wildcard
    pub kinds:      BTreeSet<u32>,           // empty = wildcard (rare)
    pub tags:       BTreeMap<TagKey, BTreeSet<String>>,  // sorted for hash stability
    pub since:      Option<UnixSeconds>,
    pub until:      Option<UnixSeconds>,
    pub limit:      Option<u32>,
    pub event_ids:  BTreeSet<EventId>,       // for pointer/thread hydration
    /// Parameterized-replaceable event coordinates for address-pointer hydration.
    /// Non-empty when a read model needs to resolve a specific `naddr` (e.g. a
    /// NIP-23 article referenced by a thread/comment projection). The compiler routes
    /// each coordinate to the addressed author's write relays (Stage 1 Outbox
    /// direction keyed on `NaddrCoord::pubkey`). See §3.3 Rule 8 and §7.
    /// Rationale: T21 research (NDK `$metaSubscribe` / svelte subscription
    /// grouping — `docs/research/ndk/subscription-compilation.md` §Grouping)
    /// shows filter-key-set identity drives merge eligibility; adding `addresses`
    /// as a first-class field gives the merge lattice a stable key to union on
    /// rather than encoding coords into opaque `#a` tag strings.
    pub addresses:  BTreeSet<NaddrCoord>,    // empty = no address-pointer hydration
}

pub enum InterestLifecycle {
    Tailing,                                   // stays open after EOSE
    OneShot,                                   // CLOSE on EOSE
    BoundedTime { until_ms: u64 },             // CLOSE on EOSE or deadline
}
```

`InterestShape` mirrors the Nostr filter shape closely on purpose: most logical interests correspond directly to a single filter, and the kernel ships canonical normalisation (sort, dedup, fold ranges) so equality and hashing are deterministic. The compiler is then free to merge two shapes (or refuse to) on the basis of structural compatibility (§3 step 3).

### 2.2 How current code expresses interests

The proposed runtime `ViewModule` registry did not ship. Current NMP code opens
interests through typed feed sessions, ref/dependent-interest claims, event
observers, and registered projections. The planner still receives the same
kind of normalized `LogicalInterest` data; the ownership boundary changed.

Concrete current examples:

- `FeedParams { primary_kinds: {1}, acquisition:
  FeedScope::ActiveUserFollows, ... }` is an app-facing feed declaration. The
  NIP-02/NIP-18/defaults composition reduces that source into acquisition
  interests over the active account's current follows with derived wrapper
  kinds. The app declares `{1}`, not `{1, 6}` and not a concrete follow list.
- `FeedScope::Authors { ... }` and `FeedScope::Referrer { ... }` compile to
  dynamic FlatFeed sidecars through `open_feed`, with close-by-handle teardown.
- `resolve_ref` / profile and event claims express component/read-model
  dependent interests. They are refcounted and deduped by the kernel; native
  components do not construct filters.
- NIP-51 list sources are ReducedSource reducers above the planner. Current
  defaults include kind:30000 people-list members and the active account's
  public kind:10000 mute-list `p` tags; follow-pack reducers follow the same
  shape when added. Protocol/defaults code reduces the source event(s) to
  pubkeys/tags/ids, then materializes normal interests.
- Pointer/meta-subscribe-style views should follow the same rule: pointer
  streams and referenced targets become dependent interests or refs that route
  through the planner. They must not use out-of-band fetches or revive the
  removed runtime reducer surface.

The app-facing feed declarations name primary content kinds (`{1}` here) and a
reactive source. The `{1, 6}` shapes above are compiled acquisition output, not
what the app asks for and not substrate defaults.

**Worked example — address-pointer dedup across thread/comment and
meta-subscribe projections:**

```
Thread/comment read-model code for kind:1111 comment on kind:30023 article →
  hydrate interest { addresses: {(article_pk, 30023, "slug")} }
Highlights/engagement meta-subscribe projection →
  hydrate interest { addresses: {(article_pk, 30023, "slug")} }
Compiler Stage 1: both coords resolve to article_pk's write relays.
Compiler Stage 3: Rule 7 (§3.3) unions the address sets (identical here).
Result: ONE REQ per relay carrying { #a: ["30023:<article_pk>:slug"] }.
```

This is the D8 substrate invariant applied to address pointers: the composite reverse index, when extended to `NaddrCoord`, deduplicates across views without any view-module coordination.

The seed-bootstrap path (`crates/nmp-core/src/kernel/requests.rs:50-106`) becomes one `LogicalInterest` per concern, registered at actor `Start` rather than emitted as raw REQs. The compiler produces the wire artifacts.

### 2.3 Account scope binding

The kernel `SessionState` (`docs/product-spec/subsystems.md` §7.4) carries an
active account id. `InterestScope::ActiveAccount` resolves at compile time, not
at registration time. On account switch (§4 trigger A4), the compiler
re-evaluates every `ActiveAccount`-scoped interest against the new active
account's mailbox view. This is the structural enforcement of account-context
overlap: the kernel cannot "forget" to re-route because every plan re-derives
from the active scope.

Account-scoped interests with empty `authors` and empty `#p` (e.g. a free-form hashtag firehose) resolve against the active account's *read relays* (NIP-65 read side) — the user's own subscription preferences, not a globally hardcoded relay. Today's `firehose_requests()` at `crates/nmp-core/src/kernel/requests.rs:357-372` hardcodes `RelayRole::Content`; under the compiler this becomes "active-account read relays, falling back to indexer set if the active account has no kind:10002."

### 2.4 What is *not* a logical interest

To keep the surface small, the following are explicitly **not** logical interests:

- A **wire REQ**. Wire REQs are produced by the compiler; they live in `WireSubscriptionStatus` per ADR-0007.
- A **publish**. Publishes are durable actions on the action ledger (`docs/design/kernel-substrate.md` §4); they consult the `PublishPlanner` (§7) but they are not interests because they do not stay alive.
- A **diagnostic record**. ADR-0007 lanes are facts derived from the planner's state, not inputs.
- An **HTTP fetch** (Blossom upload, indexer JSON probe). Those are `CapabilityModule` requests.

The boundary is intentional: an interest is anything that asks the planner to *keep a REQ open*. Everything else routes through a different seam.
