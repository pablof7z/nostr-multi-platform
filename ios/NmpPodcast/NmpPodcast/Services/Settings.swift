import Foundation

// MARK: - Settings

enum OpenRouterCredentialSource: String, Codable, Hashable, Sendable {
    case none, manual, byok
}

enum ElevenLabsCredentialSource: String, Codable, Hashable, Sendable {
    case none, manual, byok
}

enum OllamaCredentialSource: String, Codable, Hashable, Sendable {
    case none, manual, byok
}

/// Action mapped to a headphone-remote multi-tap gesture (AirPods double/triple
/// tap or stem squeeze, wired earbud inline button). iOS converts these into
/// standard `MPRemoteCommandCenter.nextTrackCommand` / `.previousTrackCommand`
/// events — this enum decides what each one does inside the podcast player.
///
/// Enabling the underlying remote commands also makes a next/previous-track
/// button visible on the lock screen, Control Center, and CarPlay — whatever
/// the gesture is mapped to fires from those surfaces too.
enum HeadphoneGestureAction: String, Codable, Hashable, Sendable, CaseIterable {
    case skipForward
    case skipBackward
    case nextChapter
    case previousChapter
    case clipNow
    case none

    var displayName: String {
        switch self {
        case .skipForward:     return "Skip Forward"
        case .skipBackward:    return "Skip Back"
        case .nextChapter:     return "Next Chapter"
        case .previousChapter: return "Previous Chapter"
        case .clipNow:         return "Clip Current Position"
        case .none:            return "Do Nothing"
        }
    }
}

enum STTProvider: String, Codable, Hashable, Sendable, CaseIterable {
    case elevenLabsScribe = "elevenlabs_scribe"
    case assemblyAI = "assemblyai"
    case openRouterWhisper = "openrouter_whisper"
    /// Apple's on-device `SpeechTranscriber` (iOS 26+). No API key required;
    /// uses Apple Silicon neural engine. Requires a locally downloaded episode.
    case appleNative = "apple_native"

    var displayName: String {
        switch self {
        case .elevenLabsScribe: return "ElevenLabs Scribe"
        case .assemblyAI: return "AssemblyAI"
        case .openRouterWhisper: return "OpenRouter Whisper"
        case .appleNative: return "Apple on-device"
        }
    }
}

struct Settings: Codable, Hashable, Sendable {

    // MARK: - Defaults
    private enum Defaults {
        static let llmModel = "openai/gpt-4o-mini"
        static let elevenLabsSTTModel = "scribe_v1"
        static let elevenLabsTTSModel = "eleven_turbo_v2_5"
        static let nostrRelayURL = "wss://relay.tenex.chat"
        static let defaultPlaybackRate: Double = 1.0
        static let skipForwardSeconds: Int = 30
        static let skipBackwardSeconds: Int = 15
    }

    /// Default Ollama chat endpoint (Ollama Cloud). Users can override this to
    /// point at a local or self-hosted instance from Settings → Providers → Ollama.
    static let defaultOllamaChatURL = "https://ollama.com/api/chat"

