import Kingfisher
import SwiftUI

/// Navigation payload for the highlight-centric detail view. Wraps the
/// hydrated highlight so the destination has full context without a
/// round-trip; Hashable so the parent NavigationStack can dispatch via
/// `.navigationDestination(for:)`.
struct HighlightDetailTarget: Hashable {
    let item: HydratedHighlight
}

/// Detail screen that puts a single highlight at the centerpiece. The
/// underlying artifact (article / book / web / podcast) is reduced to a
/// compact, tappable header at the top — a one-tap escape hatch into the
/// reader views the feed previously navigated to directly. The action
/// bar surfaces:
///   - bookmark (source article only; existing kind:10003/30004 flow)
///   - comments (NIP-22, scoped to the kind:9802 highlight event)
///   - share (system share sheet — `https://beta.highlighter.com/highlight/<nevent>`
///     URL that the SvelteKit web app server-renders into a social card)
///   - add to room (kind:16 repost into one of the user's NIP-29 rooms)
struct HighlightDetailView: View {
    @Environment(HighlighterStore.self) private var app

    let item: HydratedHighlight

    @State private var commentsStore = CommentsStore()
    @State private var commentsStarted = false
    @State private var showComments = false
    @State private var shareTarget: ShareToCommunityTarget?
    @State private var shareURL: URL?

