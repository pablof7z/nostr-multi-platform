import SwiftUI

// MARK: - PlayerShowNotesView

/// Plain-text show notes surface for the full-screen player.
///
/// Rendered inside the parent's `ScrollView` (no nested scroll), so the
/// content scrolls naturally with the artwork header above it.
struct PlayerShowNotesView: View {

    let episode: Episode?

    private var notes: String {
        EpisodeShowNotesFormatter.plainText(from: episode?.description ?? "")
    }

    var body: some View {
        if notes.isEmpty {
            emptyState
        } else {
            Text(notes)
                .font(AppTheme.Typography.body)
                .lineSpacing(7)
                .foregroundStyle(.secondary)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(AppTheme.Spacing.md)
                .background(cardBackground)
        }
    }

    private var emptyState: some View {
        VStack(spacing: AppTheme.Spacing.sm) {
            Image(systemName: "doc.text")
                .font(.system(size: 28, weight: .light))
                .foregroundStyle(.secondary)
            Text("No show notes")
                .font(AppTheme.Typography.headline)
                .foregroundStyle(.primary)
            Text("This episode has no show notes.")
                .font(AppTheme.Typography.caption)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
        }
        .frame(maxWidth: .infinity, minHeight: 200)
        .padding(AppTheme.Spacing.lg)
        .background(cardBackground)
    }

    private var cardBackground: some View {
        RoundedRectangle(cornerRadius: AppTheme.Corner.lg, style: .continuous)
            .fill(.ultraThinMaterial)
            .overlay(
                RoundedRectangle(cornerRadius: AppTheme.Corner.lg, style: .continuous)
                    .stroke(Color.primary.opacity(0.06), lineWidth: 0.5)
            )
    }
}