    // AI / LLM
    /// Model the agent chat session starts on. Designed to be a cheap/fast model
    /// — the agent decides per-task whether to call `upgrade_thinking`, which
    /// switches the session over to `agentThinkingModel` for subsequent turns.
    var agentInitialModel: String = Defaults.llmModel
    var agentInitialModelName: String = ""
    /// Stronger model the agent escalates to via the `upgrade_thinking` tool when
    /// a task needs more reasoning than the initial model can reliably provide.
    /// Defaults to the same value as `agentInitialModel` so behavior is unchanged
    /// until the user picks something stronger in Settings.
    var agentThinkingModel: String = Defaults.llmModel
    var agentThinkingModelName: String = ""
    var memoryCompilationModel: String = Defaults.llmModel
    var memoryCompilationModelName: String = ""
    /// Model used by `WikiGenerator`. Kept distinct from `llmModel` so users can pick a
    /// cheaper / faster model for wiki compilation than for live agent chat — same pattern
    /// as `memoryCompilationModel`.
    var wikiModel: String = Defaults.llmModel
    var wikiModelName: String = ""
    /// Model used by `PodcastCategorizationService`. Kept distinct so users can pick a
    /// cheaper model for one-shot categorization without affecting live agent chat.
    var categorizationModel: String = Defaults.llmModel
    var categorizationModelName: String = ""
    /// Model used by `AIChapterCompiler` to synthesise chapter boundaries from
    /// a ready transcript. Kept distinct from `wikiModel` so users can pick a
    /// cheaper / faster model for chapter compile without affecting wiki quality.
    var chapterCompilationModel: String = Defaults.llmModel
    var chapterCompilationModelName: String = ""
    var embeddingsModel: String = Self.defaultEmbeddingsModel
    var embeddingsModelName: String = ""
    /// Model used by `ImageGenerationService`. Multimodal models (Gemini/Banana,
    /// GPT-image) route through /chat/completions; legacy DALL-E/FLUX use
    /// /images/generations. Defaults to Gemini 2.5 Flash Image ("Nano Banana").
    var imageGenerationModel: String = "google/gemini-2.5-flash-image"
    var imageGenerationModelName: String = ""
    /// When `true`, optionally re-rank top-k RAG candidates with a cross-encoder. Off by
    /// default to save tokens; settings UI exposes the toggle.
    var rerankerEnabled: Bool = false

    // Blossom
    /// Blossom BUD-02 server used for uploading podcast artwork, audio, chapters, and
    /// transcripts. Defaults to blossom.primal.net. Configurable in Settings > Agent.
    var blossomServerURL: String = "https://blossom.primal.net"

    // OpenRouter credentials (secret stored in Keychain; only metadata here)
    var openRouterCredentialSource: OpenRouterCredentialSource = .none
    var openRouterBYOKKeyID: String?
    var openRouterBYOKKeyLabel: String?
    var openRouterConnectedAt: Date?
    var legacyOpenRouterAPIKey: String?

    // Ollama Cloud credentials (secret stored in Keychain; only metadata here)
    var ollamaCredentialSource: OllamaCredentialSource = .none
    var ollamaBYOKKeyID: String?
    var ollamaBYOKKeyLabel: String?
    var ollamaConnectedAt: Date?
    /// Chat endpoint for Ollama requests. Defaults to the Ollama Cloud API so
    /// existing users see no change. Set to e.g. `http://localhost:11434/api/chat`
    /// to point at a local instance. Stored as a String so partial edits during
    /// typing don't break `Codable`; validated as a URL at the network call site.
    var ollamaChatURL: String = Settings.defaultOllamaChatURL

    // YouTube ingestion
    /// Endpoint of a self-hosted YouTube audio-extraction service (e.g. cobalt,
    /// yt-dlp wrapper). The agent's `youtube_ingestion` skill POSTs
    /// `{"url": "<youtube_url>"}` here and expects back JSON with at least an
    /// `audio_url` (or `url`) field. `nil` means the skill is unavailable.
    var youtubeExtractorURL: String?

    // ElevenLabs credentials (secret stored in Keychain; only metadata here)
    var elevenLabsCredentialSource: ElevenLabsCredentialSource = .none
    var elevenLabsBYOKKeyID: String?
    var elevenLabsBYOKKeyLabel: String?
    var elevenLabsConnectedAt: Date?

    // STT provider selection
    var sttProvider: STTProvider = .elevenLabsScribe
    /// Whisper model used when `sttProvider == .openRouterWhisper`. Must be a model
    /// accessible on OpenRouter's audio transcription endpoint.
    var openRouterWhisperModel: String = "openai/whisper-1"
    /// Comma-separated AssemblyAI speech models submitted to `/v2/transcript`.
    var assemblyAISTTModel: String = "universal-3-pro,universal-2"

    // ElevenLabs configuration
    var elevenLabsSTTModel: String = Defaults.elevenLabsSTTModel
    var elevenLabsTTSModel: String = Defaults.elevenLabsTTSModel
    var elevenLabsVoiceID: String = ""
    var elevenLabsVoiceName: String = ""

