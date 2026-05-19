# Design Note: Kind-Specific Event Wrappers (NDK pattern vs NMP)

> **Status:** Draft — design recommendation (no impl). T65.
> **Date:** 2026-05-18
> **Scope:** Should NMP ship NDK-style typed event wrappers (`NDKArticle`, `NDKWiki`, `NDKHighlight`, …)? If yes, in what shape?
> **Reads:** NDK `core/src/events/kinds/` (~37 wrapper files), applesauce `packages/core/src/{helpers,factories,models}/`, NMP doctrine D0–D8 (`docs/product-spec/doctrine.md`), ADR-0009 (kernel boundary), ADR-0010 (per-app concrete enums), `docs/research/sessions/synthesis.md`.

## 1. Recommendation

**Yes — but neither NDK's class-as-wrapper pattern nor a kernel-blessed mega-crate.** NMP adopts applesauce's split as the architectural shape, refactored into Rust idiom:

- **Decoders** — pure `fn decode(&Event) -> Option<Record>` functions that produce already-typed `DomainRecord`s (`crates/nmp-nip29/src/domain/records.rs:30-100` is the precedent).
- **Blueprints** — `Builder → UnsignedEvent` constructors, signed and published through the existing action ledger.

Both live in **protocol-module crates** (Option A) for NIP-defined kinds and **app-core crates** (Option B) for app-specific kinds. There is no `nmp-kinds` mega-crate (Option C). There are **no read-side mutable wrapper classes** at all — NDK's `article.title = "foo"` setter pattern violates D4 (single writer per fact) by giving every caller a phantom writer over shared state.

The kernel itself ships **no decoders** — every kind, including the universal 0 / 3 / 10002, lives in its protocol module (`nmp-nip01`, `nmp-nip02`, `nmp-nip65`). The kernel retains only the dispatch table that routes events to registered modules. The currently kernel-resident handlers (`crates/nmp-core/src/kernel/ingest/{profile,contacts,relay_list,timeline}.rs`) are extracted in Phase 1 — see §6 + §8.

Three doctrines force this shape and they were not invented for this design — they predate it:

- **D0** forbids `NDKArticle` in `nmp-core`.
- **D4** forbids the NDK mutate-tags-via-setter pattern; only the actor writes.
- **D5** makes read-side wrappers largely redundant — views project typed `ViewPayload`s already; "raw event in hand → typed accessor" is a narrow ingest-side need, not a view-side one.

## 2. Where wrappers live

| Option | Verdict | Why |
|---|---|---|
| A. In protocol modules (`nmp-nip23::Article`, `nmp-nip54::Wiki`, `nmp-nip84::Highlight`) | **Accepted** for protocol-defined kinds | Matches ADR-0009 layering. Same crate owns the kind's wire format, its decode/encode, its domain record, its `DomainModule` registration. One change radius per NIP. |
| B. In app-core crates (`podcast-core::EpisodeRecord`, highlighter app records) | **Accepted** for app-specific kinds | When the wire format is a local invention not standardized as a NIP. The app-core boundaries already exist at `apps/podcast/podcast-core/src/domain/records.rs:1-80` and `crates/nmp-highlighter-core/src/lib.rs:1-25`; concrete highlighter records are still scaffolded. |
| C. Shared `nmp-kinds` library | **Rejected** | Recreates the junk-drawer that ADR-0009 partitioned away. Forces every app to compile every wrapper. Inverts the protocol-module dependency graph (the bookmarks crate would have to know about wiki). |
| D. Blueprints only (no decoders) | **Rejected** | The ingest path needs typed decoding to populate domain records — see §6. Encode-only is half a feature. |

## 3. The shape of a wrapper (worked example: kind 30023)

`nmp-nip23` is the (not-yet-created) crate that owns NIP-23 long-form. The wrapper has **two halves with no shared mutable state**.

### 3.1 Decoder half (read side)

