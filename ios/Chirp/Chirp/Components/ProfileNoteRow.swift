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
            HStack(alignment: .top, spacing: ChirpSpace.m) {
                Button(action: onAvatarTap) {
                    ChirpAvatar(
                        url: item.authorPictureUrl,
                        initials: item.authorAvatarInitials,
                        colorHex: item.authorAvatarColor,
                        size: 40
                    )
                }
                .buttonStyle(.plain)

                VStack(alignment: .leading, spacing: ChirpSpace.xs) {
                    HStack(spacing: ChirpSpace.s) {
                        Text(item.authorDisplay)
                            .font(ChirpFont.headline)
                            .foregroundStyle(ChirpColor.textPrimary)
                            .lineLimit(1)
                        Spacer()
                        Text(item.createdAtDisplay)
                            .font(ChirpFont.caption)
                            .foregroundStyle(ChirpColor.textTertiary)
                    }

                    Text(item.content)
                        .font(ChirpFont.body)
                        .foregroundStyle(ChirpColor.textPrimary)
                        .fixedSize(horizontal: false, vertical: true)
                        .multilineTextAlignment(.leading)

                    // Like action row
                    HStack(spacing: ChirpSpace.xl) {
                        Button(action: onLike) {
                            Label("Like", systemImage: "heart")
                                .font(ChirpFont.caption)
                                .foregroundStyle(ChirpColor.textTertiary)
                                .labelStyle(.iconOnly)
                        }
                        .buttonStyle(.plain)

                        if item.relayCount > 0 {
                            HStack(spacing: ChirpSpace.xs) {
                                Image(systemName: "antenna.radiowaves.left.and.right")
                                    .font(ChirpFont.caption)
                                Text("\(item.relayCount)")
                                    .font(ChirpFont.caption)
                            }
                            .foregroundStyle(ChirpColor.textTertiary)
                        }
                    }
                    .padding(.top, ChirpSpace.xs)
                }
            }
            .padding(.vertical, ChirpSpace.s)
            .padding(.horizontal, ChirpSpace.l)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }
}
