import SwiftUI

// MARK: - VoiceView
//
// T-podcast-gap-002: Verbatim Podcastr VoiceView requires agent infrastructure.
// Stub until the kernel exposes voice mode.
//
// Podcastr source:
// /Users/pablofernandez/Work/podcast/App/Sources/Features/Voice/VoiceView.swift

struct VoiceView: View {
    let onSwitchToText: () -> Void

    var body: some View {
        VStack(spacing: AppTheme.Spacing.lg) {
            Spacer()
            Image(systemName: "waveform")
                .font(.system(size: 60))
                .foregroundStyle(Color.accentColor)
            Text("Voice Mode")
                .font(AppTheme.Typography.title)
            Text("Voice interface loads once the kernel exposes the agent interface.")
                .font(AppTheme.Typography.body)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
            Spacer()
            Button("Switch to Text") {
                onSwitchToText()
            }
            .buttonStyle(.bordered)
            .padding(.bottom, AppTheme.Spacing.xl)
        }
        .padding(AppTheme.Spacing.lg)
    }
}