```rust
// crates/nmp-nip23/src/decode.rs
use nmp_core::store::StoredEvent;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ArticleRecord {
    pub event_id: String,
    pub author: String,
    pub d_tag: String,                  // identifier
    pub title: Option<String>,
    pub image: Option<String>,
    pub summary: Option<String>,
    pub published_at: Option<u64>,      // unix seconds, normalised from ms
    pub created_at: u64,
    pub content: String,                // markdown body
}

/// Pure, allocation-bounded, no I/O. Returns None if `event.kind != 30023`.
pub fn decode_article(event: &StoredEvent) -> Option<ArticleRecord> { /* … */ }
```

The decoder is the inverse of NDK's `get title()` getter (`/Users/pablofernandez/Work/NDK-nhlteu/core/src/events/kinds/article.ts:35-37`), but it runs **once at ingest** and yields an immutable record, not on every UI access against a mutable bag of tags.

### 3.2 Blueprint half (write side)

```rust
// crates/nmp-nip23/src/build.rs
use nmp_core::substrate::UnsignedEvent;

pub struct ArticleBuilder {
    d_tag: String,
    title: Option<String>,
    image: Option<String>,
    summary: Option<String>,
    published_at: Option<u64>,
    content: String,
}

impl ArticleBuilder {
    pub fn new(d_tag: impl Into<String>, content: impl Into<String>) -> Self { /* … */ }
    pub fn title(mut self, v: impl Into<String>) -> Self { self.title = Some(v.into()); self }
    pub fn image(mut self, v: impl Into<String>) -> Self { self.image = Some(v.into()); self }
    pub fn summary(mut self, v: impl Into<String>) -> Self { self.summary = Some(v.into()); self }
    pub fn published_at(mut self, ts: u64) -> Self { self.published_at = Some(ts); self }

    /// Pure: tags-shape produced deterministically; no signer, no clock.
    pub fn into_unsigned(self, author: &str, created_at: u64) -> UnsignedEvent { /* … */ }
}
```

### 3.3 How it composes with the existing kernel

| Concern | Wrapper interaction |
|---|---|
| **EventStore queries** | The app calls `store.scan_by_author_kind(pk, &[30023], …)` (`crates/nmp-core/src/store/events.rs:153-160`) and pipes results through `decode_article`. **No** `store.articles_by_author(pk)` method — that would re-grow kernel app-noun surface (D0). |
| **Planner / InterestShape** | Untouched. The planner sees `BTreeSet<u32> { 30023 }` in `InterestShape.kinds` (`crates/nmp-core/src/planner/interest.rs:80`). Wrappers are **post-decode / pre-encode** — they never participate in plan-id derivation, so plan-id stability survives. |
| **Signing** | The action handler reads `account_manager.signer_active()` and passes the `UnsignedEvent` into `Signer::sign(unsigned)` (per ADR-0015 / M6). The existing signer pipeline applies its post-conditions. |
| **Publishing** | The action ledger turns the signed event into `PublishAction::Publish { target: PublishTarget::Auto, … }` and drives `PublishEngine::start_publish`. D3 routing applies — the blueprint never picks relays. |
| **Ingest routing** | `nmp-nip23` registers `Nip23DomainModule`; on `kind == 30023` ingest, the kernel dispatches into that module's `decode_and_route` hook, which calls `decode_article` and writes an `ArticleRecord` into `domain_open("nip23.article")`. See §6. |

Nothing about this requires a *class*. The wrapper is a function on the read side and a builder on the write side. The owning identity (an article authored by Alice with d-tag `foo`) is the `(author, kind, d_tag)` triple in the store, not a Rust object.

## 4. v1 kind list (priority-ordered)

P0 = ships before Twitter clone (M11-ish). P1 = follow-up protocol crates. P2 = post-v1.

