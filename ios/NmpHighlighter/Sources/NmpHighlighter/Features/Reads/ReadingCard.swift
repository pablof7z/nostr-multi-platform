import Kingfisher
import SwiftUI

/// Shared presentational card for reading-style items. Used by the reads
/// feed (`ReadingFeedCardView`) and the rooms library article rows
/// (`RoomLibraryArticleCardView`). Pure layout — callers pass plain strings
/// and provide the avatar + optional trailing meta view via builder slots.
struct ReadingCard<Avatar: View, Trailing: View>: View {
    let title: String
    let summary: String
    let imageURL: URL?
    let authorName: String
    let authorPubkey: String?
    let relativeDate: String?
    let metaBits: [String]
    let showTrailing: Bool
    @ViewBuilder let avatar: () -> Avatar
    @ViewBuilder let trailing: () -> Trailing

    init(
        title: String,
        summary: String,
        imageURL: URL?,
        authorName: String,
        authorPubkey: String? = nil,
        relativeDate: String?,
        metaBits: [String],
        showTrailing: Bool,
        @ViewBuilder avatar: @escaping () -> Avatar,
        @ViewBuilder trailing: @escaping () -> Trailing
    ) {
        self.title = title
        self.summary = summary
        self.imageURL = imageURL
        self.authorName = authorName
        self.authorPubkey = authorPubkey
        self.relativeDate = relativeDate
        self.metaBits = metaBits
        self.showTrailing = showTrailing
        self.avatar = avatar
        self.trailing = trailing
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(alignment: .top, spacing: 16) {
                VStack(alignment: .leading, spacing: 6) {
                    Text(title.isEmpty ? "Untitled" : title)
                        .font(.title3.weight(.semibold))
                        .foregroundStyle(
                            title.isEmpty
                                ? Color.highlighterInkMuted
                                : Color.highlighterInkStrong
                        )
                        .lineLimit(3)
                        .fixedSize(horizontal: false, vertical: true)

                    attributionContent
                }
                .frame(maxWidth: .infinity, alignment: .leading)

                thumbnail
            }

            if !metaBits.isEmpty || showTrailing {
                metaRow
            }
        }
        .padding(.vertical, 18)
        .contentShape(Rectangle())
    }

    @ViewBuilder
    private var attributionContent: some View {
        let row = HStack(spacing: 6) {
            avatar()
            Text(authorName.uppercased())
                .font(.caption2.weight(.bold))
                .tracking(0.6)
                .foregroundStyle(Color.highlighterInkMuted)
                .lineLimit(1)
            if let relativeDate {
                Text("·")
                    .font(.caption2)
                    .foregroundStyle(Color.highlighterInkMuted)
                Text(relativeDate)
                    .font(.caption2)
                    .foregroundStyle(Color.highlighterInkMuted)
                    .lineLimit(1)
            }
            Spacer(minLength: 0)
        }
        if let authorPubkey, !authorPubkey.isEmpty {
            NavigationLink(value: ProfileDestination.pubkey(authorPubkey)) { row }
                .buttonStyle(.plain)
        } else {
            row
        }
    }

    @ViewBuilder
    private var thumbnail: some View {
        if let imageURL {
            KFImage(imageURL)
                .placeholder { thumbnailPlaceholder }
                .fade(duration: 0.15)
                .resizable()
                .scaledToFill()
                .frame(width: 96, height: 96)
                .clipShape(RoundedRectangle(cornerRadius: 6, style: .continuous))
        } else {
            thumbnailPlaceholder
                .frame(width: 96, height: 96)
                .clipShape(RoundedRectangle(cornerRadius: 6, style: .continuous))
        }
    }

    private var thumbnailPlaceholder: some View {
        LinearGradient(
            colors: [Color.highlighterRule.opacity(0.7), Color.highlighterRule.opacity(0.35)],
            startPoint: .topLeading,
            endPoint: .bottomTrailing
        )
        .overlay(
            Image(systemName: "doc.text")
                .font(.title3)
                .foregroundStyle(Color.highlighterInkMuted.opacity(0.7))
        )
    }

    private var metaRow: some View {
        HStack(spacing: 8) {
            if !metaBits.isEmpty {
                Text(metaBits.joined(separator: " · "))
                    .font(.caption)
                    .foregroundStyle(Color.highlighterInkMuted)
                    .lineLimit(1)
            }

            Spacer(minLength: 0)

            if showTrailing {
                trailing()
            }
        }
    }
}
