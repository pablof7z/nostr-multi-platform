import SwiftUI

/// Note row used inside ThreadScreen. Supports a "focused" state
/// (the event the thread was opened on) which gives it a hairline accent
/// leading edge and slightly more visual weight.
struct ThreadNoteRow: View {
    let item: TimelineItem
    let isFocused: Bool
    let contentTree: ContentTreeWire?
    let mentionProfiles: [String: MentionProfile]
    let eventCards: [String: ChirpEventCard]
    let timelineItems: [String: TimelineItem]
    let onAvatarTap: () -> Void
    let onLike: () -> Void
    let onReply: () -> Void

    @State private var likeTapped = false

    var body: some View {
        HStack(alignment: .top, spacing: 0) {
            // Accent hairline for focused note
            if isFocused {
                Rectangle()
                    .fill(.tint)
                    .frame(width: 2)
                    .cornerRadius(1)
                    .padding(.vertical, 4)
            }

            HStack(alignment: .top, spacing: 8) {
                Button(action: onAvatarTap) {
                    ChirpAvatar(
                        url: item.authorPictureUrl,
                        initials: item.authorAvatarInitials,
                        colorHex: item.authorAvatarColor,
                        size: isFocused ? 46 : 38
                    )
                }
                .buttonStyle(.plain)

                VStack(alignment: .leading, spacing: 4) {
                    HStack(spacing: 4) {
                        Text(item.authorDisplay)
                            .font(isFocused ? .headline : .callout)
                            .fontWeight(isFocused ? .semibold : .regular)
                            .foregroundStyle(.primary)
                            .lineLimit(1)
                        Spacer()
                        Text(item.createdAtDisplay)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }

                    NoteContentView(
                        content: item.content,
                        contentTree: contentTree,
                        mentionProfiles: mentionProfiles,
                        eventCards: eventCards,
                        timelineItems: timelineItems,
                        font: isFocused ? .body : .callout
                    )
                    .foregroundStyle(.primary)
                    .padding(.bottom, isFocused ? 4 : 0)

                    // Action row
                    HStack(spacing: 24) {
                        Button {
                            guard !likeTapped else { return }
                            likeTapped = true
                            onLike()
                            UIImpactFeedbackGenerator(style: .soft).impactOccurred()
                        } label: {
                            Image(systemName: likeTapped ? "heart.fill" : "heart")
                                .font(.caption)
                                .foregroundStyle(likeTapped ? Color.red : Color.secondary)
                                .scaleEffect(likeTapped ? 1.35 : 1.0)
                                .animation(.spring(response: 0.25, dampingFraction: 0.4), value: likeTapped)
                        }
                        .buttonStyle(.plain)

                        Button(action: onReply) {
                            Label("Reply", systemImage: "arrowshape.turn.up.left")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                                .labelStyle(.iconOnly)
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
            .padding(.vertical, isFocused ? 12 : 8)
            .padding(.horizontal, 16)
        }
        .background(isFocused ? Color.accentColor.opacity(0.06) : Color.clear)
    }
}
