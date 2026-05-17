# Step 2 — capabilities

> Parent: [`../podcast-app-rebuild.md`](../podcast-app-rebuild.md).
> Substrate reference: [`../kernel-substrate.md`](../kernel-substrate.md) §5; doctrine: D5 (capabilities report, never decide).

M11 adds **nine** new `CapabilityModule`s to the kernel's reusable set. Each is generic (no podcast nouns in request or result types) so a future Pika-like, Highlighter, or messaging app could reuse them. Each is **idempotent** (start/stop/restart safe per RMP bible commandment #7) and has **bounded native state** (the bridge holds only the OS handle; no caches, no derived state, no policy).

Six were named in `docs/plan.md` §M11; three (`TranscriptionCapability`, `VoiceRecordingCapability`, `AppleIntelligenceCapability`) are added here because parity demands them — without them the rebuild routes around the entire on-device Apple Intelligence + Speech stack that the reference app depends on.

---

## A. `AudioPlaybackCapability`

```rust
pub struct AudioPlaybackCapability;

impl CapabilityModule for AudioPlaybackCapability {
    const NAMESPACE: &'static str = "core.audio_playback";
    type Request = AudioRequest;
    type Result = AudioResult;
    fn callback_interface_name() -> &'static str { "AudioPlaybackBridge" }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum AudioRequest {
    Load { url_or_path: String, start_s: Option<f64>, rate: Option<f32> },
    Play,
    Pause,
    Seek { to_s: f64 },
    SetRate { rate: f32 },
    Stop,
    SetNowPlayingInfo { title: String, artist: Option<String>, artwork_url: Option<String>, duration_s: f64 },
    ClearNowPlayingInfo,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum AudioResult {
    Loaded { duration_s: f64 },
    StateChanged { state: AudioState },
    Tick { position_s: f64 },                      // ≤ 4 Hz from bridge (kernel coalesces)
    RemoteCommand { command: RemoteCommand },      // play/pause/skip from lock-screen
    Ended,
    Error { reason: String },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum AudioState { Idle, Loading, Playing, Paused, Buffering }

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum RemoteCommand { Play, Pause, SkipForward(f64), SkipBack(f64), SeekTo(f64) }
```

### iOS impl sketch (`ios/NmpPodcast/Bridge/Capabilities/AudioPlayback.swift`)

```swift
final class AudioPlaybackBridgeImpl: NSObject, AudioPlaybackBridge {
    private var player: AVPlayer?
    private var timeObserver: Any?
    private weak var sink: AudioPlaybackResultSink?

    func execute(_ request: AudioRequest) {
        switch request {
        case .load(let path, let start, let rate):
            setupAudioSession()
            let item = AVPlayerItem(url: URL(...path...))
            player = AVPlayer(playerItem: item)
            // KVO status observer → on .readyToPlay: seek to start, play, emit Loaded.
            // periodicTimeObserver(interval: 0.25s) → emit Tick.
        case .play:  player?.play(); sink?.report(.stateChanged(.playing))
        case .pause: player?.pause(); sink?.report(.stateChanged(.paused))
        // ...
        }
    }
}
```

### Idempotency proof (RMP bible #7)

- `Load { url_a } → Load { url_a }` → the bridge executes every `Load` unconditionally: it tears down the current item (KVO removed, observer removed, `AVPlayer` released) and sets up a fresh `AVPlayer`. Coalescing duplicate-URL loads is the **Rust kernel's responsibility** (`podcast-core::player` ActionModule checks whether the requested episode is already loaded before dispatching `AudioRequest::Load`; the bridge never short-circuits on its own).
- `Load { url_a } → Load { url_b }` → first item torn down, second item set up. Tests cover this exact sequence on app suspend/resume.
- `Stop → Stop` → idempotent (already idle).
- `Pause → Pause` → idempotent.
- App suspend/resume: `AVAudioSession` is configured with `.spokenAudio` mode and `.playback` category; `setActive(true)` is called inside `setupAudioSession()` on every `Load`. Interruption notifications (`AVAudioSession.interruptionNotification`) feed back into the bridge as `.stateChanged(.paused)` or `.stateChanged(.playing)`.

### Bounded-state proof (D5)

The bridge holds: `player: AVPlayer?` (transient — one per current episode), `timeObserver: Any?` (transient — one per current load), `sink: weak AudioPlaybackResultSink?`. **No queue, no playback history, no preferences, no Settings access.** The "skip ad" decision is made by `podcast-core::AudioPlaybackOrchestrator` from the Rust side, which receives `Tick` events and dispatches `Seek` requests when a chapter is `is_ad`. The bridge never decides anything.

---

## B. `BackgroundWorkCapability`

```rust
pub enum BackgroundWorkRequest {
    RegisterTask { identifier: String, kind: BackgroundTaskKind, earliest_run_ms: u64 },
    CancelTask { identifier: String },
}

pub enum BackgroundTaskKind { AppRefresh, Processing }

pub enum BackgroundWorkResult {
    Registered { identifier: String },
    Woke { identifier: String, deadline_ms: u64 },
    Completed { identifier: String },
    Failed { identifier: String, reason: String },
}
```

