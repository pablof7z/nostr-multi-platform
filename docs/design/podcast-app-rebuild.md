# Design: `../podcast` rebuild on NMP (M11)

> **Status:** Proposed (M11 prep)
> **Date:** 2026-05-18
> **Scope:** How M11 rebuilds `/Users/pablofernandez/src/podcast` on NMP — kernel-boundary proof + pixel-perfect UI parity.
> **Prerequisites:** `docs/plan.md` §M11, `docs/aim.md`, `docs/design/app-extension-kernel.md`, `docs/design/kernel-substrate.md`, `docs/decisions/0009`, `docs/decisions/0010`, `crates/fixture-todo-core/src/lib.rs`.
> **Companion docs:** under `docs/design/podcast/` — split per AGENTS.md ≤ 500 LOC ceiling.

---

## 1. Goal

A **pixel-perfect rebuild** of the canonical Swift podcast app at `/Users/pablofernandez/src/podcast` (8,793 LOC across 47 Swift files, see [`inventory.md`](podcast/inventory.md)) running on NMP. Every Swift view file in `PodcastApp/Views/` is copied verbatim into `ios/NmpPodcast/Views/`; only the data source is rewired. All business logic — services, models, view models, the LLM/RAG/transcription pipeline, audio orchestration, downloads, RSS, recommendations — moves into Rust extension modules behind the kernel substrate (`DomainModule`, `ViewModule`, `ActionModule`, `CapabilityModule`, `IdentityModule`).

**This is M11's load-bearing kernel-boundary check.** If the kernel needs even one podcast noun to make it work, the boundary is wrong and we go back to fix it. The exit gate is dual: (a) `nmp-core` gains zero podcast nouns, verified by grep + manual review; (b) screenshot diff vs `../podcast` is ≤ 1 px on every screen, font-rendering exceptions whitelisted.

**Scope clarification (additive vs strict parity).** `docs/plan.md` §M11 lists `ImportOpml` as an action. The reference Swift app has no OPML import (`AddPodcastView` is a single-URL form, `LibraryView` lists subscribed podcasts only). M11 ships `ImportOpml` as **additive beyond strict parity** — implementable purely in Rust (no new Swift UI required beyond a button in `AddPodcastView`), so it does not threaten parity. The parity screenshot harness whitelists views that did not exist in `../podcast`. All other M11 features map 1:1 to a Swift surface.

**Multiplatform-ready.** Although M11 ships only the iPhone target, every extension crate is pure Rust and compiles to wasm32 + android targets (capabilities aside). Android/Desktop/Web shells are explicitly post-M11; the design must not foreclose them.

---

## 2. Sub-doc index

| § | Sub-doc | Purpose |
|---|---|---|
| 2 | [`podcast/inventory.md`](podcast/inventory.md) | Full Swift file → NMP module/crate mapping table (47 files, 8,793 LOC). |
| 3 | [`podcast/copy.md`](podcast/copy.md) | Step 0: the verbatim copy step, `NMP-WIRE` placeholder pattern, screenshot-diff harness design. |
| 4 | [`podcast/podcast-core.md`](podcast/podcast-core.md) | Step 1: `podcast-core` extension crate — `Cargo.toml`, all DomainModules / ViewModules / ActionModules with field-level signatures. |
| 4 | [`podcast/podcast-llm.md`](podcast/podcast-llm.md) | Step 1: `podcast-llm` extension crate — `rig.rs` + Apple Intelligence dual path, Ask/Insights/GuestEnrichment flows. |
| 4 | [`podcast/podcast-rag.md`](podcast/podcast-rag.md) | Step 1: `podcast-rag` extension crate — sqlite-vec choice, chunking, embedding model, retrieval. |
| 4 | [`podcast/podcast-feeds.md`](podcast/podcast-feeds.md) | Step 1: `podcast-feeds` extension crate — RSS / Atom / Podcasting 2.0 / JSON Feed parsing. |
| 5 | [`podcast/capabilities.md`](podcast/capabilities.md) | Step 2: nine new `CapabilityModule`s — Rust traits, iOS impl sketches, idempotency + bounded-state proofs. |
| 6 | [`podcast/screenshots.md`](podcast/screenshots.md) | Step 6: side-by-side screenshot diff harness (ImageMagick `compare`, thresholds, reference store). |
| 7 | [`podcast/wiring.md`](podcast/wiring.md) | Step 7: per-view-group wiring checklist (Library, Feed, Player, Insights, Ask, Settings, Components) with dependencies. |
| 8 | [`podcast/exit-gate.md`](podcast/exit-gate.md) | Step 8: M11 exit-gate-to-evidence-artifact map. |
| 9 | [`podcast/lessons.md`](podcast/lessons.md) | Lessons from `podcast-rmp` (DerivedData sprawl mitigation, sqlite-vec iOS spike, god-module avoidance). |
| 10 | [`podcast/risks.md`](podcast/risks.md) | Risks + open questions (combined to stay ≤ 500 LOC). |