| Kind(s) | Wrapper | Crate | Priority |
|---|---|---|---|
| 0 | `Profile` (decoder only — written by kind:0 action, no app builder needed beyond profile edit) | `nmp-nip01` | **P0** |
| 1 | `ShortNote` (decoder + builder) | `nmp-nip01` | **P0** |
| 3 | `Contacts` (decoder; mutation is `FollowAction`/`UnfollowAction`, not a builder) | `nmp-nip02` | **P0** |
| 5 | `Deletion` (decoder + builder) | `nmp-nip09` | **P0** |
| 6, 16 | `Repost`, `GenericRepost` | `nmp-nip18` | **P0** |
| 7 | `Reaction` | `nmp-nip25` | **P0** |
| 10002 | `RelayList` (decoder only — written by `UpdateRelayListAction`) | `nmp-nip65` | **P0** |
| 4 / 1059 / 14 | `LegacyDm` / `GiftWrap` / `Rumor` | `nmp-nip17` | **P0** for messenger app, P1 otherwise |
| 30023 | `Article`, `ArticleBuilder` | `nmp-nip23` (new) | **P1** |
| 30818 | `Wiki`, `WikiBuilder` | `nmp-nip54` (new) | **P1** |
| 9802 | `Highlight`, `HighlightBuilder` | `nmp-nip84` (new) | **P1** |
| 9 / 11 / 39000-39003 | Group chat / discussion / metadata | `nmp-nip29` (exists) | **P0** records/modules done; ingest decoders pending §6 |
| 10000 / 10001 / 30000-39999 NIP-51 list family | `MuteList`, `BookmarkList`, `InterestList`, `RelayFeedList`, … | `nmp-nip51` (new) | **P1** |
| 20, 21, 22 | `Image`, `Video`, `ShortVideo` | `nmp-nip68` / `nmp-nip71` | **P2** |
| 31234 | `Draft` | `nmp-nip37` | **P2** |
| 10063 | `BlossomList` | `nmp-blossom` | **P2** |
| 7375, 17375, 9321, 10019 | Cashu token, wallet, nutzap, nutzap-info | `nmp-nip60` / `nmp-nip61` | **P2** |
| 23196 (podcast) / app-local | `EpisodeRecord`, `WeightLog`, `ReadsFeedEntry` | `podcast-core`, `wtd-core`, `highlighter-core` | per-app cadence |

Baseline P0 protocol-module wrapper surface is seven crates before conditional messenger support, with NIP-29 already split into records/modules but its ingest decoders waiting on the hook in §6. **An order of magnitude smaller than NDK's 30+** (`/Users/pablofernandez/Work/NDK-nhlteu/core/src/events/wrap.ts:82-114`) because (a) per-NIP partitioning forces small crates and (b) views deliver most of the typing already.

## 5. Opt-out path

Per ADR-0010, every app declares its modules in `nmp.toml`:

```toml
[modules]
protocol = ["nmp-nip01", "nmp-nip02", "nmp-nip25"]   # no nip23, no nip54
app      = ["my-microblog-core"]
```

A microblog app that doesn't depend on `nmp-nip23` pays **zero code weight** for `ArticleRecord` / `ArticleBuilder`. The opt-out is the module-graph; no per-wrapper feature flags, no trait sealing, no runtime registry. The unused decoder doesn't compile in; the unused builder doesn't appear in `AppAction`; the unused `DomainModule` doesn't claim LMDB namespace. This is the same mechanism ADR-0010 §"What we get" already established — wrappers ride it for free.

## 6. The raw-event-in-hand use case (the load-bearing section)

When a relay sends `EVENT` to the kernel, `verify_and_persist` writes the raw event to the store. Some kinds also need to update derived domain records (e.g. kind:30023 → `ArticleRecord`). **The kernel cannot decide which** — D0 forbids it from knowing `kind 30023 == article`.

Resolution — extend `DomainModule` with a typed ingest hook:

```rust
// crates/nmp-core/src/substrate/domain.rs (extension)
pub trait DomainModule: Send + Sync + 'static {
    const NAMESPACE: &'static str;
    const SCHEMA_VERSION: u32;

    /// Kinds this module wants to see at ingest. Empty = no Nostr ingest (pure
    /// domain-store module like fixture-todo).
    fn ingest_kinds() -> &'static [u32] { &[] }

    /// Called once per matching event after `verify_and_persist` succeeds.
    /// Pure decode + single-handle write; never publishes, never queries the wire.
    fn decode_and_route(event: &StoredEvent, handle: &DomainHandle) -> Result<(), StoreError> {
        let _ = (event, handle); Ok(())
    }

    fn migrations() -> Vec<DomainMigration>;
    fn indexes() -> Vec<DomainIndex>;
    fn register(registry: &mut DomainRegistry);
}
```