iOS impl: `BGTaskScheduler.shared.register(forTaskWithIdentifier:)` per `RegisterTask`. The bridge fans out wake events to the kernel; the kernel dispatches the appropriate action (e.g., `Podcast::RefreshAllFeeds` on `AppRefresh`, drains pending downloads on `Processing`). Idempotent: re-registering with the same identifier replaces the prior schedule (`BGTaskScheduler.submit`). Bounded state: BGTaskScheduler is OS-owned; the bridge holds only the identifier→completion-handler map for currently-running tasks.

---

## C. `LocalNotificationCapability`

```rust
pub enum NotificationRequest {
    Authorize,
    Schedule { id: String, title: String, body: String, when: NotificationTrigger, payload_json: String },
    Cancel { id: String },
    ListPending,
}

pub enum NotificationTrigger {
    AtMs(u64),
    AfterSeconds(u64),
    OnAppRelaunch,
}

pub enum NotificationResult {
    AuthorizationGranted { granted: bool },
    Scheduled { id: String },
    Cancelled { id: String },
    Pending { ids: Vec<String> },
    Tapped { id: String, payload_json: String },
}
```

iOS impl: `UNUserNotificationCenter`. Tap delegation routes back as `Tapped { id, payload_json }` events. Used for **new-episode-available** notifications when `RefreshAllFeeds` discovers new items. Idempotent re-schedule, bounded state.

---

## D. `HttpCapability` extensions

The existing `HttpCapability` (M10 prerequisite) is extended for **streaming GET** (RSS feed pulls) and **resumable POST/PUT** (already there for Blossom). The new variant:

```rust
pub enum HttpRequest {
    Get { url: Url, headers: Vec<(String, String)>, max_bytes: Option<u64> },
    GetStreaming { url: Url, headers: Vec<(String, String)>, max_bytes: Option<u64> },
    // … existing M10 variants (Put, Post with resumable progress)
    Download { url: Url, dest_path: String, resumable_token: Option<String> },
    CancelDownload { token: String },
}

pub enum HttpResult {
    Response { status: u16, headers: Vec<(String, String)>, body: Vec<u8> },
    StreamChunk { stream_id: Ulid, bytes: Vec<u8> },
    StreamComplete { stream_id: Ulid, total_bytes: u64 },
    StreamError { stream_id: Ulid, reason: String },
    DownloadProgress { token: String, bytes_done: u64, total_bytes: Option<u64> },
    DownloadComplete { token: String, dest_path: String },
}
```

iOS impl: `URLSession` with `URLSessionConfiguration.background(withIdentifier:)` for downloads (so the bridge survives app suspend). Resumable downloads use `URLSessionDownloadTask.cancel(byProducingResumeData:)`. Idempotent cancel; bounded state (one task per token, cleaned up on completion).

---

## E. `EmbeddingCapability`

```rust
pub enum EmbeddingRequest {
    Embed { text: String, max_tokens: Option<u32> },
    EmbedBatch { texts: Vec<String>, max_tokens: Option<u32> },
}

pub enum EmbeddingResult {
    Embedded { vector: Vec<f32> },                 // 384 dims for bge-small-en-v1.5
    EmbeddedBatch { vectors: Vec<Vec<f32>> },
    Unavailable { reason: String },
    Error { reason: String },
}
```

iOS impl: CoreML inference against the bundled `bge-small-en-v1.5.mlpackage`. The bridge holds the model handle (loaded once on first request, kept warm until app backgrounded for > 60 s). Bounded state: one `MLModel` instance. Idempotent: identical inputs → identical outputs (deterministic per CoreML guarantee at FP16; documented + asserted in tests).

Non-iOS impl (post-M11): `fastembed-rs` in pure Rust, same model.

The kernel-owned policy: which model to use (constant in M11), how to chunk overflow text (truncate at 256 tokens; documented as the bge-small ceiling), retry policy (zero retries on `Unavailable`, two retries on `Error`). The bridge reports facts; Rust decides.

---

## F. `KeyValueStoreCapability`

```rust
pub enum KvRequest {
    Get { key: String },
    Set { key: String, value_json: String },
    Delete { key: String },
    Watch { key_prefix: String },          // emits Changed events for live UI
}

pub enum KvResult {
    Value { key: String, value_json: Option<String> },
    Set { key: String },
    Deleted { key: String },
    Changed { key: String, value_json: Option<String> },
}
```

iOS impl: `UserDefaults.standard` (per `Models/Settings.swift` parity). The watch path uses `KVO` on `UserDefaults`. Bounded state: one `NSObjectProtocol` per active watch. Idempotent set; idempotent delete.

The reason this is a capability (not Rust-owned storage): the reference Swift app uses `UserDefaults`; we keep that surface so iOS user data migrates seamlessly. **Authoritative settings live in Rust** (`SettingsRecord` is the source of truth); UserDefaults is just the platform persistence backing.

---

