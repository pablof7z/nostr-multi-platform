import Kingfisher
import SwiftUI

struct RoomLibraryBookCardView: View {
    @Environment(HighlighterStore.self) private var app

    let artifact: ArtifactRecord
    var commentCount: Int = 0

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(alignment: .top, spacing: 16) {
                VStack(alignment: .leading, spacing: 6) {
                    Text(artifact.preview.title.isEmpty ? "Untitled" : artifact.preview.title)
                        .font(.title3.weight(.semibold))
                        .foregroundStyle(
                            artifact.preview.title.isEmpty
                                ? Color.highlighterInkMuted
                                : Color.highlighterInkStrong
                        )
                        .lineLimit(3)
                        .fixedSize(horizontal: false, vertical: true)

                    if !artifact.preview.author.isEmpty {
                        Text(artifact.preview.author.uppercased())
                            .font(.caption2.weight(.bold))
                            .tracking(0.6)
                            .foregroundStyle(Color.highlighterInkMuted)
                            .lineLimit(1)
                    }

                    if !artifact.preview.description.isEmpty {
                        Text(artifact.preview.description)
                            .font(.subheadline)
                            .foregroundStyle(Color.highlighterInkMuted)
                            .lineLimit(2)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)

                bookCover
            }

            sharerRow
        }
        .padding(.vertical, 18)
        .contentShape(Rectangle())
        .task(id: artifact.pubkey) {
            await app.requestProfile(pubkeyHex: artifact.pubkey)
        }
    }

    private var sharerRow: some View {
        HStack(spacing: 6) {
            AuthorAvatar(
                pubkey: artifact.pubkey,
                pictureURL: app.profileCache[artifact.pubkey]?.picture ?? "",
                displayInitial: sharerInitial,
                size: 18
            )

            Text(sharerName.uppercased())
                .font(.caption2.weight(.bold))
                .tracking(0.6)
                .foregroundStyle(Color.highlighterInkMuted)
                .lineLimit(1)

            if let date = relativeDate {
                Text("·")
                    .font(.caption2)
                    .foregroundStyle(Color.highlighterInkMuted)
                Text(date)
                    .font(.caption2)
                    .foregroundStyle(Color.highlighterInkMuted)
                    .lineLimit(1)
            }

            Spacer(minLength: 0)

            if commentCount > 0 {
                HStack(spacing: 3) {
                    Image(systemName: "bubble.left")
                        .font(.caption2)
                    Text("\(commentCount)")
                        .font(.caption2.weight(.semibold))
                }
                .foregroundStyle(Color.highlighterInkMuted)
            }
        }
    }

    @ViewBuilder
    private var bookCover: some View {
        let image = artifact.preview.image
        Group {
            if !image.isEmpty, let url = URL(string: image) {
                KFImage(url)
                    .placeholder { bookPlaceholder }
                    .fade(duration: 0.15)
                    .resizable()
                    .scaledToFill()
            } else {
                bookPlaceholder
            }
        }
        .frame(width: 64, height: 96)
        .clipShape(RoundedRectangle(cornerRadius: 4, style: .continuous))
        .shadow(color: .black.opacity(0.12), radius: 4, x: 0, y: 2)
    }

    private var bookPlaceholder: some View {
        LinearGradient(
            colors: [Color.highlighterRule.opacity(0.7), Color.highlighterRule.opacity(0.35)],
            startPoint: .topLeading,
            endPoint: .bottomTrailing
        )
        .overlay(
            Image(systemName: "book.closed")
                .font(.title3)
                .foregroundStyle(Color.highlighterInkMuted.opacity(0.7))
        )
    }

    private var sharerName: String {
        let profile = app.profileCache[artifact.pubkey]
        if let dn = profile?.displayName, !dn.isEmpty { return dn }
        if let n = profile?.name, !n.isEmpty { return n }
        return String(artifact.pubkey.prefix(10))
    }

    private var sharerInitial: String {
        sharerName.first.map { String($0).uppercased() } ?? ""
    }

    private var relativeDate: String? {
        guard let seconds = artifact.createdAt, seconds > 0 else { return nil }
        let date = Date(timeIntervalSince1970: TimeInterval(seconds))
        let formatter = RelativeDateTimeFormatter()
        formatter.unitsStyle = .abbreviated
        formatter.dateTimeStyle = .numeric
        return formatter.localizedString(for: date, relativeTo: Date())
    }
}
