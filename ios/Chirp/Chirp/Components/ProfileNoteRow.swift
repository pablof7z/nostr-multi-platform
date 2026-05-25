import SwiftUI

/// Compact note row used inside ProfileView's post list.
/// Avatar tap → profile, row tap → thread.
///
/// `renderContext` carries the per-note mention map, event-card lookup, and
/// timeline-item lookup `NoteContentView` consumes. ProfileView builds it
/// once at the body root rather than passing three separate dictionaries
/// (aim.md §4.2 — derived views are kernel-supplied; ProfileView reads
/// `mentionProfiles` from `projections["mention_profiles"]` instead of
/// rebuilding it from `items`).
struct ProfileNoteRow: View {
    let item: TimelineItem
    let contentTree: ContentTreeWire?
    let renderContext: NoteRenderContext
    let onAvatarTap: () -> Void
    let onRowTap: () -> Void
    let onLike: () -> Void

    @State private var likeTapped = false

    var body: some View {
        Button(action: onRowTap) {
            HStack(alignment: .top, spacing: 8) {
                Button(action: onAvatarTap) {
                    ChirpAvatar(
                        url: item.authorPictureUrl,
                        initials: item.authorAvatarInitials,
                        colorHex: item.authorAvatarColor,
                        size: 40
                    )
                }
                .buttonStyle(.plain)

                VStack(alignment: .leading, spacing: 4) {
                    HStack(spacing: 4) {
                        Text(item.authorDisplay)
                            .font(.headline)
                            .foregroundStyle(.primary)
                            .lineLimit(1)
                        Spacer()
                        Text(item.createdAtDisplay)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }

                    NoteContentView(
                        content: item.renderedContent,
                        contentTree: renderContext.contentTree(for: item, fallback: contentTree),
                        renderContext: renderContext,
                        font: .body
                    )
                        .foregroundStyle(.primary)

                    // Like action row
                    HStack(spacing: 24) {
                        Button {
                            guard !likeTapped else { return }
                            likeTapped = true
                            onLike()
                            UIImpactFeedbackGenerator(style: .soft).impactOccurred()
                        } label: {
                            Image(systemName: likeTapped ? "heart.fill" : "heart")
                                .font(.caption)
                                .foregroundStyle(likeTapped ? ChirpColor.like : ChirpColor.textSecondary)
                                .scaleEffect(likeTapped ? 1.35 : 1.0)
                                .animation(.spring(response: 0.25, dampingFraction: 0.4), value: likeTapped)
                        }
                        .buttonStyle(.plain)

                        if item.relayCount > 0 {
                            HStack(spacing: 4) {
                                Image(systemName: "antenna.radiowaves.left.and.right")
                                    .font(.caption)
                                Text("\(item.relayCount)")
                                    .font(.caption)
                            }
                            .foregroundStyle(.secondary)
                        }
                    }
                    .padding(.top, 4)
                }
            }
            .padding(.vertical, 4)
            .padding(.horizontal, 16)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityIdentifier("profile-thread-link")
    }
}
