# Step 1B — `apps/podcast/podcast-llm`

> Parent: [`../podcast-app-rebuild.md`](../podcast-app-rebuild.md)
> Reference Swift sources: `Services/AIService.swift` (308 LOC), `Services/GuestEnrichmentService.swift` (94 LOC), `Services/InsightService.swift` (233 LOC, the `matchExcerpt` part), `Views/Ask/AskView.swift` (322 LOC), `Views/Insights/InsightsView.swift` (252 LOC), `Views/Player/GuestAgentSheet.swift` (297 LOC), `Views/Player/ChaptersPanel.swift` (324 LOC — the `findRelevantTimestamp` integration).

`podcast-llm` owns every LLM-driven action: ask-the-corpus (RAG-grounded chat), per-episode summarization, chapter extraction, "find this topic" timestamp search, guest enrichment, guest-agent chat, and excerpt matching. It is **dual-path**: Apple Intelligence on-device on iOS via a capability bridge; `rig.rs` cross-platform fallback.

---

## A. Why dual-path

The reference Swift app uses `import FoundationModels` and `LanguageModelSession()`. That is Apple Intelligence on-device — free, private, requires no API key, but **iOS-only and capped to Apple-managed models**. To preserve UX parity on iPhone (no API key onboarding, no network for inference), the rebuild must route through Apple Intelligence when available.

For Android/Desktop/Web (post-M11) the same actions route through `rig.rs` against a configured provider. The action shape is identical; the routing decision lives in `podcast-llm/src/router.rs` and reads from `SettingsRecord.llm_preferred_route`.

The Foundation Models prompts in `AIService.swift` (summary, chapter-extract, find-relevant-timestamp, chat, embed-stub) are **byte-identical** in the Rust crate. Identical prompts to identical model = identical outputs. Any prompt change is an ADR.

---

## B. `Cargo.toml`

```toml
[package]
name = "podcast-llm"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
nmp-core = { path = "../../../crates/nmp-core" }
podcast-core = { path = "../podcast-core" }
podcast-rag = { path = "../podcast-rag" }
rig-core = "0.4"          # crate name on crates.io is `rig-core`; importable as `rig`
serde = { workspace = true, features = ["derive"] }
serde_json = { workspace = true }
ulid = "1"
url = "2"
async-trait = "0.1"
futures = "0.3"
tracing = "0.1"
thiserror = "1"

[features]
default = ["rig-openai", "rig-anthropic"]
rig-openai = []
rig-anthropic = []
```

`rig-core` is chosen over `langchain-rust` because: smaller dependency footprint; first-class streaming `StreamingChat` traits; multi-provider with a uniform `CompletionModel` trait; actively maintained; no Python interop. The trade-off is fewer prebuilt agents — we want lean orchestration, not framework lock-in.

---

## C. Router

```rust
pub enum LlmRoute {
    AppleIntelligence,
    RigProvider { provider: RigProvider, model: String },
}

pub enum RigProvider {
    OpenAI,
    Anthropic,
    Local { base_url: Url, model: String },
}

pub fn select_route(settings: &SettingsRecord, capabilities: &CapabilityAvailability) -> LlmRoute {
    match settings.llm_preferred_route {
        Some(LlmRoutePref::AppleIntelligence) if capabilities.apple_intelligence => LlmRoute::AppleIntelligence,
        Some(LlmRoutePref::Rig { provider, model }) => LlmRoute::RigProvider { provider, model },
        _ if capabilities.apple_intelligence => LlmRoute::AppleIntelligence,
        _ => LlmRoute::RigProvider { provider: RigProvider::OpenAI, model: "gpt-4o-mini".into() },
    }
}
```

The actor injects `CapabilityAvailability` at boot (probed by the bridge); subsequent capability state changes invalidate cached availability.

---

## D. Action modules

### D.1 `AskQuestion`

```rust
pub struct AskQuestion {
    pub session_id: ChatSessionId,    // ulid; owns chat history
    pub query: String,
    pub rag_filter: Option<RagFilter>,  // RagFilter from podcast-rag (episode_ids, podcast_ids)
}

pub enum AskQuestionOutput { Started { token_stream_id: Ulid } }
```

Step machine:

1. `Validating` — non-empty query, session exists.
2. `Retrieving` — `ctx.dispatch(PodcastRag::Retrieve { query, limit: 5, filter })`.
3. `Generating` — assemble prompt (per `AIService.chat` system prompt; cites `[1]`..`[N]`). On `AppleIntelligence` route: `AwaitCapability(AppleIntelligenceCapability::StreamGenerate)`. On `rig` route: spawn a Rust-side streaming task; tokens come back as `InternalEvent::LlmToken`.
4. As each token arrives: append to the in-memory `ChatSessionState` (lives in `podcast-llm::sessions` projection cache, not in `AppState`); emit a `AskViewModule::Delta::TokenAppended { session_id, token }` (≤ 30 deltas/sec coalesced per ADR-0002).
5. `Complete` when stream finishes: write final `ChatTurn` with source citations to the session.

Streaming contract: tokens are bundled into the `ViewBatch` lane (typed deltas), not the `SideEffect` lane. The Swift side reads `ChatTurnPayload { content_so_far: String, sources: Vec<SourceChip>, is_streaming: bool }` and re-renders incrementally — same UI shape as `MessageBubble` in `AskView.swift`.

### D.2 `RunInsight`

The reference app calls this implicitly: the `InsightsView` is just a list; *creating* an insight is the `StopInsightRecording` action chain that ends in `MatchExcerpt`. `RunInsight` here is the **on-demand episode insight** generation — a structured episode-summary action distinct from the `SummarizeEpisode` brief/detailed summary. M11 ships it because the M11 exit gate mentions "Insights view generates a structured episode summary on demand."

```rust
pub struct RunInsight { pub episode_id: EpisodeId }
pub enum RunInsightOutput { Generated { sections: Vec<InsightSectionPayload> } }
```

Prompt: a structured-output prompt that asks for `key_points: [String]`, `quotes: [{ speaker?: String, text: String, timestamp_s?: f64 }]`, `topics: [String]`. Routes via `LlmRoute`.

### D.3 `EnrichGuest`

```rust
pub struct EnrichGuest { pub guest_id: GuestId }
pub enum EnrichGuestOutput { Enriched { bio: String, content_chunks_indexed: u32 } }
```

Step machine matches `GuestEnrichmentService.swift`:

1. Walk up to 3 most-recent appearances → extract mention sentences.
2. Build context prompt → generate bio → write `GuestRecord.bio`, `last_enriched_at_ms`.
3. Optionally fetch external sources (Twitter handle / website) via `HttpCapability` and `GuestContent::ingest`.

### D.4 `AskGuest`

```rust
pub struct AskGuest { pub guest_id: GuestId, pub episode_id: EpisodeId, pub query: String }
```

Reuses the `AskQuestion` machine with a guest-specific system prompt and `RagFilter::ByGuest(guest_id)`.

### D.5 `FindRelevantTimestamp`

```rust
pub struct FindRelevantTimestamp { pub episode_id: EpisodeId, pub query: String }
pub enum FindRelevantTimestampOutput {
    Found { timestamp_s: f64, context: String },
    NotFound,
}
```

Matches `AIService.findRelevantTimestamp` byte-equal. Used by `ChaptersPanel.swift`'s search bar.

### D.6 `MatchExcerpt`

```rust
pub struct MatchExcerpt {
    pub episode_id: EpisodeId,
    pub thought_text: String,
    pub capture_time_s: f64,
}
pub enum MatchExcerptOutput { Matched { text: String, start_s: f64, end_s: f64 } }
```

10-minute window before capture_time_s → AI matches the relevant excerpt. Called inside `StopInsightRecording`'s step machine.

### D.7 `SummarizeEpisode` (delegated by `podcast-core::SummarizeEpisode`)

`podcast-core` owns the action **shape**; the LLM call is delegated to `podcast-llm` via a private `ctx.dispatch_internal(SummarizePrompt { episode_id, style })` (one actor message tick — no FFI boundary crossed). This avoids `podcast-core` depending on `podcast-llm` while keeping the action namespaced under `podcast.*` from the Swift app's POV.

---

## E. ViewModules

### E.1 `AskViewModule`

Single view module driving `AskView.swift`. Reads from `podcast-llm::sessions::ChatSessionState`:

```rust
pub struct AskView {
    pub session_id: ChatSessionId,
    pub messages: Vec<ChatTurnPayload>,
    pub suggested: Vec<String>,          // pre-formatted suggestion chips
    pub episode_count: u32,
    pub hours_listened_str: String,      // pre-formatted "12 hours listened"
    pub is_loading: bool,
}

pub struct ChatTurnPayload {
    pub id: Ulid,
    pub role: ChatRole,                  // User | Assistant
    pub content: String,                 // already grown by streaming token deltas
    pub sources: Vec<SourceChipPayload>,
    pub is_streaming: bool,
}

pub struct SourceChipPayload {
    pub episode_title: String,
    pub podcast_title: String,
    pub timestamp_str: Option<String>,   // "12:34" pre-formatted
    pub is_insight: bool,
}
```