    private var highlight: HighlightRecord { item.highlight }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 22) {
                resourceHeader
                bylineRow
                quoteBlock
                if !highlight.note.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                    noteBlock
                }
                actionBar
                    .padding(.top, 4)
            }
            .padding(.horizontal, 20)
            .padding(.vertical, 24)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .background(Color.highlighterPaper.ignoresSafeArea())
        .navigationTitle("Highlight")
        .navigationBarTitleDisplayMode(.inline)
        .navigationDestination(isPresented: $showComments) {
            CommentsView(
                artifact: commentsArtifactRef,
                artifactAuthorPubkey: highlight.pubkey,
                artifactHeader: nil,
                store: commentsStore
            )
        }
        .sheet(item: $shareTarget) { target in
            ShareToCommunitySheet(target: target)
                .presentationDetents([.medium, .large])
        }
        .task {
            guard !commentsStarted else { return }
            commentsStarted = true
            await commentsStore.start(
                artifact: commentsArtifactRef,
                core: app.safeCore,
                currentUserPubkey: app.currentUser?.pubkey
            )
        }
        .task(id: highlight.eventId) {
            await refreshShareURL()
        }
        .task(id: highlight.pubkey) {
            await app.requestProfile(pubkeyHex: highlight.pubkey)
        }
    }

    // MARK: - Resource header (tappable → opens artifact view)

    @ViewBuilder
    private var resourceHeader: some View {
        if let t = articleReaderTarget {
            NavigationLink(value: t) { resourceHeaderCard(showsChevron: true) }
                .buttonStyle(.plain)
        } else if let t = bookReaderTarget {
            NavigationLink(value: t) { resourceHeaderCard(showsChevron: true) }
                .buttonStyle(.plain)
        } else if let t = webReaderTarget {
            NavigationLink(value: t) { resourceHeaderCard(showsChevron: true) }
                .buttonStyle(.plain)
        } else {
            resourceHeaderCard(showsChevron: false)
        }
    }

    private func resourceHeaderCard(showsChevron: Bool) -> some View {
        HStack(alignment: .center, spacing: 12) {
            resourceCover
                .frame(width: 40, height: 40)
                .clipShape(RoundedRectangle(cornerRadius: 6, style: .continuous))

            VStack(alignment: .leading, spacing: 2) {
                Text(resourceKindLabel.uppercased())
                    .font(.caption2.weight(.bold))
                    .tracking(0.6)
                    .foregroundStyle(Color.highlighterInkMuted)
                Text(resourceTitle)
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(Color.highlighterInkStrong)
                    .lineLimit(2)
                    .multilineTextAlignment(.leading)
                if !resourceAuthor.isEmpty {
                    Text(resourceAuthor)
                        .font(.caption)
                        .foregroundStyle(Color.highlighterInkMuted)
                        .lineLimit(1)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)

            if showsChevron {
                Image(systemName: "chevron.right")
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(Color.highlighterInkMuted)
            }
        }
        .padding(12)
        .background(
            RoundedRectangle(cornerRadius: 12, style: .continuous)
                .fill(Color.highlighterPaperTint)
        )
    }

    @ViewBuilder
    private var resourceCover: some View {
        if let urlString = item.artifact?.preview.image,
           !urlString.isEmpty,
           let url = URL(string: urlString) {
            KFImage(url)
                .placeholder { coverFallback }
                .fade(duration: 0.15)
                .resizable()
                .scaledToFill()
        } else {
            coverFallback
        }
    }

    private var coverFallback: some View {
        ZStack {
            LinearGradient(
                colors: [
                    Color.highlighterAccent.opacity(0.30),
                    Color.highlighterAccent.opacity(0.12),
                ],
                startPoint: .topLeading,
                endPoint: .bottomTrailing
            )
            Image(systemName: kindIconName)
                .font(.system(size: 14, weight: .semibold))
                .foregroundStyle(Color.highlighterInkStrong.opacity(0.55))
        }
    }

    // MARK: - Highlighter byline

    private var bylineRow: some View {
        NavigationLink(value: ProfileDestination.pubkey(highlight.pubkey)) {
            HStack(spacing: 10) {
                AuthorAvatar(
                    pubkey: highlight.pubkey,
                    pictureURL: app.profileCache[highlight.pubkey]?.picture ?? "",
                    displayInitial: initial(for: highlight.pubkey),
                    size: 32
                )
                VStack(alignment: .leading, spacing: 2) {
                    Text(displayName(for: highlight.pubkey))
                        .font(.subheadline.weight(.semibold))
                        .foregroundStyle(Color.highlighterInkStrong)
                        .lineLimit(1)
                    if let rel = relativeDate(highlight.createdAt) {
                        Text(rel)
                            .font(.caption)
                            .foregroundStyle(Color.highlighterInkMuted)
                    }
                }
                Spacer(minLength: 0)
            }
        }
        .buttonStyle(.plain)
    }

    // MARK: - Quote

    @ViewBuilder
    private var quoteBlock: some View {
        if let pageURL = pageImageURL {
            VStack(alignment: .leading, spacing: 14) {
                HighlightPageImage(url: pageURL, treatment: .feature)
                quoteText
            }
        } else {
            HStack(alignment: .top, spacing: 14) {
                Rectangle()
                    .fill(Color.highlighterAccent)
                    .frame(width: 3)
                    .clipShape(RoundedRectangle(cornerRadius: 1.5))
                quoteText
            }
        }
    }

    private var quoteText: some View {
        Text(highlight.quote.trimmingCharacters(in: .whitespacesAndNewlines))
            .font(.system(size: 21, design: .default).italic())
            .foregroundStyle(Color.highlighterInkStrong)
            .lineSpacing(5)
            .fixedSize(horizontal: false, vertical: true)
            .frame(maxWidth: .infinity, alignment: .leading)
            .textSelection(.enabled)
    }

    private var noteBlock: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("NOTE")
                .font(.caption2.weight(.bold))
                .tracking(0.6)
                .foregroundStyle(Color.highlighterInkMuted)
            Text(highlight.note)
                .font(.system(.body, design: .default))
                .foregroundStyle(Color.highlighterInkStrong)
                .lineSpacing(3)
                .fixedSize(horizontal: false, vertical: true)
                .frame(maxWidth: .infinity, alignment: .leading)
                .textSelection(.enabled)
        }
        .padding(.leading, 17)
    }

    // MARK: - Action bar

    private var actionBar: some View {
        HStack(spacing: 22) {
            if let articleAddress = articleAddressForBookmark {
                BookmarkMenuButton(articleAddress: articleAddress)
                    .font(.system(size: 20, weight: .medium))
            }

            commentsButton

            if let url = shareURL {
                ShareLink(
                    item: url,
                    subject: Text("Highlight"),
                    message: Text(highlight.quote.trimmingCharacters(in: .whitespacesAndNewlines))
                ) {
                    actionIcon(systemName: "square.and.arrow.up")
                }
                .accessibilityLabel("Share highlight")
            }

            Button {
                shareTarget = .highlight(highlight)
            } label: {
                actionIcon(systemName: "rectangle.stack.badge.plus")
            }
            .disabled(app.joinedCommunities.isEmpty)
            .accessibilityLabel("Add to room")

            Spacer(minLength: 0)
        }
        .padding(.vertical, 12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .overlay(alignment: .top) {
            Rectangle()
                .fill(Color.highlighterRule.opacity(0.5))
                .frame(height: 1)
        }
    }

    private var commentsButton: some View {
        Button {
            showComments = true
        } label: {
            HStack(spacing: 5) {
                Image(systemName: "bubble.left")
                    .font(.system(size: 20, weight: .medium))
                if commentsStore.totalCount > 0 {
                    Text("\(commentsStore.totalCount)")
                        .font(.system(size: 14, weight: .semibold, design: .rounded))
                        .monospacedDigit()
                }
            }
            .foregroundStyle(Color.highlighterInkStrong)
        }
        .accessibilityLabel(
            commentsStore.totalCount == 0
                ? "Start the thread"
                : "\(commentsStore.totalCount) comments"
        )
    }

    private func actionIcon(systemName: String) -> some View {
        Image(systemName: systemName)
            .font(.system(size: 20, weight: .medium))
            .foregroundStyle(Color.highlighterInkStrong)
    }

    // MARK: - Comments scope

    private var commentsArtifactRef: ArtifactRef {
        .event(id: highlight.eventId, kind: 9802)
    }

    /// Public web URL that the share sheet hands to other apps. The
    /// route at `/highlight/<nevent>` on `beta.highlighter.com` is
    /// server-rendered with full Open Graph + Twitter Card meta so the
    /// link unfurls into a social card built around the quote.
    private func refreshShareURL() async {
        guard
            let nevent = try? await app.safeCore.encodeNevent(
                eventIdHex: highlight.eventId,
                authorPubkeyHex: highlight.pubkey,
                relayHints: ["wss://relay.highlighter.com"],
                kind: 9802
            )
        else {
            shareURL = nil
            return
        }
        shareURL = URL(string: "https://beta.highlighter.com/highlight/\(nevent)")
    }

    // MARK: - Reader-target dispatch (mirror of HighlightsTabView)

    /// Article a-tag we can bookmark. Only NIP-23 articles are
    /// bookmarkable today (the existing curation-set machinery is
    /// addressable-only).
    private var articleAddressForBookmark: String? {
        let addr = highlight.artifactAddress.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !addr.isEmpty else { return nil }
        let parts = addr.split(separator: ":", maxSplits: 2, omittingEmptySubsequences: false)
        guard parts.count == 3, parts[0] == "30023" else { return nil }
        return addr
    }

    private var articleReaderTarget: ArticleReaderTarget? {
        let addr = highlight.artifactAddress.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !addr.isEmpty else { return nil }
        let parts = addr.split(separator: ":", maxSplits: 2, omittingEmptySubsequences: false)
        guard parts.count == 3, parts[0] == "30023" else { return nil }
        let pubkey = String(parts[1])
        let dTag = String(parts[2])
        guard !pubkey.isEmpty, !dTag.isEmpty else { return nil }
        return ArticleReaderTarget(pubkey: pubkey, dTag: dTag, seed: nil)
    }

    private var bookReaderTarget: BookTarget? {
        let extRef = highlight.externalReference.trimmingCharacters(in: .whitespacesAndNewlines)
        if extRef.hasPrefix("isbn:") { return BookTarget(catalogId: extRef) }
        let addr = highlight.artifactAddress.trimmingCharacters(in: .whitespacesAndNewlines)
        if addr.hasPrefix("isbn:") { return BookTarget(catalogId: addr) }
        return nil
    }

    private var webReaderTarget: WebReaderTarget? {
        let raw = highlight.sourceUrl.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !raw.isEmpty, let url = URL(string: raw) else { return nil }
        guard let scheme = url.scheme?.lowercased(),
              scheme == "http" || scheme == "https" else { return nil }
        return WebReaderTarget(url: url, highlightQuote: highlight.quote)
    }

    // MARK: - Resource metadata

    private var pageImageURL: URL? {
        let raw = highlight.imageUrl.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !raw.isEmpty else { return nil }
        return URL(string: raw)
    }

    private var resourceTitle: String {
        if let t = item.artifact?.preview.title, !t.isEmpty { return t }
        if let host = sourceURLHost { return host }
        return "Untitled"
    }

    private var resourceAuthor: String {
        if let a = item.artifact?.preview.author, !a.isEmpty { return a }
        if let d = item.artifact?.preview.domain, !d.isEmpty { return d }
        return sourceURLHost ?? ""
    }

    private var resourceKindLabel: String {
        switch artifactKind {
        case .article: return "Article"
        case .book:    return "Book"
        case .podcast: return "Podcast"
        case .web:     return "Web"
        case .video:   return "Video"
        case .paper:   return "Paper"
        case .unknown: return "Source"
        }
    }

    private var kindIconName: String {
        switch artifactKind {
        case .article: return "doc.text"
        case .web:     return "globe"
        case .podcast: return "waveform"
        case .book:    return "book.closed"
        case .video:   return "play.rectangle"
        case .paper:   return "doc.richtext"
        case .unknown: return "quote.bubble"
        }
    }

    private enum ArtifactKind {
        case article, web, podcast, book, video, paper, unknown
    }

    private var artifactKind: ArtifactKind {
        if let source = item.artifact?.preview.source.lowercased(), !source.isEmpty {
            switch source {
            case "article": return .article
            case "web":     return .web
            case "podcast": return .podcast
            case "book":    return .book
            case "video":   return .video
            case "paper":   return .paper
            default:        return .unknown
            }
        }
        let extRef = highlight.externalReference.trimmingCharacters(in: .whitespacesAndNewlines)
        if extRef.hasPrefix("isbn:") { return .book }
        let addr = highlight.artifactAddress.trimmingCharacters(in: .whitespacesAndNewlines)
        if addr.hasPrefix("30023:") { return .article }
        if addr.hasPrefix("isbn:") { return .book }
        if !highlight.sourceUrl.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            return .web
        }
        return .unknown
    }

    private var sourceURLHost: String? {
        let raw = highlight.sourceUrl.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !raw.isEmpty, let url = URL(string: raw), let host = url.host else { return nil }
        return host
    }

    // MARK: - Profile helpers

    private func displayName(for pubkey: String) -> String {
        let profile = app.profileCache[pubkey]
        if let dn = profile?.displayName, !dn.isEmpty { return dn }
        if let n = profile?.name, !n.isEmpty { return n }
        return String(pubkey.prefix(10))
    }

    private func initial(for pubkey: String) -> String {
        displayName(for: pubkey).first.map { String($0).uppercased() } ?? "?"
    }

    private func relativeDate(_ seconds: UInt64?) -> String? {
        guard let s = seconds, s > 0 else { return nil }
        let now = Date().timeIntervalSince1970
        let delta = now - TimeInterval(s)
        guard delta >= 0 else { return nil }
        switch delta {
        case ..<60: return "just now"
        case ..<3600: return "\(Int(delta / 60))m ago"
        case ..<86400: return "\(Int(delta / 3600))h ago"
        case ..<(86400 * 7): return "\(Int(delta / 86400))d ago"
        case ..<(86400 * 30): return "\(Int(delta / (86400 * 7)))w ago"
        default: return "\(Int(delta / (86400 * 30)))mo ago"
        }
    }
}
