# Risks + open questions

> Parent: [`../podcast-app-rebuild.md`](../podcast-app-rebuild.md).
> Combined per AGENTS.md — keep sub-docs ≤ 500 LOC.

---

## A. Risks (high → low)

### A.1 sqlite-vec on iOS — static linking with `load_extension`

- **Severity**: High. Top risk in the whole M11 stack.
- **Why**: `rusqlite` with `bundled` compiles `libsqlite3` from source, which is fine. Loading `vec0` at runtime requires `SQLITE_ENABLE_LOAD_EXTENSION`. iOS App Store review historically rejects `dlopen` of arbitrary dylibs; the safer path is **static link** of the vec0 symbols into the libsqlite3 we build, behind `SQLITE_CORE`.
- **Mitigation**:
  1. Spike before Step 1C: build a minimal Rust+rusqlite+sqlite-vec crate, archive for `aarch64-apple-ios`, embed in a throwaway xcframework, install on iPhone 12, run a 1000-vector insert + search round-trip.
  2. Document the `build.rs` patch (cflags + statically registered extension) in `docs/design/podcast/podcast-rag.md`.
  3. **Fallback**: if static link is infeasible, ship a Rust-side `instant-distance` HNSW backend behind the same `VectorStore` trait. Schema persists to a flat `.bin` file instead of SQLite. Worse cold-start but unblocks M11.
- **Owner**: assigned at M11 kickoff; spike result is a blocker for `podcast-rag` impl beginning.

### A.2 Apple Intelligence is iOS-only → cross-platform divergence

- **Severity**: Medium-High.
- **Why**: The reference app's entire LLM surface uses `FoundationModels.LanguageModelSession`. On iOS we route through `AppleIntelligenceCapability` (free, on-device). On Android/Desktop/Web (post-M11) we must route through `rig.rs`, which means: a provider, an API key, network calls, cost. The UX divergence is real — Ask becomes a fundamentally different feature off-iOS.
- **Mitigation**:
  1. M11 ships iOS only **and** requires the rig.rs path to land and be CI-tested against a real LLM endpoint (see `podcast-llm.md` §K). Divergence across platforms is real post-M11, but the rig.rs path is demonstrated in M11, not deferred to M15.
  2. The router lives in Rust (`podcast-llm/src/router.rs`) so the policy is centrally testable.
  3. Document divergence + behavioral expectations in `docs/perf/m15/cross-platform.md` (deferred).
  4. Settings surface (post-M11) lets the user pick "Local (Apple)" / "OpenAI" / "Anthropic" / "Local (Ollama URL)" — same code path, different routing.

### A.3 Foundation Models prompt-shape parity vs `rig.rs`

- **Severity**: Medium.
- **Why**: The Foundation Models API takes one big string + optional system prompt; `rig.rs` likes a structured `Vec<ChatMessage>` with role tags. The same prompt routed two ways can produce divergent output if the framing changes ("User: ... Assistant: ..." in-string vs proper message roles).
- **Mitigation**:
  1. `podcast-llm/src/prompts.rs` defines each prompt as both a flat string (for AppleIntel) AND a `Vec<rig::completion::Message>` (for rig). Both shapes are byte-deterministic functions of the same inputs.
  2. Golden tests: same input → both shapes → snapshotted; reviewed when changed.
  3. Where output format matters (the `CHAPTER|...|YES` parsing in `extractChapters`, the `FOUND|MM:SS|...` in `findRelevantTimestamp`), the parser is a single Rust function that handles both routes' outputs. Tests cover each route's raw output samples.

### A.4 Embedding-dimension swap mid-project

- **Severity**: Medium.
- **Why**: We choose `bge-small-en-v1.5` at 384 dims. Future requests for `bge-large-en-v1.5` (1024 dims) or `text-embedding-3-large` (3072 dims) require a sqlite-vec schema migration that rebuilds every existing index. Each embedding takes ~12 ms on iPhone 12; 5,000 chunks per podcast × 100 podcasts × 12 ms = 100 minutes of background re-index per device.
- **Mitigation**:
  1. Lock 384 dims in M11. Schema records the model name.
  2. Migration is an explicit `RebuildEmbeddingIndex` action with progress UI.
  3. Tests for the migration include the worst-case (500k vectors) and assert wall-clock budget.

### A.5 Background audio reliability on iPhone 12

- **Severity**: Medium.
- **Why**: `AVAudioSession` interactions with the kernel's actor crossing FFI on every `Tick` event can introduce jitter. The reference Swift app handles this in-process; we cross FFI ~4×/s plus on every state change. On a battery-throttled iPhone 12, this is non-zero.
- **Mitigation**:
  1. The bridge throttles `Tick` to ≤ 4 Hz at the source (per [`capabilities.md`](capabilities.md) §A).
  2. The Rust-side coalescer is the second line of defense (per ADR-0002).
  3. Hardware perf test in [`exit-gate.md`](exit-gate.md) §C catches regressions.

### A.6 `feed-rs` Podcasting 2.0 gaps

- **Severity**: Low-Medium.
- **Why**: `feed-rs` doesn't expose `<podcast:transcript>` etc. We layer a bespoke walker over the raw XML. Edge-case malformed feeds may break our walker.
- **Mitigation**:
  1. Golden fixtures for 5+ real Podcasting 2.0 feeds in `tests/fixtures/`.
  2. Walker errors are non-fatal: missing P2.0 extensions don't fail `FetchFeed`; the episode is parsed with the basic fields only.
  3. Future: replace with a maintained P2.0 Rust crate if one emerges.