## G. `TranscriptionCapability`

```rust
pub enum TranscriptionRequest {
    Probe { language_bcp47: String },
    Transcribe { audio_path: String, language_bcp47: String },
    Cancel { token: String },
}

pub enum TranscriptionResult {
    Available { language_bcp47: String },
    ModelDownloading { language_bcp47: String, progress_pct: f32 },
    Transcribed {
        token: String,
        full_text: String,
        chunks: Vec<TranscriptionChunk>,
        language_bcp47: String,
    },
    Cancelled { token: String },
    Error { token: String, reason: String },
}

pub struct TranscriptionChunk {
    pub text: String,
    pub start_s: f64,
    pub end_s: f64,
}
```

iOS impl: `SpeechAnalyzer` + `SpeechTranscriber` (iOS 26 APIs already used in `TranscriptionService.swift`). On-device model download via `AssetInventory.assetInstallationRequest(supporting:)` happens inside the bridge on first `Probe` for the language; emits `ModelDownloading` progress events. Bounded state: one `SpeechAnalyzer` per active transcription token; cleaned up on completion or cancel. Idempotent probe.

Non-iOS impl (post-M11): whisper.cpp via the `whisper-rs` crate or remote provider via `rig.rs`.

---

## H. `VoiceRecordingCapability`

```rust
pub enum VoiceRecordingRequest {
    Authorize,
    Start { dest_path: String, format: VoiceFormat },
    Stop { token: String },
    Cancel { token: String },
}

pub enum VoiceFormat { AacMonoM4a { sample_rate_hz: u32, bitrate_bps: u32 } }

pub enum VoiceRecordingResult {
    AuthorizationGranted { granted: bool },
    Started { token: String },
    Stopped { token: String, dest_path: String, duration_s: f64 },
    Cancelled { token: String },
    Error { token: String, reason: String },
}
```

iOS impl: `AVAudioRecorder` (per `InsightService.swift::startRecording`). Bounded state: one `AVAudioRecorder` per active token. Idempotent stop; idempotent cancel.

---

## I. `AppleIntelligenceCapability`

```rust
pub enum AppleIntelRequest {
    Probe,
    Generate { prompt: String, max_output_tokens: u32 },
    StreamGenerate { stream_id: Ulid, prompt: String, max_output_tokens: u32 },
    Cancel { stream_id: Ulid },
}

pub enum AppleIntelResult {
    Available,
    Unavailable { reason: AppleIntelUnavailability },
    Generated { content: String },
    Token { stream_id: Ulid, token: String },
    StreamComplete { stream_id: Ulid, total_chars: usize },
    Cancelled { stream_id: Ulid },
    Error { stream_id: Option<Ulid>, reason: String },
}

pub enum AppleIntelUnavailability {
    DeviceNotSupported,
    AppleIntelligenceDisabled,
    ModelDownloading,
    Other(String),
}
```

iOS impl: `FoundationModels.SystemLanguageModel.default` + `LanguageModelSession()` (per `AIService.swift`). `StreamGenerate` wraps `session.streamResponse(to:)` and emits `Token` events for each `chunk.content`. Bounded state: one `LanguageModelSession` per active stream id. Idempotent cancel.

Non-iOS impl: **none** — the capability is iOS-only. On other platforms the actor reports `Unavailable { reason: DeviceNotSupported }`. The router (see [`podcast-llm.md`](podcast-llm.md) §C) routes via `rig` instead.

---

## J. Where the trait files live

All trait files live in `crates/nmp-core/src/substrate/capabilities/`:

```
crates/nmp-core/src/substrate/capabilities/
├── mod.rs
├── audio_playback.rs
├── background_work.rs
├── local_notification.rs
├── http.rs                    # extended in M11; not new
├── embedding.rs
├── key_value_store.rs
├── transcription.rs
├── voice_recording.rs
└── apple_intelligence.rs
```

The Swift implementations live in `ios/NmpPodcast/Bridge/Capabilities/` and are registered with the `FfiApp` at app launch (`@main` shows the registration boilerplate). The pattern mirrors the existing `KernelBridge.swift` in `ios/NmpStress`.

---

## K. Acceptance per capability (M10.5-flavored)

For each, the M11 stress harness in `crates/nmp-testing/bin/ffi-stress/` adds:

- 1000-cycle start/stop/restart sequence with leak instrument (zero retained-by-cycle leaks).
- Concurrent dispatch: 100 distinct request shapes interleaved; assert FIFO ordering of results per stream_id.
- Cancel-mid-stream: assert no straggler events arrive after `Cancelled`.
- Suspend/resume: app backgrounded mid-capability; assert resumption produces correct state.

Each capability has a `crates/nmp-testing/tests/capability_<name>.rs` integration test exercising the above against a `MockBridge`.

---

## L. Capability-bridge philosophy guardrail

For every new capability, the design review asks: **does any field of the Request or Result name a podcast noun?** If yes, redesign. The current set passes — the request types name URLs, paths, tokens, languages, and prompts; nothing about podcasts, episodes, or transcripts.
