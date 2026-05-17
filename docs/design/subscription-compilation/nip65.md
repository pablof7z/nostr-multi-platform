# Subscription Compilation §6 — `nmp-nip65` Crate Layout

> Parent: `docs/design/subscription-compilation.md`.
> Read first: `docs/design/kernel-substrate.md` §3 (`ViewModule`) for the trait this crate implements; `docs/design/app-extension-kernel.md` §3 layering table — `nmp-nip65` is a **protocol module**, not an app module.

`nmp-nip65` is the first NMP protocol module (per the v1 reference-modules list in `docs/design/kernel-substrate.md` §11) whose job is *not* to expose product views. It contributes one `ViewModule` (for app-side rendering of "this user's relay list") and implements the NIP-65 parsing logic. It is **not** the canonical source of the mailbox cache — that trait lives in `nmp-core` to avoid a dependency cycle.

**Crate dependency resolution (D0):** `nmp-core::kernel::planner` must consume the mailbox
cache, but it cannot import from `nmp-nip65` (that would create a cycle: `nmp-nip65` →
`nmp-core` → `nmp-nip65`). Resolution: the `MailboxCache` trait and associated types
(`MailboxSnapshot`, `CachePutResult`) live in `nmp-core::substrate::mailbox` (or a tiny
zero-dependency `nmp-mailbox-types` crate). `nmp-nip65` implements the trait via
`InMemoryMailboxCache` and registers the kind:10002 ingest handler; `nmp-core` declares the
trait and consumes it — no cycle. This is consistent with D0: the kernel defines the seam,
the module fills it.

## 6.1 File structure

```
crates/nmp-nip65/
├── Cargo.toml
├── README.md
├── src/
│   ├── lib.rs              # re-exports + crate-level documentation
│   ├── module.rs           # impl ViewModule for MailboxesView
│   ├── parse.rs            # kind:10002 tag parsing (extracted from kernel)
│   ├── cache.rs            # MailboxCache trait + InMemory impl
│   ├── routing.rs          # AuthorRouting, RoutingSource, mailbox lookup API
│   └── tests/
│       ├── parse.rs        # round-trip + edge-case tag parsing
│       ├── routing.rs      # mailbox → relay-set resolution scenarios
│       └── snapshot.rs     # cache snapshot/restore for compiler-input tests
└── tests/                  # integration tests against the in-memory cache
```

Soft target per file: ≤ 300 LOC (AGENTS.md). The crate stays small; everything heavier (filter compilation, indexer probes) lives in `nmp-core::kernel::planner`, not here.

## 6.2 Traits implemented

`nmp-nip65` implements exactly one extension trait family: `ViewModule`. It does *not* implement `ActionModule` (kind:10002 publish is the user's own "update my relay list" action, deferred to M6's action ledger; in this milestone it has no write surface). It does *not* implement `DomainModule` (mailbox records live in the kernel-owned mailbox cache; they are queryable Nostr events, not durable app-defined records).

### `MailboxesView` (`impl ViewModule`)

