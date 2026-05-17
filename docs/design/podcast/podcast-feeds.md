# Step 1D — `apps/podcast/podcast-feeds`

> Parent: [`../podcast-app-rebuild.md`](../podcast-app-rebuild.md).
> Reference Swift sources: `Utilities/RSSParser.swift` (261 LOC), `Services/PodcastService.swift` (118 LOC), `Services/PodcastIndexService.swift` (186 LOC), `App/Config.swift` (35 LOC).
> Capabilities consumed: `HttpCapability`.

`podcast-feeds` owns feed fetching + parsing + Podcast Index API integration. It is the only crate that speaks RSS/Atom/JSON Feed/Podcasting 2.0; everyone else consumes parsed records over the kernel boundary. It has **no view modules** — discovered data is materialised by callers (`podcast-core::DiscoverViewModule`, `podcast-core::SubscribePodcast` action).

---

## A. Library choice

### A.1 Comparison

| Crate | Pros | Cons | Verdict |
|---|---|---|---|
| **`feed-rs`** | Single parser for RSS 0.9/1.0/2.0 + Atom + JSON Feed. Active. Returns a normalised `Feed { entries }`. Lightweight. | Doesn't know Podcasting 2.0 extensions (transcript, chapters, value, person, soundbite). Doesn't expose raw XML for custom-element drill-down. | **Chosen as the base.** We wrap it with a Podcasting 2.0 extension parser. |
| `rss` (crate) | Pure RSS 2.0. Mature. Exposes extensions via `iTunesItemExtension`, `Extension`. | RSS-only — separate path for Atom and JSON Feed. | Reject — wrong axis of generality. |
| `atom_syndication` | Pure Atom. | Atom-only. | Reject. |
| Hand-rolled `quick-xml` | Total control; matches the Swift `XMLParserDelegate` impl 1:1. | All the bugs the existing crates already fixed get re-found. | Reject for v1; revisit if `feed-rs` is missing something critical. |
| `feed-parser` (npm port) | None — no widely-used Rust equivalent. | n/a | n/a |

### A.2 Podcasting 2.0 extensions

`feed-rs` exposes per-entry extension elements via `Entry.media`/`Entry.itunes_ext`/`Entry.extensions`. The Podcasting 2.0 namespace (`<podcast:transcript>`, `<podcast:chapters>`, `<podcast:value>`, `<podcast:person>`, `<podcast:soundbite>`, `<podcast:location>`) is parsed by a small bespoke walker in `podcast-feeds/src/podcasting20.rs`. The Swift app does not handle Podcasting 2.0 (its `RSSParser` is RSS-only); we add it as **additive beyond strict parity**, similar to `ImportOpml`. Parity views ignore the extra fields; future UI consumes them.

```rust
pub struct Podcasting20Extensions {
    pub transcript: Option<TranscriptRef>,
    pub chapters: Option<ChaptersRef>,
    pub value: Option<ValueBlock>,
    pub persons: Vec<PersonRef>,
    pub soundbites: Vec<SoundbiteRef>,
    pub locked: Option<bool>,
    pub guid: Option<String>,
}

pub struct TranscriptRef { pub url: Url, pub mime: String, pub language: Option<String> }
pub struct ChaptersRef { pub url: Url, pub mime: String }
pub struct ValueBlock { pub model: String, pub recipients: Vec<ValueRecipient> }
pub struct ValueRecipient { pub name: String, pub address: String, pub split: u8, pub kind: String }
pub struct PersonRef { pub name: String, pub role: Option<String>, pub group: Option<String>, pub href: Option<Url>, pub img: Option<Url> }
pub struct SoundbiteRef { pub start_s: f64, pub duration_s: f64, pub title: Option<String> }
```

These ride along on the parsed `ParsedEpisode` shape without breaking parity callers.

---

## B. Streaming HTTP pull

The Swift impl uses `URLSession.shared.data(for:)` — buffered. For ≥ 1 MB feeds (the Lex Fridman Podcast feed is ~5 MB) buffering means latency. We stream:

```rust
pub struct FetchFeed { pub url: Url }
pub enum FetchFeedOutput { Parsed { podcast: ParsedPodcast } }
```

Step machine:

