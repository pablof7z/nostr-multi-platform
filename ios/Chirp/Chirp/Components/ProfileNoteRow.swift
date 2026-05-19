import SwiftUI

/// Compact note row used inside ProfileView's post list.
/// Avatar tap → profile, row tap → thread.
struct ProfileNoteRow: View {
    let item: TimelineItem
    let onAvatarTap: () -> Void
    let onRowTap: () -> Void
    let onLike: () -> Void

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

                    NoteContentView(content: item.content, font: .body)
                        .foregroundStyle(.primary)

                    // Like action row
                    HStack(spacing: 24) {
                        Button(action: onLike) {
                            Label("Like", systemImage: "heart")
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
            .padding(.vertical, 4)
            .padding(.horizontal, 16)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityIdentifier("profile-thread-link")
    }
}
