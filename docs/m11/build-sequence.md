# M11 Build Sequence

> Companion to [`inventory.md`](inventory.md) and [`../design/podcast-app-rebuild.md`](../design/podcast-app-rebuild.md).
> Each step is a single mergeable unit with a passing `cargo check --workspace` gate.

## Step 0 — Swift verbatim copy + Rust crate scaffolds (DONE)

**Gate:** `cargo check --workspace` passes. Clippy clean. No podcast noun in `nmp-core`.

Deliverables:
- `ios/NmpPodcast/NmpPodcast/Views/` — 29 Swift files copied verbatim from `/Users/pablofernandez/src/podcast`.
- `ios/NmpPodcast/NmpPodcast/Bridge/` — 2 bridge utilities.
- `ios/NmpPodcast/NmpPodcast/Resources/Assets.xcassets` — app icon + accent color.
- `apps/podcast/podcast-{core,feeds,audio,llm,rag}/` — 5 Rust crates with type-stub `src/` (no business logic).
- `docs/m11/inventory.md` — full file-to-crate mapping.

## Step 1 — Domain records + view/action stubs wired to kernel FFI

**Gate:** `cargo test --workspace` passes. `podcast_core` exports all DomainRecord, ViewModule, ActionModule types.

Deliverables:
- `podcast-core`: complete `DomainRecord` impls for Podcast, Episode, Transcript, Chapter, Guest, Insight, Subscription, PlayerState, QueueEntry, Activity.
- `podcast-core`: `ViewModule` stubs returning typed payloads over FFI.
- `podcast-core`: `ActionModule` handler dispatch table (all 19 actions routed, no-op bodies).
- Swift `NmpPodcast`: replace `// MARK: NMP-WIRE` stubs with FFI observer calls.

## Step 2 — Capability modules

**Gate:** `cargo check --workspace`. Each new capability compiles on the simulator target.

Deliverables:
- `AudioPlaybackCapability` — AVPlayer backend, position events, lock-screen controls.
- `BackgroundWorkCapability` — BGTaskScheduler wrapper.
- `LocalNotificationCapability` — episode-available alerts.
- `HttpCapability` — streaming response support for RSS + transcripts.
- `EmbeddingCapability` — CoreML on-device path + remote API fallback.
- `KeyValueStoreCapability` — typed persistent KV for playback positions.

## Step 3 — Protocol module integration

**Gate:** Integration tests pass for Nostr event kinds used by podcast features.

Deliverables:
- `podcast-core` subscribes to kind 1 (note), kind 31337 (chapter), kind 9735 (zap) as appropriate.
- Value-for-value streaming sats routed through `podcast-feeds` + wallet capability.

## Step 4 — Feed parsing (podcast-feeds)

**Gate:** Unit tests for RSS, Atom, Podcasting 2.0, JSON Feed. No panics on corpus.

Deliverables:
- `podcast-feeds::parser` — RSS 2.0 + Atom 1.0 full parse.
- `podcast-feeds::podcasting20` — `<podcast:*>` namespace extensions (chapters, transcripts, value).
- `podcast-feeds::podcast_index` — PodcastIndex.org API client (search, trending, categories).

## Step 5 — LLM + RAG (podcast-llm, podcast-rag)

**Gate:** Ask/Insights/GuestEnrichment action handlers return typed responses end-to-end (mocked LLM in tests).

Deliverables:
- `podcast-llm::router` — Apple Intelligence vs remote API dual path.
- `podcast-llm::actions` — AskQuestion, EnrichGuest, RunInsight fully implemented.
- `podcast-rag::store` — sqlite-vec backed vector store.
- `podcast-rag::embedding` — EmbeddingCapability integration.

## Step 6 — Screenshot diff harness

**Gate:** Side-by-side simulator screenshot diff vs `../podcast` reference shows ≤ 1 px delta on all screens.

Deliverables:
- `docs/design/podcast/screenshots.md` — harness spec.
- ImageMagick `compare` script in `justfile`.
- Reference screenshots committed to `docs/m11/screenshots/`.

## Step 7 — Per-view wiring

**Gate:** Each view group listed in `docs/design/podcast/wiring.md` is checked off.

Deliverables:
- Library group wired (LibraryView, AddPodcastView, PodcastDetailView/Sheet, ActivityView, QueueView, Discover*, EpisodeDetailView).
- Feed group wired (FeedView, EpisodeRow).
- Player group wired (PlayerSheet*, MiniPlayer, ChaptersPanel, TranscriptView, GuestAgentSheet).
- Insights wired (InsightsView).
- Ask wired (AskView).
- Settings wired (SettingsView).

## Step 8 — Exit gate

**Gate:** All criteria in `docs/design/podcast/exit-gate.md` have a passing evidence artifact.

Deliverables:
- `grep -r 'podcast\|Podcast\|Episode\|Feed' crates/nmp-core/src/` returns zero matches (kernel-boundary clean).
- Simulator screenshot diff ≤ 1 px.
- All `cargo test --workspace` tests green.
- `cargo clippy --workspace --all-targets -- -D warnings` clean.
