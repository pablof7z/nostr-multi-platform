import SwiftUI

/// Medium-style card for an article artifact in a room's library. Mirrors
/// the reads-tab treatment by parsing the artifact's NIP-23 `a`-tag
/// (`30023:<pubkey>:<d>`) so the real article author drives the attribution
/// avatar/profile lookup rather than the sharer.
struct RoomLibraryArticleCardView: View {
    @Environment(HighlighterStore.self) private var app

    let artifact: ArtifactRecord
    var commentCount: Int = 0

    var body: some View {
        ReadingCard(
            title: artifact.preview.title,
            summary: artifact.preview.description,
            imageURL: coverURL,
            authorName: authorDisplayName,
            authorPubkey: articleAuthorPubkey,
            relativeDate: relativeDate,
            metaBits: metaBits,
            showTrailing: false,
            avatar: {
                let pubkey = articleAuthorPubkey ?? artifact.pubkey
                AuthorAvatar(
                    pubkey: pubkey,
                    pictureURL: app.profileCache[pubkey]?.picture ?? "",
                    displayInitial: authorInitial,
                    size: 22
                )
            },
            trailing: { EmptyView() }
        )
        .task(id: articleAuthorPubkey ?? "") {
            guard let pk = articleAuthorPubkey else { return }
            await app.requestProfile(pubkeyHex: pk)
        }
    }

    // MARK: - Derived bits

    private var coverURL: URL? {
        guard !artifact.preview.image.isEmpty else { return nil }
        return URL(string: artifact.preview.image)
    }

    /// Parse `30023:<pubkey>:<d>` out of the artifact's highlight or
    /// primary reference tag.
    private var articleAuthorPubkey: String? {
        let raw: String
        if artifact.preview.highlightTagName == "a", !artifact.preview.highlightTagValue.isEmpty {
            raw = artifact.preview.highlightTagValue
        } else if artifact.preview.referenceTagName == "a", !artifact.preview.referenceTagValue.isEmpty {
            raw = artifact.preview.referenceTagValue
        } else {
            return nil
        }
        let parts = raw.split(separator: ":", maxSplits: 2, omittingEmptySubsequences: false)
        guard parts.count == 3, parts[0] == "30023" else { return nil }
        let pubkey = String(parts[1])
        return pubkey.isEmpty ? nil : pubkey
    }

    private var authorDisplayName: String {
        let profile = articleAuthorPubkey.flatMap { app.profileCache[$0] }
        if let dn = profile?.displayName, !dn.isEmpty { return dn }
        if let n = profile?.name, !n.isEmpty { return n }
        if !artifact.preview.author.isEmpty { return artifact.preview.author }
        if let pk = articleAuthorPubkey { return String(pk.prefix(10)) }
        return "Unknown"
    }

    private var authorInitial: String {
        authorDisplayName.first.map { String($0).uppercased() } ?? ""
    }

    private var relativeDate: String? {
        guard let seconds = artifact.createdAt, seconds > 0 else { return nil }
        let date = Date(timeIntervalSince1970: TimeInterval(seconds))
        let formatter = RelativeDateTimeFormatter()
        formatter.unitsStyle = .abbreviated
        formatter.dateTimeStyle = .numeric
        return formatter.localizedString(for: date, relativeTo: Date())
    }

    private var metaBits: [String] {
        var out: [String] = []
        if !artifact.preview.domain.isEmpty { out.append(artifact.preview.domain) }
        if commentCount > 0 {
            out.append("\(commentCount) comment\(commentCount == 1 ? "" : "s")")
        }
        return out
    }
}