### E.2 `GuestAgentViewModule`

Drives `GuestAgentSheet.swift`. Specced by `{ guest_id, episode_id }`.

---

## F. Prompt library

Lives in `podcast-llm/src/prompts.rs`. **Byte-identical** to the Swift source. Each prompt is a `const &str` plus a builder function for parameter injection:

```rust
pub const SUMMARY_SYSTEM: &str = "..."; // empty in reference; user message carries everything
pub fn build_summary_user(text: &str, style: SummaryStyle) -> String { /* mirrors AIService.buildSummaryPrompt */ }

pub fn build_chapter_extract_user(chunks: &[(String, f64, f64)]) -> String { /* mirrors AIService.extractChapters prompt */ }

pub fn build_find_timestamp_user(query: &str, chunks: &[(String, f64, f64)]) -> String { /* … */ }

pub fn build_chat_system(context: &str) -> String { /* RAGService.chat systemPrompt */ }
```

The 8 KB transcript truncation cap and 12 KB chapter-extract cap from the Swift source are preserved as `const usize`. Future tuning is an ADR.

---

## G. Streaming pipeline

```
AppleIntelligenceCapability::StreamGenerate { prompt }
    → bridge emits SidePush::Token { stream_id, token }
        each token: kernel → AskAction::reduce(AssistantTokenArrived)
            ↳ append to ChatSessionState.streaming_buffer
            ↳ emit AskViewModule::Delta::TokenAppended (coalesced per ADR-0002)
                ↳ Swift @StateObject re-renders incrementally
    → bridge emits SidePush::Finish { stream_id }
        kernel → AskAction::reduce(StreamFinished)
            ↳ commit final ChatTurn to ChatSession history
            ↳ emit AskViewModule::Delta::TurnCommitted
```

Streaming-output back-pressure: if the coalescer drops tokens (≥60 Hz), they accumulate in `ChatSessionState.streaming_buffer` server-side; the next delta carries the full buffer. The Swift side reads `content` directly — no lost tokens.

For `rig.rs` routes, the same shape: `rig::completion::StreamingChat` yields a `Stream<Item = String>`; an internal Rust task awaits each, converts to `InternalEvent::LlmToken`, posts to the actor.

---

## H. Cache strategy

- **Per-session chat history**: in-memory `ChatSessionState`. Survives navigation. Drops 5 minutes after the session view module unsubscribes (per ADR-0005 warmth).
- **Per-episode generated content** (summary, chapters, find-timestamp answers): persisted on `EpisodeRecord.ai_summary`, `ChapterRecord`s, and a `find_timestamp_cache: HashMap<(EpisodeId, query_hash), TimestampHit>` projection (size-capped at 100 entries per episode).
- **Per-guest enrichment**: persisted on `GuestRecord.bio` + `GuestRecord.last_enriched_at_ms`. Stale after 30 days → silently re-enriched on next view open.
- **No prompt response caching** for Ask. Each query is a fresh stream; the cost is borne by the user, and the prompt is dynamic (uses RAG context).

---

## I. API key storage

`KeyValueStoreCapability` (see [`capabilities.md`](capabilities.md)) hosts:

- `podcast.llm.openai_api_key` (set via Settings view post-M11; not in M11 UI surface; M11 uses AppleIntelligence-only on iOS, fallback prompts in Settings if Apple Intelligence unavailable).
- `podcast.llm.anthropic_api_key`.
- `podcast.llm.local_base_url`.

Reads happen at action boot only. Per RMP bible commandment #2, keys never cross FFI as part of dispatch — they live on the platform side; the bridge resolves and injects them when executing the capability.

---

## J. Tests

- Prompt-parity test: each `build_*` function called with the exact arguments from a reference transcript fixture → output asserted byte-equal to a checked-in `prompts_golden/*.txt`.
- Router test table: each route selected per `(settings, capabilities)` combo.
- Streaming integration test: `MockLlmCapability` emits tokens at a chosen cadence; assert `AskViewModule` deltas coalesce to ≤ 30 Hz; assert final `ChatTurn` content is the concatenation; assert citations parsed correctly.
- Failure-mode test: capability emits `Error { reason: "rate_limit" }` mid-stream → `ChatTurn` commits with `is_error: true` and the `toast` field is set per doctrine D3.
