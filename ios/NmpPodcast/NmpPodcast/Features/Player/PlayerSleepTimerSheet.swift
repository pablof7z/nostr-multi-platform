import SwiftUI

/// Bottom sheet for setting a sleep timer.
struct PlayerSleepTimerSheet: View {

    @Bindable var state: PlaybackState
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        let current = state.sleepTimer
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 0) {
                    Text("Sleep Timer")
                        .font(AppTheme.Typography.title)
                        .foregroundStyle(.primary)
                        .padding(.horizontal, AppTheme.Spacing.lg)
                        .padding(.top, AppTheme.Spacing.lg)
                        .padding(.bottom, AppTheme.Spacing.sm)
                        .accessibilityAddTraits(.isHeader)

                    ForEach(PlaybackSleepTimer.presets) { preset in
                        timerRow(for: preset, isSelected: preset == current)
                    }

                    Spacer(minLength: AppTheme.Spacing.lg)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            .toolbarTitleDisplayMode(.inline)
        }
        // `.medium` + `.large` so accessibility text sizes don't clip
        // rows. Drop the redundant Done button — tapping a row dismisses
        // and the drag indicator is the cancel path.
        .presentationDetents([.medium, .large])
        .presentationDragIndicator(.visible)
    }

    private func timerRow(for preset: PlaybackSleepTimer, isSelected: Bool) -> some View {
        Button {
            state.setSleepTimer(preset)
            dismiss()
        } label: {
            HStack {
                Image(systemName: glyph(for: preset))
                    .font(.system(size: 16, weight: .semibold))
                    .foregroundStyle(.tint)
                    .frame(width: 28)
                    .accessibilityHidden(true)
                Text(preset.label)
                    .font(AppTheme.Typography.headline)
                    .foregroundStyle(.primary)
                Spacer()
                if isSelected {
                    Image(systemName: "checkmark")
                        .font(.system(size: 16, weight: .semibold))
                        .foregroundStyle(.tint)
                        .accessibilityHidden(true)
                }
            }
            .padding(.horizontal, AppTheme.Spacing.lg)
            .padding(.vertical, AppTheme.Spacing.md)
            .contentShape(Rectangle())
        }
        .buttonStyle(.pressable(scale: 0.98, opacity: 0.85))
        .accessibilityAddTraits(isSelected ? [.isButton, .isSelected] : .isButton)
    }

    private func glyph(for preset: PlaybackSleepTimer) -> String {
        switch preset {
        case .off: return "moon.zzz"
        case .minutes: return "timer"
        case .endOfEpisode: return "stop.circle"
        }
    }
}