    // Playback
    /// Default playback rate (0.5x – 3.0x). Per-show overrides live on `PodcastSubscription`.
    var defaultPlaybackRate: Double = Defaults.defaultPlaybackRate
    /// Seconds the forward-skip transport button advances by. Mirrored to the lock-screen.
    var skipForwardSeconds: Int = Defaults.skipForwardSeconds
    /// Seconds the back-skip transport button rewinds by. Mirrored to the lock-screen.
    var skipBackwardSeconds: Int = Defaults.skipBackwardSeconds
    /// When `true`, an episode is automatically marked played the first time playback
    /// reaches its end. Defaults on for parity with Apple Podcasts.
    var autoMarkPlayedAtEnd: Bool = true
    /// When `true`, downloaded enclosures are deleted as soon as the episode is
    /// marked played (auto-end-of-play OR explicit "Mark as played"). Off by
    /// default — without it, downloads are kept until manually removed.
    var autoDeleteDownloadsAfterPlayed: Bool = false
    /// When `true`, the next episode in `PlaybackState.queue` (Up Next)
    /// starts playing automatically when the current episode finishes.
    /// Defaults on for parity with Apple Podcasts. Suppressed when the
    /// sleep timer has armed an end-of-episode stop.
    var autoPlayNext: Bool = true
    /// When `true`, the player auto-seeks past detected ad segments
    /// (`AIChapterCompiler` output, stored on `Episode.adSegments`).
    /// Defaults off for v1 — opt-in until detection quality is proven. The
    /// chapter rail still flags ad-overlapping chapters visually regardless.
    var autoSkipAds: Bool = false
    /// Action fired by an AirPods double-tap / double-squeeze (or any headphone
    /// remote that emits `MPRemoteCommandCenter.nextTrackCommand`). Default
    /// matches the common podcast-player muscle memory: jump forward by the
    /// configured skip-forward interval.
    var headphoneDoubleTapAction: HeadphoneGestureAction = .skipForward
    /// Action fired by an AirPods triple-tap / triple-squeeze (or any headphone
    /// remote that emits `MPRemoteCommandCenter.previousTrackCommand`). Default
    /// captures a clip — quickly bookmarking what you just heard is the most
    /// valuable thing a third tap can do that single/double don't already cover.
    var headphoneTripleTapAction: HeadphoneGestureAction = .clipNow

    // Wiki
    /// When `true`, `WikiGenerator` runs (or refreshes) the relevant wiki pages as soon as
    /// a new transcript finishes ingesting. Defaults off so first-run users don't burn
    /// tokens before deciding to opt in.
    var wikiAutoGenerateOnTranscriptIngest: Bool = false

    // Transcripts
    /// When `true`, the app pre-fetches publisher-supplied transcripts in the
    /// background as soon as new episodes appear (called from
    /// `AppStateStore.upsertEpisodes` after a feed refresh). Default-on
    /// because the agent layer (RAG, wiki, briefings, summarisation) only
    /// works once the transcript exists; publisher transcripts are typically
    /// tens of KB so the bandwidth cost is small. Toggle off in
    /// Settings → Transcripts to defer everything to manual fetch.
    var autoIngestPublisherTranscripts: Bool = true
    /// When `true`, episodes lacking a publisher transcript fall back to ElevenLabs Scribe
    /// transcription. Requires an ElevenLabs key; defaults on so existing behaviour is
    /// preserved.
    var autoFallbackToScribe: Bool = true

    // Notifications (per-kind toggles; the system permission is separate)
    /// When `true`, fire a local notification when a feed refresh discovers a brand-new
    /// episode for a subscription that has notifications enabled.
    var notifyOnNewEpisodes: Bool = true
    /// When `true`, fire a local notification when a daily/weekly briefing finishes
    /// generating and is ready to play.
    var notifyOnBriefingReady: Bool = true

