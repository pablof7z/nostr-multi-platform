import SwiftUI

/// Bottom sheet for picking playback rate.
///
/// One-thumb operable, every option a full-width row with a leading checkmark
/// for the current selection. Lane 1 will hand the same rate values directly
/// to `AVPlayer.rate`; nothing else here changes.
struct PlayerSpeedSheet: View {

    @Bindable var state: PlaybackState
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        // Cache the current rate once per body eval so each row's
        // selection check doesn't re-run `PlaybackRate.bestFit(for:)` —
        // that helper allocates an `allCases` walk per call.
        let current = state.rate
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 0) {
                    Text("Playback Speed")
                        .font(AppTheme.Typography.title)
                        .foregroundStyle(.primary)
                        .padding(.horizontal, AppTheme.Spacing.lg)
                        .padding(.top, AppTheme.Spacing.lg)
                        .padding(.bottom, AppTheme.Spacing.sm)
                        .accessibilityAddTraits(.isHeader)

                    ForEach(PlaybackRate.allCases) { rate in
                        rateRow(for: rate, isSelected: rate == current)
                    }

                    Spacer(minLength: AppTheme.Spacing.lg)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            .toolbarTitleDisplayMode(.inline)
        }
        // `.medium` plus `.large` so the row list stays reachable at
        // accessibility text sizes — at default sizes the medium detent
        // already fits comfortably, so most users won't notice the
        // difference. Drop the redundant Done button: tapping a row
        // dismisses, the drag indicator is the cancel path.
        .presentationDetents([.medium, .large])
        .presentationDragIndicator(.visible)
    }

    private func rateRow(for rate: PlaybackRate, isSelected: Bool) -> some View {
        Button {
            state.setRate(rate)
            dismiss()
        } label: {
            HStack {
                Text(rate.label)
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
        // VoiceOver announces this row as "1.5×, selected" when active —
        // previously every row sounded identical and the user had no
        // way to know which speed was current.
        .accessibilityAddTraits(isSelected ? [.isButton, .isSelected] : .isButton)
    }
}