1. `Validating` — URL well-formed.
2. `Fetching` — `AwaitCapability(HttpCapability::GetStreaming { url, headers, max_bytes: Some(50 << 20) })`.
3. Bridge emits a stream of `Chunk { bytes }` events; the action accumulates into a `Vec<u8>` (we cannot parse incrementally — `feed-rs` is buffer-oriented).
4. On `FetchComplete`: `feed_rs::parser::parse(&bytes)` → `Podcasting20Extensions::walk(&bytes)` → emit `ParsedPodcast`.

Conditional GET (ETag / If-Modified-Since) for `RefreshFeed`: the kernel persists a `FeedFetchCache { feed_url, etag?, last_modified?, fetched_at_ms }` row per podcast. On refresh: send the headers; on 304 → emit `RefreshFeedOutput::NotModified`.

---

## C. Podcast Index API

The Podcast Index requires HMAC-SHA1 of `(api_key + api_secret + timestamp)` in the `Authorization` header. Swift impl uses Apple's `CC_SHA1`; Rust uses the `sha1` crate.

```rust
pub struct SearchPodcasts { pub query: String, pub limit: u32 }
pub struct TrendingPodcasts { pub limit: u32 }
pub struct PodcastsByCategory { pub category: String, pub limit: u32 }
```

Endpoints (matching Swift):

- `GET /api/1.0/search/byterm?q={query}&max={limit}`
- `GET /api/1.0/podcasts/trending?max={limit}`

Result shapes:

```rust
pub struct PodcastIndexPodcast {
    pub id: i64,
    pub title: String,
    pub url: Url,
    pub description: Option<String>,
    pub author: Option<String>,
    pub image: Option<Url>,
    pub categories: HashMap<String, String>,    // {id_str: name_str}
    pub newest_item_published_at_ms: Option<u64>,
}
```

API keys are read at action boot from `KeyValueStoreCapability` (`podcast.feeds.podcastindex.api_key`, `..._secret`); env-var override (`PODCAST_INDEX_API_KEY`, `_SECRET`) supported for dev. If neither is set, the action returns `Rejection::Misconfigured`; the Library `DiscoverView` shows the existing "trending unavailable" empty state.

---

## D. Static category list

`PodcastIndexCategory.all` from the Swift code is verbatim — same 19 categories with same ids. Migrated to `pub const CATEGORIES: &[(u8, &str)]` in `podcast-feeds/src/categories.rs`. The icon/color choice is UI (see `Views/Components/DiscoveryCards.swift`); it stays Swift.

---

## E. Action surface

```rust
pub enum Action {
    FetchFeed(FetchFeed),
    RefreshFeed(RefreshFeed),
    SearchPodcasts(SearchPodcasts),
    TrendingPodcasts(TrendingPodcasts),
    PodcastsByCategory(PodcastsByCategory),
}
```

All actions return parsed records. None of them write to the domain store directly — `podcast-core::SubscribePodcast`/`RefreshFeed` orchestrate the writes, preserving the doctrine: **one writer per fact** (D4).

---

## F. `Cargo.toml`

```toml
[package]
name = "podcast-feeds"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
nmp-core = { path = "../../../crates/nmp-core" }
feed-rs = "1"
quick-xml = { version = "0.31", features = ["serialize"] }   # for the Podcasting 2.0 walker
sha1 = "0.10"
hex = "0.4"
serde = { workspace = true, features = ["derive"] }
serde_json = { workspace = true }
url = "2"
async-trait = "0.1"
futures = "0.3"
tracing = "0.1"
thiserror = "1"

[dev-dependencies]
nmp-testing = { path = "../../../crates/nmp-testing" }
```

No reqwest. HTTP rides through `HttpCapability` so the bridge can use `URLSession` on iOS (background URL sessions, system cookie jar, OS-managed retries).

---

## G. Tests

- Golden feed fixtures (`tests/fixtures/`): `tim-ferriss.rss`, `huberman.rss`, `lex-fridman.rss` (truncated to 5 episodes each), `podcasting20-sample.xml`. Assertions on parsed `ParsedPodcast.title`, `episodes.len()`, first-episode field-by-field equality.
- Podcast Index auth-header test: known `(api_key, api_secret, timestamp)` triple → assert header bytes match a golden value computed offline with `openssl`.
- HTTP failure-mode test: `MockHttpCapability` returns `HttpError(500)` → `FetchFeed` fails with `toast` set.
- Conditional-GET test: stub returns 304 → `RefreshFeedOutput::NotModified`; episodes table unchanged.

Per-crate test budget: ≤ 1,200 LOC of test code.
