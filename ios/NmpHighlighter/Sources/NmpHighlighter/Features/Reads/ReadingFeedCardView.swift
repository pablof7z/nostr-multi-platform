import SwiftUI

/// Editorial card in the Following Reads feed. Wraps the shared
/// `ReadingCard` presentation with a social-signal trailing slot and
/// pubkey-driven profile lookup for the author avatar.
struct ReadingFeedCardView: View {
    @Environment(HighlighterStore.self) private var app

    let item: ReadingFeedItem

    var body: some View {
        ReadingCard(
            title: item.article.title,
            summary: item.article.summary,
            imageURL: coverURL,
            authorName: authorDisplayName,
            authorPubkey: item.article.pubkey,
            relativeDate: relativeDate,
            metaBits: metaBits,
            showTrailing: hasSocialSignal,
            avatar: {
                AuthorAvatar(
                    pubkey: item.article.pubkey,
                    pictureURL: app.profileCache[item.article.pubkey]?.picture ?? "",
                    displayInitial: authorInitial,
                    size: 22
                )
            },
            trailing: { socialBadge }
        )
        .task(id: item.article.pubkey) {
            await app.requestProfile(pubkeyHex: item.article.pubkey)
        }
        .task(id: primaryInteractor ?? "") {
            guard let pk = primaryInteractor else { return }
            await app.requestProfile(pubkeyHex: pk)
        }
    }

    // MARK: - Meta bits

    private var metaBits: [String] {
        var out: [String] = []
        if let mins = readTimeMinutes { out.append("\(mins) min read") }
        if let tag = item.article.hashtags.first, !tag.isEmpty { out.append("#\(tag)") }
        return out
    }

    // MARK: - Social signal

    private var hasSocialSignal: Bool {
        !item.interactorPubkeys.isEmpty || (item.authorFollowed && item.interactorPubkeys.isEmpty)
    }

    @ViewBuilder
    private var socialBadge: some View {
        let interactors = Array(item.interactorPubkeys.prefix(3))
        HStack(spacing: 6) {
            if !interactors.isEmpty {
                HStack(spacing: -6) {
                    ForEach(interactors, id: \.self) { pk in
                        AuthorAvatar(pubkey: pk, size: 18, ringWidth: 1.5)
                    }
                }
            }
            Text(socialText)
                .font(.caption)
                .foregroundStyle(Color.highlighterInkMuted)
                .lineLimit(1)
        }
    }

    private var socialText: String {
        let interactors = item.interactorPubkeys
        let authorFollowed = item.authorFollowed

        if authorFollowed && interactors.isEmpty {
            return "From someone you follow"
        }

        switch interactors.count {
        case 0:
            return ""
        case 1:
            let name = firstInteractorName
            return authorFollowed
                ? "\(name) and the author liked this"
                : "\(name) liked this"
        case 2:
            return "\(firstInteractorName) and 1 other"
        default:
            let more = interactors.count - 1
            return "\(firstInteractorName) and \(more) others"
        }
    }

    // MARK: - Author name / initial resolution

    private var authorDisplayName: String {
        let profile = app.profileCache[item.article.pubkey]
        if let dn = profile?.displayName, !dn.isEmpty { return dn }
        if let n = profile?.name, !n.isEmpty { return n }
        return shortPubkey(item.article.pubkey)
    }

    private var authorInitial: String {
        authorDisplayName.first.map { String($0).uppercased() } ?? ""
    }

    private var firstInteractorName: String {
        guard let pk = primaryInteractor else { return "Someone" }
        let profile = app.profileCache[pk]
        if let dn = profile?.displayName, !dn.isEmpty { return dn }
        if let n = profile?.name, !n.isEmpty { return n }
        return shortPubkey(pk)
    }

    private var primaryInteractor: String? {
        item.interactorPubkeys.first
    }

    // MARK: - Derived bits

    private var coverURL: URL? {
        guard !item.article.image.isEmpty else { return nil }
        return URL(string: item.article.image)
    }

    private var relativeDate: String? {
        let seconds = item.article.publishedAt ?? item.article.createdAt ?? 0
        guard seconds > 0 else { return nil }
        let date = Date(timeIntervalSince1970: TimeInterval(seconds))
        let formatter = RelativeDateTimeFormatter()
        formatter.unitsStyle = .abbreviated
        formatter.dateTimeStyle = .numeric
        return formatter.localizedString(for: date, relativeTo: Date())
    }

    /// Rough read-time estimate: 240 wpm. Matches the reader view.
    private var readTimeMinutes: Int? {
        let words = item.article.content.split(whereSeparator: { $0.isWhitespace }).count
        guard words > 60 else { return nil }
        return max(1, words / 240)
    }

    private func shortPubkey(_ hex: String) -> String {
        String(hex.prefix(10))
    }
}