    // Nostr identity (private key stored in Keychain via NostrCredentialStore)
    var nostrEnabled: Bool = false
    var nostrRelayURL: String = Defaults.nostrRelayURL
    /// Relay list used when publishing NIP-F4 podcast events. Initialized from the
    /// user's NIP-65 kind:10002 outbox relays; falls back to primal + damus when empty.
    var nostrPublicRelays: [String] = []
    var nostrProfileName: String = ""
    var nostrProfileAbout: String = ""
    var nostrProfilePicture: String = ""
    var nostrPublicKeyHex: String?

    // Onboarding
    var hasCompletedOnboarding: Bool = false

    init() {}

    private enum CodingKeys: String, CodingKey {
        // RawValues preserved as "llmModel" / "llmModelName" so existing
        // user data continues to decode after the rename to agentInitialModel.
        case agentInitialModel = "llmModel"
        case agentInitialModelName = "llmModelName"
        case agentThinkingModel, agentThinkingModelName
        case memoryCompilationModel, memoryCompilationModelName
        case wikiModel, wikiModelName, categorizationModel, categorizationModelName
        case chapterCompilationModel, chapterCompilationModelName
        case embeddingsModel, embeddingsModelName, rerankerEnabled
        case imageGenerationModel, imageGenerationModelName
        case blossomServerURL
        case openRouterAPIKey                                             // legacy
        case openRouterCredentialSource
        case openRouterBYOKKeyID, openRouterBYOKKeyLabel, openRouterConnectedAt
        case ollamaCredentialSource, ollamaBYOKKeyID, ollamaBYOKKeyLabel, ollamaConnectedAt, ollamaChatURL
        case elevenLabsCredentialSource
        case elevenLabsBYOKKeyID, elevenLabsBYOKKeyLabel, elevenLabsConnectedAt
        case sttProvider, openRouterWhisperModel, assemblyAISTTModel
        case elevenLabsSTTModel, elevenLabsTTSModel, elevenLabsVoiceID, elevenLabsVoiceName
        case defaultPlaybackRate, skipForwardSeconds, skipBackwardSeconds, autoMarkPlayedAtEnd
        case autoDeleteDownloadsAfterPlayed, autoPlayNext, autoSkipAds
        case headphoneDoubleTapAction, headphoneTripleTapAction
        case wikiAutoGenerateOnTranscriptIngest
        case autoIngestPublisherTranscripts, autoFallbackToScribe
        case notifyOnNewEpisodes, notifyOnBriefingReady
        case nostrEnabled, nostrRelayURL, nostrPublicRelays
        case nostrProfileName, nostrProfileAbout, nostrProfilePicture
        case nostrPublicKeyHex
        case hasCompletedOnboarding
        case youtubeExtractorURL
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        agentInitialModel = try c.decodeIfPresent(String.self, forKey: .agentInitialModel) ?? Defaults.llmModel
        agentInitialModelName = try c.decodeIfPresent(String.self, forKey: .agentInitialModelName) ?? ""
        agentThinkingModel = try c.decodeIfPresent(String.self, forKey: .agentThinkingModel) ?? agentInitialModel
        agentThinkingModelName = try c.decodeIfPresent(String.self, forKey: .agentThinkingModelName) ?? ""
        memoryCompilationModel = try c.decodeIfPresent(String.self, forKey: .memoryCompilationModel) ?? Defaults.llmModel
        memoryCompilationModelName = try c.decodeIfPresent(String.self, forKey: .memoryCompilationModelName) ?? ""
        wikiModel = try c.decodeIfPresent(String.self, forKey: .wikiModel) ?? Defaults.llmModel
        wikiModelName = try c.decodeIfPresent(String.self, forKey: .wikiModelName) ?? ""
        categorizationModel = try c.decodeIfPresent(String.self, forKey: .categorizationModel) ?? Defaults.llmModel
        categorizationModelName = try c.decodeIfPresent(String.self, forKey: .categorizationModelName) ?? ""
        chapterCompilationModel = try c.decodeIfPresent(String.self, forKey: .chapterCompilationModel) ?? Defaults.llmModel
        chapterCompilationModelName = try c.decodeIfPresent(String.self, forKey: .chapterCompilationModelName) ?? ""
        embeddingsModel = try c.decodeIfPresent(String.self, forKey: .embeddingsModel) ?? Self.defaultEmbeddingsModel
        embeddingsModelName = try c.decodeIfPresent(String.self, forKey: .embeddingsModelName) ?? ""
        imageGenerationModel = try c.decodeIfPresent(String.self, forKey: .imageGenerationModel) ?? "google/gemini-2.5-flash-image"
        imageGenerationModelName = try c.decodeIfPresent(String.self, forKey: .imageGenerationModelName) ?? ""
        blossomServerURL = try c.decodeIfPresent(String.self, forKey: .blossomServerURL) ?? "https://blossom.primal.net"
        rerankerEnabled = try c.decodeIfPresent(Bool.self, forKey: .rerankerEnabled) ?? false
        openRouterCredentialSource = try c.decodeIfPresent(OpenRouterCredentialSource.self, forKey: .openRouterCredentialSource) ?? .none
        openRouterBYOKKeyID = try c.decodeIfPresent(String.self, forKey: .openRouterBYOKKeyID)
        openRouterBYOKKeyLabel = try c.decodeIfPresent(String.self, forKey: .openRouterBYOKKeyLabel)
        openRouterConnectedAt = try c.decodeIfPresent(Date.self, forKey: .openRouterConnectedAt)
        legacyOpenRouterAPIKey = try c.decodeIfPresent(String.self, forKey: .openRouterAPIKey)
        ollamaCredentialSource = try c.decodeIfPresent(OllamaCredentialSource.self, forKey: .ollamaCredentialSource) ?? .none
        ollamaBYOKKeyID = try c.decodeIfPresent(String.self, forKey: .ollamaBYOKKeyID)
        ollamaBYOKKeyLabel = try c.decodeIfPresent(String.self, forKey: .ollamaBYOKKeyLabel)
        ollamaConnectedAt = try c.decodeIfPresent(Date.self, forKey: .ollamaConnectedAt)
        ollamaChatURL = try c.decodeIfPresent(String.self, forKey: .ollamaChatURL) ?? Settings.defaultOllamaChatURL
        elevenLabsCredentialSource = try c.decodeIfPresent(ElevenLabsCredentialSource.self, forKey: .elevenLabsCredentialSource) ?? .none
        elevenLabsBYOKKeyID = try c.decodeIfPresent(String.self, forKey: .elevenLabsBYOKKeyID)
        elevenLabsBYOKKeyLabel = try c.decodeIfPresent(String.self, forKey: .elevenLabsBYOKKeyLabel)
        elevenLabsConnectedAt = try c.decodeIfPresent(Date.self, forKey: .elevenLabsConnectedAt)
        sttProvider = try c.decodeIfPresent(STTProvider.self, forKey: .sttProvider) ?? .elevenLabsScribe
        openRouterWhisperModel = try c.decodeIfPresent(String.self, forKey: .openRouterWhisperModel) ?? "openai/whisper-1"
        assemblyAISTTModel = try c.decodeIfPresent(String.self, forKey: .assemblyAISTTModel) ?? "universal-3-pro,universal-2"
        elevenLabsSTTModel = try c.decodeIfPresent(String.self, forKey: .elevenLabsSTTModel) ?? Defaults.elevenLabsSTTModel
        elevenLabsTTSModel = try c.decodeIfPresent(String.self, forKey: .elevenLabsTTSModel) ?? Defaults.elevenLabsTTSModel
        elevenLabsVoiceID = try c.decodeIfPresent(String.self, forKey: .elevenLabsVoiceID) ?? ""
        elevenLabsVoiceName = try c.decodeIfPresent(String.self, forKey: .elevenLabsVoiceName) ?? ""
        defaultPlaybackRate = try c.decodeIfPresent(Double.self, forKey: .defaultPlaybackRate) ?? Defaults.defaultPlaybackRate
        skipForwardSeconds = try c.decodeIfPresent(Int.self, forKey: .skipForwardSeconds) ?? Defaults.skipForwardSeconds
        skipBackwardSeconds = try c.decodeIfPresent(Int.self, forKey: .skipBackwardSeconds) ?? Defaults.skipBackwardSeconds
        autoMarkPlayedAtEnd = try c.decodeIfPresent(Bool.self, forKey: .autoMarkPlayedAtEnd) ?? true
        autoDeleteDownloadsAfterPlayed = try c.decodeIfPresent(Bool.self, forKey: .autoDeleteDownloadsAfterPlayed) ?? false
        autoPlayNext = try c.decodeIfPresent(Bool.self, forKey: .autoPlayNext) ?? true
        autoSkipAds = try c.decodeIfPresent(Bool.self, forKey: .autoSkipAds) ?? false
        headphoneDoubleTapAction = try c.decodeIfPresent(HeadphoneGestureAction.self, forKey: .headphoneDoubleTapAction) ?? .skipForward
        headphoneTripleTapAction = try c.decodeIfPresent(HeadphoneGestureAction.self, forKey: .headphoneTripleTapAction) ?? .clipNow
        wikiAutoGenerateOnTranscriptIngest = try c.decodeIfPresent(Bool.self, forKey: .wikiAutoGenerateOnTranscriptIngest) ?? false
        autoIngestPublisherTranscripts = try c.decodeIfPresent(Bool.self, forKey: .autoIngestPublisherTranscripts) ?? true
        autoFallbackToScribe = try c.decodeIfPresent(Bool.self, forKey: .autoFallbackToScribe) ?? true
        notifyOnNewEpisodes = try c.decodeIfPresent(Bool.self, forKey: .notifyOnNewEpisodes) ?? true
        notifyOnBriefingReady = try c.decodeIfPresent(Bool.self, forKey: .notifyOnBriefingReady) ?? true
        nostrEnabled = try c.decodeIfPresent(Bool.self, forKey: .nostrEnabled) ?? false
        nostrRelayURL = try c.decodeIfPresent(String.self, forKey: .nostrRelayURL) ?? Defaults.nostrRelayURL
        nostrPublicRelays = try c.decodeIfPresent([String].self, forKey: .nostrPublicRelays) ?? []
        nostrProfileName = try c.decodeIfPresent(String.self, forKey: .nostrProfileName) ?? ""
        nostrProfileAbout = try c.decodeIfPresent(String.self, forKey: .nostrProfileAbout) ?? ""
        nostrProfilePicture = try c.decodeIfPresent(String.self, forKey: .nostrProfilePicture) ?? ""
        nostrPublicKeyHex = try c.decodeIfPresent(String.self, forKey: .nostrPublicKeyHex)
        hasCompletedOnboarding = try c.decodeIfPresent(Bool.self, forKey: .hasCompletedOnboarding) ?? false
        youtubeExtractorURL = try c.decodeIfPresent(String.self, forKey: .youtubeExtractorURL)