The kernel maintains a `u32 → Vec<ModuleId>` map built from each registered module's `ingest_kinds()`. On ingest, it dispatches:

```rust
for module_id in self.ingest_dispatch.get(&event.kind).iter().flatten() {
    let handle = self.store.domain_open(module_id.namespace())?;
    module_id.decode_and_route(&event, &handle)?;
}
```

Per **D4**, exactly one module owns each `(kind, optional discriminator)` pair — e.g. `nmp-nip29::GroupHighlightModule` owns kind 9802 *with* an `h` tag, while a future `nmp-nip84::HighlightModule` owns kind 9802 *without* an `h` tag. The discriminator is the module's business (NDK punts this — see anti-pattern §9). Conflicting module sets are rejected by `nmp gen modules`, with startup assertions as a direct-registry backstop.

**The current D0 loophole.** The kernel's existing ingest handlers — `crates/nmp-core/src/kernel/ingest/{profile,contacts,relay_list,timeline}.rs` — handle kinds 0 / 3 / 10002 / 1 directly. These exist for legitimate kernel needs (D3 outbox routing needs the relay list; identity bootstrap needs the profile; `ActiveAccount` rendering needs both) **but they are still D0 violations** — the kernel is decoding app nouns. Phase 1 (§8) closes this by **extracting them into `nmp-nip01` / `nmp-nip02` / `nmp-nip65`** as the first consumers of the new `DomainModule::ingest_kinds` dispatch. The extraction is mechanical (each ingest function is 30–80 LOC); the kernel keeps the dispatch table and nothing else. The kernel's *consumers* of the decoded data (e.g. the outbox planner reading the latest kind:10002) call into the nip65 module's read API — the kernel still doesn't know what a `RelayList` is, it asks the module.

## 7. NDK vs applesauce vs NMP scorecard