Each sub-doc is ≤ 500 LOC per AGENTS.md. The index (this file) is intentionally short.

---

## 3. Architecture at a glance

```
                  iOS NmpPodcast.app
                  ┌──────────────────────────────────────────────────┐
                  │  ios/NmpPodcast/Views/  (verbatim copy from      │
                  │      ../podcast/PodcastApp/Views/)               │
                  │  ios/NmpPodcast/Bridge/                          │
                  │      ├── PodcastBridge.swift  (uses generated    │
                  │      │      `@PodcastLibrary`, `@NowPlaying`)    │
                  │      └── Capabilities/                           │
                  │          ├── AudioPlayback.swift  (AVPlayer)     │
                  │          ├── BackgroundWork.swift (BGTaskSched.) │
                  │          ├── LocalNotification.swift (UNUC)      │
                  │          ├── Embedding.swift   (CoreML)          │
                  │          ├── AppleIntelligence.swift (FM)        │
                  │          ├── Transcription.swift (SpeechAnalyz.) │
                  │          ├── VoiceRecording.swift (AVAudioRec.)  │
                  │          ├── Http.swift  (URLSession streaming)  │
                  │          └── KeyValueStore.swift (UserDefaults)  │
                  └─────────────────▲───────────────▲────────────────┘
                                    │ dispatch       │ ViewBatch / SideEffect
                                    │                │ (via FfiApp)
        nmp-app-podcast (generated) │                │
        ┌───────────────────────────┴────────────────┴────────────┐
        │  AppAction { Kernel(...), Podcast(podcast_core::Act),   │
        │              PodcastLlm(...), PodcastRag(...),          │
        │              PodcastFeeds(...) }                        │
        │  AppUpdate, ViewSpec, capability trait composites       │
        └────────┬───────────────────┬────────────────┬───────────┘
                 │                   │                │
    ┌────────────┴────────┐  ┌──────┴───────┐  ┌────┴──────┐  ┌──────────────┐
    │ podcast-core (app)  │  │ podcast-llm  │  │podcast-rag│  │podcast-feeds │
    │ DomainModule × 10   │  │ AskQuestion  │  │ Index     │  │ Rss/Atom/JF/ │
    │ ViewModule × 17     │  │ RunInsight   │  │ Retrieve  │  │ Podcasting2.0│
    │ ActionModule × ~24  │  │ EnrichGuest  │  │ sqlite-vec│  │ HttpCapabil. │
    └────────────┬────────┘  └──────┬───────┘  └────┬──────┘  └──────┬───────┘
                 │                  │               │                │
                 └────────┬─────────┴───────────────┴────────────────┘
                          │
        ┌─────────────────┴─────────────────────────────────────┐
        │ nmp-core (kernel)                                     │
        │   actor · event store · view registry · action ledger │
        │   capability registry · domain registry · diagnostics │
        │   NO podcast nouns. NO LLM nouns. NO audio nouns.     │
        └───────────────────────────────────────────────────────┘
```

The kernel stays podcast-agnostic. All product nouns live in the four `podcast-*` extension crates. The generated `nmp-app-podcast/` aggregates their action / view / capability enums per ADR-0010.

---

## 4. Crate roster

Added under `apps/podcast/`:

| Crate | Owns | Surface (high-level) |
|---|---|---|
| `apps/podcast/podcast-core` | Core domain (Podcast/Episode/etc.), library/feed/player/queue/insight view modules, subscribe/play/download/enqueue actions | All DomainModules, most ViewModules + ActionModules |
| `apps/podcast/podcast-llm` | LLM-driven workflows: ask / insight summary / chapter extraction / guest enrichment | `AskQuestion`, `RunInsight`, `EnrichGuest`, `ExtractChapters`, `SummarizeEpisode` actions |
| `apps/podcast/podcast-rag` | Embedding + vector store + retrieval (sqlite-vec backed) | `IndexChunk`, `IndexInsight`, `IndexGuestContent`, `Retrieve` actions; `RagResults` view |
| `apps/podcast/podcast-feeds` | RSS / Atom / JSON Feed / Podcasting 2.0 parsing; transcript namespace; chapters; value-for-value | `FetchFeed` action (returns parsed records); no view modules |
| `apps/podcast/nmp-app-podcast` | **Generated** — aggregator crate per ADR-0010 | UniFFI surface; per-platform bindings |

Plus seven new capability families landed in `nmp-core/src/substrate/capabilities/` (each pure-trait; impls live in the platform shell). See [`podcast/capabilities.md`](podcast/capabilities.md).

`nmp-core` itself gains **zero** podcast types. The exit-gate grep is:

```
grep -RE 'Podcast|Episode|Transcript|Chapter|Player|Feed|Insight|Guest|RSS|Audio|MP3' \
     crates/nmp-core/src/ \
     | grep -v capabilities/audio_playback.rs  # the *trait* is allowed; it says nothing about podcasts
```

Expected: zero matches except the generic `AudioPlaybackCapability` trait file (whose request/result types name no podcast nouns).

---

## 5. Top-level execution order

1. [`copy.md`](podcast/copy.md) — Step 0: `cp -R`, commit, screenshot baseline.
2. [`capabilities.md`](podcast/capabilities.md) — Step 2 (intentionally before Step 1: the trait shapes are needed before the action modules can refer to them).
3. [`podcast-core.md`](podcast/podcast-core.md), [`podcast-feeds.md`](podcast/podcast-feeds.md) — Step 1 base (parallel; no LLM dependency).
4. [`podcast-rag.md`](podcast/podcast-rag.md) — Step 1 RAG (depends on `EmbeddingCapability` from Step 2 and core's `Transcript` domain record).
5. [`podcast-llm.md`](podcast/podcast-llm.md) — Step 1 LLM (depends on RAG context for Ask).
6. [`wiring.md`](podcast/wiring.md) — Step 7: wire each Swift view to its generated wrapper.
7. [`screenshots.md`](podcast/screenshots.md) — gate every wired group.
8. [`exit-gate.md`](podcast/exit-gate.md) — verify against M11's exit-gate bullets.

Each step ends with: workspace `cargo test --workspace` green; the per-step screenshot diffs pass; a perf bullet from §M11 measured numbers.

---

## 6. Lessons + risks — see sub-docs

- [`lessons.md`](podcast/lessons.md): what podcast-rmp tried, what we keep, what we change. The DerivedData sprawl gets an explicit day-one mitigation: every worktree exports `CARGO_TARGET_DIR=$HOME/.cargo-shared-target` and every `xcodebuild` invocation passes `-derivedDataPath $HOME/.cargo-shared-target/xcode-derived-data`. Encoded in `justfile`.
- [`risks.md`](podcast/risks.md): concrete risks (sqlite-vec iOS bundling, Apple-Intelligence iOS-only divergence, Foundation-Models prompt-shape parity vs `rig.rs`, embedding-dimension swap) + open questions queued for ADRs.

---

## 7. Reference Swift surface — totals

- **47 Swift files**, **8,793 LOC** under `/Users/pablofernandez/src/podcast/PodcastApp/`.
- Of which: **18 view files** stay Swift (UI), **14 services + 8 models + 0 view models** move to Rust (the Swift `ViewModels/` directory exists but is empty in the canonical app — view models are inlined as `@State` inside views or read directly from `@Observable` services).
- Full per-file table: [`inventory.md`](podcast/inventory.md).

---

## 8. Out of scope for M11

- Android / Desktop / Web targets — post-M14 (UniFFI migration) and M15 (cross-platform).
- Nostr social overlay (NIP-XX podcast events, V4V zaps, comment threads). Listed in `docs/plan.md` §M11 as opportunistic; deferred to a `nmp-podcast` Nostr protocol module post-M11.
- Watch / CarPlay / Widgets / Live Activities / Siri intents — none exist in `../podcast`; out of scope.
- NIP-46 bunker signing, Web-of-Trust scoring of podcast recommendations — out of scope (the reference app has no Nostr).
- iCloud sync of subscriptions — out of scope (the reference app uses local SwiftData only).
