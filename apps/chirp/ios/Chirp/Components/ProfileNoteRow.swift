import SwiftUI

/// Compact note row used inside ProfileView's post list.
/// Avatar tap → profile, row tap → thread.
///
/// `renderContext` carries the per-note mention map and embedded event-card
/// lookup `NoteContentView` consumes. ProfileView builds it once at the body
/// root from the dynamic `nmp.feed.author.<pubkey>` flat-feed projection.
struct ProfileNoteRow: View {
    let card: ChirpEventCard
    let renderContext: NoteRenderContext
    let onAvatarTap: () -> Void
    let onRowTap: () -> Void
    let onLike: () -> Void
    var onRepost: (() -> Void)? = nil

    @EnvironmentObject private var model: KernelModel
    @State private var likeTapped = false

    private var authorDisplayLabel: String {
        model.profile(forPubkey: card.authorPubkey)?.display
            ?? card.authorDisplayName
            ?? card.authorPubkey.shortHex
    }

    private var displayContent: String {
        card.contentPreview.isEmpty ? card.content : card.contentPreview
    }

    var body: some View {
        Button(action: onRowTap) {
            HStack(alignment: .top, spacing: 8) {
                Button(action: onAvatarTap) {
                    NostrAvatar(
                        pubkey: card.authorPubkey,
                        url: card.authorPictureUrl,
                        size: 40
                    )
                    .equatable()
                }
                .buttonStyle(.plain)

                VStack(alignment: .leading, spacing: 4) {
                    HStack(spacing: 4) {
                        Text(authorDisplayLabel)
                            .font(.headline)
                            .foregroundStyle(.primary)
                            .lineLimit(1)
                        Spacer()
                        Text(card.createdAt.relativeTimeFromUnixSeconds)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }

                    NoteContentView(
                        content: displayContent,
                        contentTree: card.contentTree,
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

                        if let onRepost {
                            Button {
                                onRepost()
                                UIImpactFeedbackGenerator(style: .light).impactOccurred()
                            } label: {
                                Label("Repost", systemImage: "arrow.2.squarepath")
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                                    .labelStyle(.iconOnly)
                            }
                            .buttonStyle(.plain)
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