```rust
// crates/nmp-nip65/src/module.rs

pub struct MailboxesView;

#[derive(Clone, Serialize, Deserialize, Hash, Eq, PartialEq)]
pub struct MailboxesSpec {
    pub pubkey: Pubkey,
}

#[derive(Clone, Serialize)]
pub struct MailboxesPayload {
    pub pubkey: Pubkey,
    pub read:  Vec<RelayUrl>,
    pub write: Vec<RelayUrl>,
    pub both:  Vec<RelayUrl>,
    pub created_at: UnixSeconds,           // 0 if unknown
    pub source: MailboxSource,
}

pub enum MailboxSource {
    Cached    { freshness: FreshnessHint },
    Fetching,
    Unknown,
}

impl ViewModule for MailboxesView {
    const NAMESPACE: &'static str = "nip65.mailboxes";
    type Spec    = MailboxesSpec;
    type Payload = MailboxesPayload;
    type Delta   = MailboxesPayload;       // payloads are small; emit whole snapshots
    type Key     = Pubkey;
    type State   = MailboxesPayload;

    fn key(spec: &MailboxesSpec) -> Pubkey {
        spec.pubkey.clone()
    }

    fn dependencies(spec: &MailboxesSpec) -> ViewDependencies {
        ViewDependencies::author_kind(&spec.pubkey, 10002)
    }

    fn interests(spec: &MailboxesSpec, ctx: &InterestContext) -> Vec<LogicalInterest> {
        vec![LogicalInterest {
            id: ctx.fresh_id(),
            scope: InterestScope::Global,
            shape: InterestShape {
                authors: btreeset![spec.pubkey.clone()],
                kinds:   btreeset![10002],
                limit:   Some(1),
                ..Default::default()
            },
            hints: vec![],
            lifecycle: InterestLifecycle::OneShot,
        }]
    }

    fn open(ctx: &ViewContext, spec: MailboxesSpec) -> (Self::State, Self::Payload) {
        let snapshot = ctx.mailbox_cache().get(&spec.pubkey);
        let payload = MailboxesPayload::from_snapshot(spec.pubkey, snapshot);
        (payload.clone(), payload)
    }

    fn on_event_inserted(ctx: &ViewContext, st: &mut Self::State, ev: &Event)
        -> Option<Self::Delta>
    {
        // D4: single writer per fact. The canonical mailbox write path is
        // `ingest_relay_list` → `MailboxCache::put`. This view MUST NOT re-parse
        // kind:10002 tags independently — doing so would make two writers for the
        // same fact (the cache writer and the view's parse). Instead, read the
        // canonical snapshot from the cache that was already updated by ingest.
        if ev.kind != 10002 || ev.pubkey != st.pubkey { return None; }
        let snapshot = ctx.mailbox_cache().get(&st.pubkey)?;
        if snapshot.created_at <= st.created_at { return None; }
        *st = MailboxesPayload::from_snapshot(st.pubkey.clone(), Some(snapshot));
        Some(st.clone())
    }

    // on_event_removed / replaced / projection_changed / on_tick: defaults
    fn snapshot(_ctx: &ViewContext, st: &Self::State) -> Self::Payload {
        st.clone()
    }
}
```

The view exists so platform code can render "alice@example uses these relays" using the same path as any other view (`useMailboxes(pubkey)`); it is *not* the compiler's source of truth. The compiler reads `MailboxCache` directly.

## 6.3 Public surface (compiler-facing, not FFI-facing)

The `MailboxCache` trait and its associated types live in **`nmp-core::substrate::mailbox`**
to break the dependency cycle (see §6.1 intro). `nmp-nip65` provides the `InMemoryMailboxCache`
implementation; `nmp-core::kernel::planner` consumes only the trait, never the concrete type.

```rust
// crates/nmp-core/src/substrate/mailbox.rs  ← trait lives here, not in nmp-nip65

pub trait MailboxCache: Send + Sync {
    fn get(&self, pubkey: &Pubkey) -> Option<MailboxSnapshot>;
    fn put(&mut self, pubkey: Pubkey, snapshot: MailboxSnapshot)
        -> CachePutResult;
    fn snapshot_all(&self) -> Vec<(Pubkey, MailboxSnapshot)>;
    fn generation(&self) -> u64;           // monotonic; advances on every accepted put
}

pub enum CachePutResult {
    Inserted,
    ReplacedNewer { prior_created_at: UnixSeconds },
    /// NIP-01 tie-break: equal `created_at` keeps the event with the lexicographically
    /// lower event id, per Applesauce gotcha `90d525af`
    /// (`docs/research/applesauce/gotchas.md` §G1). Two clients producing kind:10002
    /// in the same second must not silently disagree on the stored version.
    RejectedStale { current_created_at: UnixSeconds },
}

#[derive(Clone, Debug)]
pub struct MailboxSnapshot {
    pub kind10002_event_id: EventId,
    pub created_at: UnixSeconds,
    pub read:  Vec<RelayUrl>,
    pub write: Vec<RelayUrl>,
    pub both:  Vec<RelayUrl>,
    pub seen_from: Vec<RelayUrl>,          // ProvenanceRelayFact seed
}
```

```rust
// crates/nmp-nip65/src/cache.rs  ← concrete impl lives here

pub struct InMemoryMailboxCache { /* HashMap<Pubkey, MailboxSnapshot> */ }
impl MailboxCache for InMemoryMailboxCache { /* ... */ }
```

The `MailboxCache` trait is the seam M3 (LMDB persistence) replaces with a backing-store-aware implementation. The compiler does not know which backend it is reading.