        if openRouterCredentialSource == .none,
           let legacy = legacyOpenRouterAPIKey,
           !legacy.isBlank {
            openRouterCredentialSource = .manual
        }
    }

    func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(agentInitialModel, forKey: .agentInitialModel)
        try c.encode(agentInitialModelName, forKey: .agentInitialModelName)
        try c.encode(agentThinkingModel, forKey: .agentThinkingModel)
        try c.encode(agentThinkingModelName, forKey: .agentThinkingModelName)
        try c.encode(memoryCompilationModel, forKey: .memoryCompilationModel)
        try c.encode(memoryCompilationModelName, forKey: .memoryCompilationModelName)
        try c.encode(wikiModel, forKey: .wikiModel)
        try c.encode(wikiModelName, forKey: .wikiModelName)
        try c.encode(categorizationModel, forKey: .categorizationModel)
        try c.encode(categorizationModelName, forKey: .categorizationModelName)
        try c.encode(chapterCompilationModel, forKey: .chapterCompilationModel)
        try c.encode(chapterCompilationModelName, forKey: .chapterCompilationModelName)
        try c.encode(embeddingsModel, forKey: .embeddingsModel)
        try c.encode(embeddingsModelName, forKey: .embeddingsModelName)
        try c.encode(imageGenerationModel, forKey: .imageGenerationModel)
        try c.encode(imageGenerationModelName, forKey: .imageGenerationModelName)
        try c.encode(blossomServerURL, forKey: .blossomServerURL)
        try c.encode(rerankerEnabled, forKey: .rerankerEnabled)
        try c.encode(openRouterCredentialSource, forKey: .openRouterCredentialSource)
        try c.encodeIfPresent(openRouterBYOKKeyID, forKey: .openRouterBYOKKeyID)
        try c.encodeIfPresent(openRouterBYOKKeyLabel, forKey: .openRouterBYOKKeyLabel)
        try c.encodeIfPresent(openRouterConnectedAt, forKey: .openRouterConnectedAt)
        try c.encode(ollamaCredentialSource, forKey: .ollamaCredentialSource)
        try c.encodeIfPresent(ollamaBYOKKeyID, forKey: .ollamaBYOKKeyID)
        try c.encodeIfPresent(ollamaBYOKKeyLabel, forKey: .ollamaBYOKKeyLabel)
        try c.encodeIfPresent(ollamaConnectedAt, forKey: .ollamaConnectedAt)
        try c.encode(ollamaChatURL, forKey: .ollamaChatURL)
        try c.encode(elevenLabsCredentialSource, forKey: .elevenLabsCredentialSource)
        try c.encodeIfPresent(elevenLabsBYOKKeyID, forKey: .elevenLabsBYOKKeyID)
        try c.encodeIfPresent(elevenLabsBYOKKeyLabel, forKey: .elevenLabsBYOKKeyLabel)
        try c.encodeIfPresent(elevenLabsConnectedAt, forKey: .elevenLabsConnectedAt)
        try c.encode(sttProvider, forKey: .sttProvider)
        try c.encode(openRouterWhisperModel, forKey: .openRouterWhisperModel)
        try c.encode(assemblyAISTTModel, forKey: .assemblyAISTTModel)
        try c.encode(elevenLabsSTTModel, forKey: .elevenLabsSTTModel)
        try c.encode(elevenLabsTTSModel, forKey: .elevenLabsTTSModel)
        try c.encode(elevenLabsVoiceID, forKey: .elevenLabsVoiceID)
        try c.encode(elevenLabsVoiceName, forKey: .elevenLabsVoiceName)
        try c.encode(defaultPlaybackRate, forKey: .defaultPlaybackRate)
        try c.encode(skipForwardSeconds, forKey: .skipForwardSeconds)
        try c.encode(skipBackwardSeconds, forKey: .skipBackwardSeconds)
        try c.encode(autoMarkPlayedAtEnd, forKey: .autoMarkPlayedAtEnd)
        try c.encode(autoDeleteDownloadsAfterPlayed, forKey: .autoDeleteDownloadsAfterPlayed)
        try c.encode(autoPlayNext, forKey: .autoPlayNext)
        try c.encode(autoSkipAds, forKey: .autoSkipAds)
        try c.encode(headphoneDoubleTapAction, forKey: .headphoneDoubleTapAction)
        try c.encode(headphoneTripleTapAction, forKey: .headphoneTripleTapAction)
        try c.encode(wikiAutoGenerateOnTranscriptIngest, forKey: .wikiAutoGenerateOnTranscriptIngest)
        try c.encode(autoIngestPublisherTranscripts, forKey: .autoIngestPublisherTranscripts)
        try c.encode(autoFallbackToScribe, forKey: .autoFallbackToScribe)
        try c.encode(notifyOnNewEpisodes, forKey: .notifyOnNewEpisodes)
        try c.encode(notifyOnBriefingReady, forKey: .notifyOnBriefingReady)
        try c.encode(nostrEnabled, forKey: .nostrEnabled)
        try c.encode(nostrRelayURL, forKey: .nostrRelayURL)
        try c.encode(nostrPublicRelays, forKey: .nostrPublicRelays)
        try c.encode(nostrProfileName, forKey: .nostrProfileName)
        try c.encode(nostrProfileAbout, forKey: .nostrProfileAbout)
        try c.encode(nostrProfilePicture, forKey: .nostrProfilePicture)
        try c.encodeIfPresent(nostrPublicKeyHex, forKey: .nostrPublicKeyHex)
        try c.encode(hasCompletedOnboarding, forKey: .hasCompletedOnboarding)
        try c.encodeIfPresent(youtubeExtractorURL, forKey: .youtubeExtractorURL)
    }

    // MARK: - Display helpers

    /// Returns a human-readable display name for an OpenRouter model.
    ///
    /// Preference order:
    /// 1. `modelName` when non-empty (persisted human-readable name from catalog).
    /// 2. Slug after the last `/` in `modelID` (e.g. "gpt-4o" from "openai/gpt-4o").
    /// 3. `modelID` verbatim when it contains no `/`.
    /// 4. "Not set" when `modelID` is empty.
    static func modelDisplayName(modelID: String, modelName: String = "") -> String {
        let name = modelName.trimmed
        if !name.isEmpty { return name }
        let id = modelID.trimmed
        guard !id.isEmpty else { return "Not set" }
        let reference = LLMModelReference(storedID: id)
        if reference.provider != .openRouter { return reference.modelID }
        if let idx = id.lastIndex(of: "/") { return String(id[id.index(after: idx)...]) }
        return id
    }

    mutating func markOpenRouterManual(connectedAt: Date = Date()) {
        openRouterCredentialSource = .manual
        openRouterBYOKKeyID = nil
        openRouterBYOKKeyLabel = nil
        openRouterConnectedAt = connectedAt
        legacyOpenRouterAPIKey = nil
    }

    mutating func markOpenRouterBYOK(keyID: String?, keyLabel: String?, connectedAt: Date = Date()) {
        openRouterCredentialSource = .byok
        openRouterBYOKKeyID = keyID
        openRouterBYOKKeyLabel = keyLabel
        openRouterConnectedAt = connectedAt
        legacyOpenRouterAPIKey = nil
    }

    mutating func clearOpenRouterCredential() {
        openRouterCredentialSource = .none
        openRouterBYOKKeyID = nil
        openRouterBYOKKeyLabel = nil
        openRouterConnectedAt = nil
        legacyOpenRouterAPIKey = nil
    }

    mutating func markOllamaManual(connectedAt: Date = Date()) {
        ollamaCredentialSource = .manual
        ollamaBYOKKeyID = nil
        ollamaBYOKKeyLabel = nil
        ollamaConnectedAt = connectedAt
    }

    mutating func markOllamaBYOK(keyID: String?, keyLabel: String?, connectedAt: Date = Date()) {
        ollamaCredentialSource = .byok
        ollamaBYOKKeyID = keyID
        ollamaBYOKKeyLabel = keyLabel
        ollamaConnectedAt = connectedAt
    }

    mutating func clearOllamaCredential() {
        ollamaCredentialSource = .none
        ollamaBYOKKeyID = nil
        ollamaBYOKKeyLabel = nil
        ollamaConnectedAt = nil
    }

    mutating func markElevenLabsManual(connectedAt: Date = Date()) {
        elevenLabsCredentialSource = .manual
        elevenLabsBYOKKeyID = nil
        elevenLabsBYOKKeyLabel = nil
        elevenLabsConnectedAt = connectedAt
    }

    mutating func markElevenLabsBYOK(keyID: String?, keyLabel: String?, connectedAt: Date = Date()) {
        elevenLabsCredentialSource = .byok
        elevenLabsBYOKKeyID = keyID
        elevenLabsBYOKKeyLabel = keyLabel
        elevenLabsConnectedAt = connectedAt
    }

    mutating func clearElevenLabsCredential() {
        elevenLabsCredentialSource = .none
        elevenLabsBYOKKeyID = nil
        elevenLabsBYOKKeyLabel = nil
        elevenLabsConnectedAt = nil
    }
}