| Dimension | NDK (`core/src/events/kinds/`) | applesauce (`core/src/{helpers,factories}/`) | NMP (this doc) |
|---|---|---|---|
| Read pattern | mutable class extends Event, lazy getters | pure decoder fns over `NostrEvent` (`helpers/profile.ts:52-69`) | pure decoder fns → immutable `DomainRecord` |
| Write pattern | same class; `setter` mutates `this.tags` (`kinds/article.ts:44-48`) | separate `EventFactory`/`ProfileFactory` chain (`factories/profile.ts:10-97`) | separate `XxxBuilder` → `UnsignedEvent` |
| Where wrappers live | one mega-package; central registry in `wrap.ts:82-114` | split by concern (helpers, factories, models) in `packages/core` | one per protocol crate / app crate |
| Mutability of "the wrapped event" | yes, intentionally (the wrapper IS the event) | no — `NostrEvent` immutable, decoders cached on a symbol (`helpers/profile.ts:55`) | no — decoders return owned `DomainRecord` |
| Cross-kind inheritance | yes (`NDKWiki extends NDKArticle` `kinds/wiki.ts:10`) | no | **forbidden** — see §9 |
| Number of wrapper "types" | ~30+ classes registered in one place | ~12 helper modules, ~5 factories | ~7 crates at P0; ~3 builders each |
| Compile-time exclusion of unused kinds | no (all imported by `wrap.ts`) | per-import (tree-shake-friendly) | per-crate (cargo dep graph) |
| FFI-friendly | no (classes don't UniFFI) | n/a (no FFI) | yes (records + builders are plain serde) |

NDK's pattern is **TypeScript-shaped**: dynamic dispatch through a runtime class table, mutation via setters, `instanceof` checks at consumer sites. NMP is Rust + UniFFI + D4 + D5; copying the shape would import the friction without the (dynamic-language) ergonomic payoff.

## 8. Migration / adoption path

| Phase | Deliverable | Owner |
|---|---|---|
| **Phase 1 (now / M11-ish)** | `DomainModule::ingest_kinds` + `decode_and_route` hook; kernel dispatch table; extraction of profile/contacts/relay_list/timeline ingest from `nmp-core` into nascent `nmp-nip01`/`nip02`/`nip65`. | Kernel team. ~400 LOC kernel-side. |
| **Phase 2 (M12-13)** | `nmp-nip23::Article{,Builder}`, `nmp-nip51` list family, `nmp-nip17` DM wrappers. Twitter clone consumes them. | Per-NIP crate authors. |
| **Phase 3 (post-v1)** | `nmp-nip54::Wiki`, `nmp-nip84::Highlight`, `nmp-nip68/71` image/video, `nmp-nip37::Draft`, `nmp-nip60/61` cashu/nutzap. App-core wrappers grow alongside their apps. | Protocol authors as needs arise. |

Each phase ships behind real apps, never speculatively. NDK's mistake is the inverse — `wrap.ts` registers 30+ wrappers that any single app uses ~3 of.

## 9. Anti-patterns to forbid

1. **`Wrapper extends OtherWrapper`** — `NDKWiki extends NDKArticle` (`kinds/wiki.ts:10`). A wiki is not a subtype of an article; sharing a getter table by inheritance bakes accidental coupling into the wire-format change radius.
2. **Setters that mutate tag arrays** — `set title(v) { this.removeTag('title'); this.tags.push(['title', v]); }` (`kinds/article.ts:44-48`). Violates D4. Builders that consume `self` are the Rust idiom.
3. **One-class-many-kinds with `static kinds[]`** — `NDKList` covers 11 different list kinds (`kinds/lists/index.ts:35-47`) with `kind`-discriminated branching inside getters. Each kind deserves its own decoder; pretending they're a "list family" hides per-kind invariants (e.g. encrypted vs public tag handling differs by kind).
4. **Centralized wrapper registry** — `wrap.ts` (`/Users/pablofernandez/Work/NDK-nhlteu/core/src/events/wrap.ts:82-114`) is a hard import of every wrapper. Adding a new kind requires editing the central file. The cargo dep graph + ADR-0010 codegen replace this; never recreate it.
5. **Async getters that hit the network** — `NDKHighlight.getArticle()` (`kinds/highlight.ts:75-115`) does `await this.ndk?.fetchEvent(…)`. Mixing decode with I/O violates D8's hot-path discipline and makes wrappers untestable as pure functions.
6. **Side-effecting setters** — `NDKHighlight.set article` writes both `this._article = article` *and* `this.tags.push(['r', article])` (`kinds/highlight.ts:61-69`). Two writers per fact → D4 violation.
7. **Type-erased registration** — `registerEventClass(class)` (`wrap.ts:56-58`) defers conflicts to runtime. NMP catches them at `nmp gen modules` time.
8. **Lazy `published_at` normalisation in the getter** (ms→s coercion `kinds/article.ts:91-93`) — normalise once at decode, store canonical form, never re-coerce on read.
9. **Wrapper holds an `ndk` reference** — `constructor(ndk: NDK | …)` (`kinds/article.ts:15`). Decoders take `&StoredEvent`, not the world.
10. **Wrappers exposed across FFI** — UniFFI doesn't do classes-with-getters cleanly; expose decoded `Record`s as plain serde structs (already the NMP norm for `GroupChatMessageRecord` et al.).

## 10. Risk register

| If we DON'T do this | If we DO do this |
|---|---|
| Every app re-implements `parse_kind_30023` ad-hoc. Five copies, four bugs. | One `decode_article` per kind, tested once. |
| Long-form clients embed tag-parsing in view code, breaking D8 (allocs in hot path). | Decoder runs once at ingest; views read `ArticleRecord`. |
| FFI surface grows piecemeal — each app exposes its own `Article` Swift struct. | Per-protocol crate exposes one canonical record across all consuming apps. |
| The `nmp-core` ingest handlers (profile, contacts, …) stay glued in, papering over D0. | Phase 1 extracts them; D0 enforcement becomes mechanical. |
| **Cost of doing this:** ~400 LOC kernel + 200 LOC per protocol crate × 7 crates ≈ 1800 LOC; one new `DomainModule` trait method to migrate existing modules. | |
| **Cost of NOT doing this:** every app pays the tag-parsing tax forever; "few hundred lines" budget (`overview-and-dx.md` §3.2) blows out. | |

## 11. Decision matrix

| I want to … | Where it goes |
|---|---|
| Decode kind 30023 in my view | `nmp_nip23::decode_article(&event)` — pure fn |
| Publish a new article | `ArticleBuilder::new(d, content).title(t).into_unsigned(pk, ts)` → action ledger → signer → publish engine |
| Render an article list | A `ViewModule` in `twitter-core` (or app-core) that subscribes to `kinds: [30023]` and projects `Vec<ArticleSummary>` from `ArticleRecord`s |
| Add a brand-new kind (say, my app's kind 38500 recipe events) | New module in app-core: `RecipeRecord`, `RecipeBuilder`, `RecipeDomainModule::ingest_kinds() = &[38500]` |
| Mutate the title of an already-published article | **Not a wrapper concern.** Issue `EditArticleAction { article_addr, new_title, … }` → action handler reads the latest event, builds a *replacement* `UnsignedEvent`, publishes (NIP-23 replaceability) |
| Decode kind 9802 with `h` tag (group highlight) vs without (web highlight) | Two modules, two decoders, two namespaces. `nmp-nip29::GroupHighlightModule` owns h-tagged; `nmp-nip84::HighlightModule` owns the rest. D4 discriminator. |
| Use NIP-51 bookmarks but not lists generally | Depend on `nmp-nip51` but only consume `BookmarkRecord` / `BookmarkBuilder`. The crate exposes per-kind types, not one mega-class. |
| Get "wrap this raw event into its typed form" in one call | **Not a kernel API.** The caller knows which module they care about; module-private decoders are the call. NDK's `wrapEvent` (`wrap.ts:78-128`) is what we explicitly don't ship. |

## 12. Open questions (for orchestrator)

- **PD-006 — kernel ingest extraction timing.** Phase 1 above extracts kind 0/3/10002/1 ingest into protocol crates. Does that block M11 (Twitter), or run alongside? Recommend alongside — extract before M12 so Twitter's profile path lands on the new pattern; the M11 path already works.
- **PD-007 — `DomainModule::ingest_kinds` trait migration.** The existing 13 `nmp-nip29` `DomainModule` impls all need this method added. Default `&[]` keeps them compiling; do we want to force every existing impl to declare explicitly (no default)?
- **PD-008 — decoded-content caches.** Applesauce caches decoded profile content on a symbol on the event (`helpers/profile.ts:55`). For NMP, do decoded records live in the domain store (write at ingest, read at query) or in a derived in-memory cache (decode on demand, evict by D5)? Recommend ingest-time write — cheaper steady state, matches D8 hot-path discipline — but it costs LMDB space. Worth confirming.
- **PD-009 — codegen of UniFFI Records.** `ArticleRecord` is plain serde; do we generate UniFFI bindings for every protocol-crate record automatically (via the per-app FFI crate of ADR-0010), or do apps opt-in per-record? Recommend automatic — the per-app FFI crate already aggregates types; one more is free at the build step.
- **PD-010 — `static from()` helper.** NDK's `NDKArticle.from(event)` is convenient. Should each NMP module expose `pub fn try_from_event(&StoredEvent) -> Option<ArticleRecord>` as a uniform vocabulary, or is the per-module `decode_xxx` name clearer? Recommend uniform `try_from_event` — searchability wins.

---

**Bottom line.** Ship the *intent* of NDK's typed wrappers (apps shouldn't hand-parse tags) without the *shape* (mutable classes wrapping shared state). The applesauce decode/encode split, refactored into Rust crates and grounded in `DomainModule`, gives us typed access at the boundaries while D0/D4/D5/D8 protect the interior. The follow-up impl agent executes Phase 1 (§8 + §6) first; Phases 2–3 land as their consuming apps emerge.