### A.7 Codegen drift between branches

- **Severity**: Low-Medium.
- **Why**: 7 parallel lanes regenerate `nmp-app-podcast/` on every module change. Merge conflicts in the generated `action.rs`/`update.rs`/`view_spec.rs` are predictable and painful.
- **Mitigation**:
  1. Generated files are committed (per ADR-0010) — conflicts surface at merge.
  2. CI runs `nmp gen modules --check` post-merge; rebases that produce stale generated code fail the build.
  3. Convention: generated files are merge-with-"theirs" then `nmp gen modules` re-run, never hand-edited.

### A.8 Reference Swift app commits drifting

- **Severity**: Low.
- **Why**: `/Users/pablofernandez/src/podcast` may be edited during M11. Our screenshot baselines + view copies depend on a specific SHA.
- **Mitigation**: per [`copy.md`](copy.md) §3, we pin the source SHA in `docs/perf/m11/parity-screenshots.md`. Re-baselining is deliberate.

### A.9 Parallel-agent codegen conflicts

- **Severity**: Low.
- **Why**: Two agents wiring two lanes both regenerate bindings. Same as A.7 but at the Swift binding layer (`bindings/swift/` checked-in). Possible "phantom" Swift diffs from generator nondeterminism.
- **Mitigation**: deterministic codegen is a hard ADR-0010 requirement. CI determinism test catches regressions immediately.

### A.10 Insight voice-recording → transcription pipeline reliability

- **Severity**: Low.
- **Why**: The Insight capture flow is 5 steps (record → transcribe thought → match excerpt → embed → write). Any step failing produces an orphan recording. Reference app has the same issue.
- **Mitigation**: action ledger atomicity. Each step is a ledger step; on failure, the cleanup is `DeleteOrphanRecording { recording_id }`. Restart recovery resumes from the failed step.

---

## B. Open questions (for ADRs)

### B.1 Should `ChatSessionState` be a DomainModule or a projection cache?

- **Context**: Chat history can be many turns × many tokens. It's expensive to persist on every token but useful to persist across kill-relaunch.
- **Options**: (a) DomainModule with batched writes (every 50 tokens or 5 s); (b) Projection cache, evaporates on kill — re-fetchable only if the LLM provider keeps a server-side history.
- **Recommendation**: (a) with snapshot writes. Future ADR.

### B.2 Where does the Podcasting 2.0 V4V (`<podcast:value>`) data live?

- **Context**: V4V/Lightning splits could feed into a future zaps integration (M12). Today we have nowhere to put them.
- **Options**: (a) Persist as `EpisodeRecord.value_block: Option<ValueBlock>`; (b) Dropped at parse time; (c) Separate `Podcast2ValuesModule` domain.
- **Recommendation**: (a). Forward-compatible; minimal cost. Future ADR if it grows.

### B.3 Should transcript-tap-to-play live in `TranscriptViewModule` or as a sibling `TranscriptInteractionModule`?

- **Context**: Tapping a chunk dispatches `Seek`. That's a one-line dispatcher — overkill for a separate module.
- **Recommendation**: in-place in `TranscriptViewModule`. No ADR needed.

### B.4 Podcast Index keys — store in `KeyValueStoreCapability` or hardcode for the demo?

- **Context**: M11 needs Discover to work for the demo. The reference app reads from Info.plist or env. We don't want to ship demo keys.
- **Options**: (a) Settings flow to enter keys (requires Settings view enhancement beyond parity); (b) `.env` file in dev; (c) Build-time bake from `nmp.toml` `[secrets]` section, gitignored.
- **Recommendation**: (b) for M11 dev, (a) post-M11 in a Settings expansion. Future ADR.

### B.5 Insight thought audio playback — Bridge or capability?

- **Context**: A short transient `AVPlayer` plays a recorded thought when the user taps "Play recording" on an InsightCard. Currently planned as Bridge-only (UI plays it, kernel doesn't know).
- **Options**: (a) Bridge-only; (b) extend `AudioPlaybackCapability` to support a secondary mini-player; (c) new `MicroAudioCapability`.
- **Recommendation**: (a). It's pure UI behavior and the Rust side doesn't care about which moment it plays. ADR only if (b) or (c) becomes necessary.

### B.6 Episode duration source of truth

- **Context**: RSS feeds publish `<itunes:duration>`. AVPlayer reports `currentItem.duration`. They can differ (rare, but documented).
- **Options**: (a) Always trust the feed; (b) Always trust AVPlayer; (c) Prefer feed, update from AVPlayer if difference > 5%.
- **Recommendation**: (c). ADR cheap; the impl is small.

### B.7 OPML import — additive scope decision documented; what about export?

- **Context**: M11 adds `ImportOpml` as additive. Export is a similar one-action win.
- **Recommendation**: `ExportOpml` follows in M11 if Settings UI allows (the existing Settings has no Import/Export section; adding one is one-screen). Defer to a Settings ADR. M11 ships Import only.

### B.8 Multi-language transcription default

- **Context**: `TranscriptionService.swift` defaults to `en-US`. Auto-detect is non-trivial and SpeechAnalyzer doesn't help.
- **Options**: (a) Always en-US; (b) Settings "Default Language"; (c) Per-podcast override.
- **Recommendation**: (a) for M11; (b) added in a Settings expansion. ADR if (c) becomes important.