```rust
// crates/nmp-nip65/src/routing.rs

pub fn resolve_author_outbox(
    cache: &dyn MailboxCache,
    user_configured: &UserConfiguredRelays,
    indexer_set: &[RelayUrl],
    author: &Pubkey,
) -> AuthorRouting { /* ... */ }

pub fn resolve_author_inbox(
    cache: &dyn MailboxCache,
    user_configured: &UserConfiguredRelays,
    indexer_set: &[RelayUrl],
    author: &Pubkey,
) -> AuthorRouting { /* ... */ }
```

These are the two pure functions [compiler.md](compiler.md) Stage 1 calls per author. They return `AuthorRouting` with the `RoutingSource` tag set per the four-lane discipline ([diagnostics.md](diagnostics.md) §5.2). Test fixtures live in `crates/nmp-nip65/src/tests/routing.rs`; the same fixtures plug into the audit gate (§9).

The `resolve_author_outbox_no_indexer` variant used by the publish planner (§7) intentionally
omits the indexer set parameter — compare NDK's `chooseRelayCombinationForPubkeys`
(`docs/research/ndk/outbox.md` §"Big picture" → `core/src/outbox/index.ts:45`) which is
the nearest equivalent for reads, and Applesauce's `OutboxModel` which delegates relay
resolution to every caller rather than enforcing publish-vs-read separation
(`docs/research/applesauce/outbox.md` §7 lines 116-138).

```rust
// crates/nmp-nip65/src/parse.rs

pub fn parse_relay_list(created_at: UnixSeconds, tags: &[Vec<String>])
    -> ParsedRelayList;
```

This is the function currently inlined as a free fn in `crates/nmp-core/src/kernel/nostr.rs` (referenced by `kernel/ingest.rs:210` and tested in `kernel/tests.rs:150`). M2 moves it here and re-exports from `nmp-core` for compatibility during the migration.

## 6.4 What `nmp-nip65` does *not* contain

By design, to keep the kernel boundary clean (per `docs/design/app-extension-kernel.md` §3):

- **No publish action.** Updating a user's own kind:10002 is `nmp-nip01::UpdateRelayList` (M6); that action depends on `nmp-nip65::parse` to validate the local copy before publishing.
- **No outbox routing policy.** The decision "publish goes to author write relays + recipient inbox relays" is the publish planner ([outbox.md](outbox.md) §7), not this crate. This crate provides the lookups; the policy lives in `nmp-core::kernel::planner::publish`.
- **No indexer-probe scheduling.** Probes are kernel-side; this crate is unaware of probe lifecycle.
- **No FFI types.** `MailboxesPayload` is exposed at FFI via the per-app generated enum (per ADR-0010 codegen); the crate itself ships pure Rust.

## 6.5 Module composition (per `docs/design/kernel-substrate.md` §8)

`nmp-nip65` consumes:

- `nmp-core::substrate::{ViewModule, ViewContext, InterestContext, LogicalInterest,
  MailboxCache, MailboxSnapshot, CachePutResult, ...}` — kernel trait surface including the
  mailbox types (trait lives in core, not in this crate).
- `nmp-core::kernel::projections` — for reading kind:10002 events out of the event store.

`nmp-nip65` is consumed by:

- The per-app generated enum — `MailboxesView` becomes one variant of `ViewSpec` in
  `nmp-app-<name>` per ADR-0010.
- Future `nmp-nip01::UpdateRelayList` (M6) — imports `parse::parse_relay_list` for validation.
- Future `nmp-nip17` (M9) — DM publish path imports `routing::resolve_author_inbox`.

`nmp-nip65` is **not** consumed by `nmp-core::kernel::planner`. The planner imports
`nmp-core::substrate::mailbox::{MailboxCache, ...}` directly (no nmp-nip65 dep). The
concrete `InMemoryMailboxCache` is injected at app startup via `AppConfig`; the planner
receives a `&dyn MailboxCache` reference.

## 6.6 Cargo manifest sketch

```toml
[package]
name = "nmp-nip65"
version = "0.0.1"
edition = "2021"

[dependencies]
nmp-core   = { path = "../nmp-core" }
serde      = { workspace = true, features = ["derive"] }
serde_json = { workspace = true }

[dev-dependencies]
nmp-testing = { path = "../nmp-testing" }
```

No `nostr-sdk` dependency: this crate operates on parsed `Event` structs from `nmp-core`'s already-vetted ingest path. Avoiding a duplicate parse dependency keeps the surface auditable.
