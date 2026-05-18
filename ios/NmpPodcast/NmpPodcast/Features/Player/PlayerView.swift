import SwiftUI

// MARK: - PlayerView
//
// T-podcast-gap-002: Verbatim Podcastr PlayerView requires PlaybackState
// backed by kernel audio engine. Stub until kernel exposes audio playback.
//
// Podcastr source:
// /Users/pablofernandez/Work/podcast/App/Sources/Features/Player/PlayerView.swift

struct PlayerView: View {
    let state: PlaybackState
    var glassNamespace: Namespace.ID

    var body: some View {
        ContentUnavailableView(
            "Player",
            systemImage: "waveform",
            description: Text("Audio player loads once the kernel exposes playback (T-podcast-gap-002).")
        )
    }
}
