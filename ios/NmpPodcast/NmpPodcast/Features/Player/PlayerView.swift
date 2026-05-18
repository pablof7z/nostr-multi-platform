import SwiftUI

// MARK: - PlayerView
//
// verbatim-5 (#164): the byte-for-byte Podcastr `PlayerView` is the
// integration hub that pulls in the still-deferred Player tail
// (PlayerChaptersScrollView / PlayerNoChaptersPlaceholder /
// PlayerGenerationSourceChip / PlayerClipSourceChip / AutoSnipBanner /
// NoLLMKeyHintBanner / VoiceNoteRecordingSheet / ChaptersHydrationService /
// AIChapterCompiler) plus an optional `Episode.duration` + AppStateStore
// `notes`/`clips`. Restoring it verbatim is the precise verbatim-6 surface.
//
// This honest stub keeps the EXACT public init signature `RootView` calls
// (`PlayerView(state:glassNamespace:)`) so the host compiles unchanged while
// the 15 verbatim Player control/sheet/type files land this iteration.
//
// Podcastr source (restore byte-for-byte in verbatim-6):
// /Users/pablofernandez/Work/podcast/App/Sources/Features/Player/PlayerView.swift

struct PlayerView: View {
    @Bindable var state: PlaybackState
    let glassNamespace: Namespace.ID

    var body: some View {
        ContentUnavailableView(
            "Player",
            systemImage: "waveform",
            description: Text("Full-screen player restores in verbatim-6 (#164) once the chapter / clip / voice-note tail and audio engine land.")
        )
    }
}
