import SwiftUI

// MARK: - TranscriptsSettingsView
//
// Settings → Transcripts. Controls transcript automation:
//   1. Toggle: auto-ingest publisher transcripts as they appear in feeds.
//   2. Toggle: when no publisher transcript exists, fall back to Scribe.
//
// Speech model selection lives under Settings → Intelligence → Models → Speech.

struct TranscriptsSettingsView: View {
    @Environment(AppStateStore.self) private var store

    var body: some View {
        Form {
            automationSection
        }
        .navigationTitle("Transcripts")
        .navigationBarTitleDisplayMode(.inline)
    }

    // MARK: - Sections

    private var automationSection: some View {
        Section {
            Toggle(isOn: autoIngestBinding) {
                Label("Auto-ingest publisher transcripts", systemImage: "square.and.arrow.down.fill")
            }
            Toggle(isOn: scribeFallbackBinding) {
                Label("AI transcription fallback", systemImage: "arrow.triangle.branch")
            }
            if store.state.settings.autoFallbackToScribe && !hasActiveProviderKey {
                // The toggle is on but the active provider key isn't configured —
                // surface the gap so the fallback doesn't silently no-op.
                Label("\(activeProviderName) key not configured — connect in Providers.", systemImage: "key.slash")
                    .font(AppTheme.Typography.caption)
                    .foregroundStyle(AppTheme.Tint.warning)
                    .padding(.vertical, 2)
            }
        } header: {
            Text("Automation")
        } footer: {
            Text("Auto-ingest pre-fetches transcripts in the background as new episodes appear. AI fallback transcribes audio when the publisher hasn't supplied a transcript. Choose the transcription provider in Models → Speech.")
        }
    }

    private var activeProvider: STTProvider {
        store.state.settings.sttProvider
    }

    private var activeProviderName: String {
        activeProvider.displayName
    }

    private var hasActiveProviderKey: Bool {
        switch activeProvider {
        case .elevenLabsScribe: return ElevenLabsCredentialStore.hasAPIKey()
        case .assemblyAI: return AssemblyAICredentialStore.hasAPIKey()
        case .openRouterWhisper: return OpenRouterCredentialStore.hasAPIKey()
        case .appleNative: return true  // no API key needed
        }
    }

    // MARK: - Bindings

    private var autoIngestBinding: Binding<Bool> {
        Binding(
            get: { store.state.settings.autoIngestPublisherTranscripts },
            set: { v in
                var s = store.state.settings
                s.autoIngestPublisherTranscripts = v
                store.updateSettings(s)
                Haptics.selection()
            }
        )
    }

    private var scribeFallbackBinding: Binding<Bool> {
        Binding(
            get: { store.state.settings.autoFallbackToScribe },
            set: { v in
                var s = store.state.settings
                s.autoFallbackToScribe = v
                store.updateSettings(s)
                Haptics.selection()
            }
        )
    }
}
