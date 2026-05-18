import Kingfisher
import SwiftUI

/// Universal grouped-highlight module used by both the Highlights tab and
/// the Room home. The shape is uniform: a tinted rounded module that
/// joins the resource header (top) to the highlight content (below) as
/// one coherent block.
///
/// Three rendering rules, encoded by `items.count` and the number of
/// distinct highlighter pubkeys:
///   1 highlight                → resource header → byline + pull-quote
///   2+ highlights, 1 highlighter → resource header → reel of cards (no strip)
///   2+ highlights, 2+ highlighters → resource header → "Highlighted by …" → reel
///
/// The resource header adapts per artifact kind (article, web, podcast,
/// book) — the rest of the layout is shared.
struct HighlightFeedCardView: View {
    @Environment(HighlighterStore.self) private var app

    let items: [HydratedHighlight]

    /// The lead item drives the resource header and tasks. All items in
    /// the array share the same source (grouping invariant), so any of
    /// them resolves to the same artifact metadata.
    private var lead: HydratedHighlight { items[0] }

    @State private var sourceArticle: ArticleRecord?

    private var bookPreview: ArtifactPreview? {
        guard let isbn = isbnFromLead else { return nil }
        return app.isbnPreviewCache[isbn]
    }

    private var isbnFromLead: String? {
        let extRef = lead.highlight.externalReference.trimmingCharacters(in: .whitespacesAndNewlines)
        if extRef.hasPrefix("isbn:") { return String(extRef.dropFirst("isbn:".count)) }
        let addr = lead.highlight.artifactAddress.trimmingCharacters(in: .whitespacesAndNewlines)
        if addr.hasPrefix("isbn:") { return String(addr.dropFirst("isbn:".count)) }
        return nil
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            resourceHeader
            if showHighlightersStrip {
                highlightersStrip
            }
            highlightsBody
        }
        .padding(.vertical, 18)
        .task(id: lead.highlight.pubkey) {
            await app.requestProfile(pubkeyHex: lead.highlight.pubkey)
        }
        .task(id: lead.highlight.artifactAddress + lead.highlight.externalReference) {
            await resolveSource()
        }
        .task(id: webMetadataURL) {
            if let url = webMetadataURL {
                await app.requestWebMetadata(url: url)
            }
        }
    }

    // MARK: - Resource header

    private var resourceHeader: some View {
        HStack(alignment: .top, spacing: 12) {
            resourceCover
                .frame(width: 44, height: 44)
                .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))

            VStack(alignment: .leading, spacing: 3) {
                Text(resourceTitle)
                    .font(.headline.weight(.semibold))
                    .foregroundStyle(Color.highlighterInkStrong)
                    .lineLimit(2)
                    .multilineTextAlignment(.leading)
                    .fixedSize(horizontal: false, vertical: true)
                    .frame(maxWidth: .infinity, alignment: .leading)

                resourceSubtitleRow
            }
        }
    }

    private var resourceSubtitleRow: some View {
        HStack(spacing: 4) {
            let author = resourceAuthorOrDomain
            let time = resourceTimeLabel
            if !author.isEmpty {
                Text(author.uppercased())
                    .font(.caption2.weight(.bold))
                    .tracking(0.6)
                    .foregroundStyle(Color.highlighterInkMuted)
                    .lineLimit(1)
            }
            if let time, !author.isEmpty {
                Text("·")
                    .font(.caption2)
                    .foregroundStyle(Color.highlighterInkMuted)
                Text(time)
                    .font(.caption2)
                    .foregroundStyle(Color.highlighterInkMuted)
                    .lineLimit(1)
            } else if let time {
                Text(time)
                    .font(.caption2)
                    .foregroundStyle(Color.highlighterInkMuted)
                    .lineLimit(1)
            }
            Spacer(minLength: 0)
        }
    }

    @ViewBuilder
    private var resourceCover: some View {
        if let urlString = resourceCoverURL,
           !urlString.isEmpty,
           let url = URL(string: urlString) {
            Color.clear
                .overlay(
                    KFImage(url)
                        .placeholder { coverFallback }
                        .fade(duration: 0.15)
                        .resizable()
                        .scaledToFill()
                )
                .clipped()
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
                .font(.system(size: 16, weight: .semibold))
                .foregroundStyle(Color.highlighterInkStrong.opacity(0.55))
        }
    }

    // MARK: - Highlighters strip (only when 2+ unique highlighters)

    private var highlightersStrip: some View {
        HStack(spacing: 8) {
            HStack(spacing: -6) {
                ForEach(uniqueHighlighters.prefix(3), id: \.highlight.pubkey) { h in
                    AuthorAvatar(
                        pubkey: h.highlight.pubkey,
                        pictureURL: app.profileCache[h.highlight.pubkey]?.picture ?? "",
                        displayInitial: initial(for: h.highlight.pubkey),
                        size: 20
                    )
                    .overlay(
                        Circle().stroke(Color.highlighterPaperTint, lineWidth: 1.5)
                    )
                    .task(id: h.highlight.pubkey) {
                        await app.requestProfile(pubkeyHex: h.highlight.pubkey)
                    }
                }
                if uniqueHighlighters.count > 3 {
                    ZStack {
                        Circle()
                            .fill(Color.highlighterPaper)
                            .overlay(Circle().stroke(Color.highlighterRule, lineWidth: 0.5))
                        Text("+\(uniqueHighlighters.count - 3)")
                            .font(.system(size: 8, weight: .bold))
                            .foregroundStyle(Color.highlighterInkMuted)
                    }
                    .frame(width: 20, height: 20)
                    .overlay(Circle().stroke(Color.highlighterPaperTint, lineWidth: 1.5))
                }
            }

            Text(highlightersLabel)
                .font(.caption)
                .foregroundStyle(Color.highlighterInkMuted)
                .lineLimit(1)
                .truncationMode(.tail)
        }
    }

    private var highlightersLabel: AttributedString {
        let names = uniqueHighlighters.map { displayName(for: $0.highlight.pubkey) }
        var out = AttributedString("Highlighted by ")
        out.foregroundColor = Color.highlighterInkMuted

        switch names.count {
        case 0:
            return out
        case 1:
            return out + boldName(names[0])
        case 2:
            return out + boldName(names[0]) + plain(" and ") + boldName(names[1])
        default:
            // First two by name, then "+N others"
            let lead = boldName(names[0]) + plain(", ") + boldName(names[1])
            let othersCount = names.count - 2
            return out + lead + plain(" and ") + boldName("\(othersCount) others")
        }
    }

    private func boldName(_ name: String) -> AttributedString {
        var s = AttributedString(name)
        s.font = .caption.weight(.semibold)
        s.foregroundColor = Color.highlighterInkStrong
        return s
    }

    private func plain(_ str: String) -> AttributedString {
        var s = AttributedString(str)
        s.foregroundColor = Color.highlighterInkMuted
        return s
    }

    // MARK: - Highlight body (single inline OR reel)

    @ViewBuilder
    private var highlightsBody: some View {
        if items.count == 1 {
            singleHighlight(lead)
        } else {
            reel
        }
    }

    private func singleHighlight(_ h: HydratedHighlight) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            highlighterByline(for: h)

            if let pageURL = pageImageURL(for: h.highlight) {
                pageHighlight(h, pageURL: pageURL)
            } else {
                textHighlight(h)
            }
        }
    }

    /// Text-only treatment: accent rail + serif italic pull-quote + note.
    private func textHighlight(_ h: HydratedHighlight) -> some View {
        HStack(alignment: .top, spacing: 14) {
            Rectangle()
                .fill(Color.highlighterAccent)
                .frame(width: 3)
                .clipShape(RoundedRectangle(cornerRadius: 1.5))

            VStack(alignment: .leading, spacing: 8) {
                Text(h.highlight.quote.trimmingCharacters(in: .whitespacesAndNewlines))
                    .font(.system(size: 18, design: .default).italic())
                    .foregroundStyle(Color.highlighterInkStrong)
                    .lineSpacing(4)
                    .lineLimit(8)
                    .truncationMode(.tail)
                    .fixedSize(horizontal: false, vertical: true)
                    .frame(maxWidth: .infinity, alignment: .leading)

                if !h.highlight.note.isEmpty {
                    Text(h.highlight.note)
                        .font(.system(.subheadline, design: .default))
                        .foregroundStyle(Color.highlighterInkMuted)
                        .lineSpacing(2)
                        .fixedSize(horizontal: false, vertical: true)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
            }
        }
    }

    /// Page-photo treatment: the scan is the centerpiece, with the quote as
    /// a serif pull-quote underneath. No accent rail — let the image breathe.
    private func pageHighlight(_ h: HydratedHighlight, pageURL: URL) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            HighlightPageImage(url: pageURL, treatment: .feature)

            VStack(alignment: .leading, spacing: 6) {
                Text(h.highlight.quote.trimmingCharacters(in: .whitespacesAndNewlines))
                    .font(.system(size: 18, design: .default).italic())
                    .foregroundStyle(Color.highlighterInkStrong)
                    .lineSpacing(4)
                    .lineLimit(8)
                    .truncationMode(.tail)
                    .fixedSize(horizontal: false, vertical: true)
                    .frame(maxWidth: .infinity, alignment: .leading)

                if !h.highlight.note.isEmpty {
                    Text(h.highlight.note)
                        .font(.system(.subheadline, design: .default))
                        .foregroundStyle(Color.highlighterInkMuted)
                        .lineSpacing(2)
                        .fixedSize(horizontal: false, vertical: true)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
            }
            .padding(.horizontal, 4)
        }
    }

    private func pageImageURL(for highlight: HighlightRecord) -> URL? {
        let raw = highlight.imageUrl.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !raw.isEmpty else { return nil }
        return URL(string: raw)
    }

    private var reel: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(alignment: .top, spacing: 10) {
                ForEach(items, id: \.highlight.eventId) { h in
                    HighlightQuoteCard(highlight: h)
                }
                Color.clear.frame(width: 4)
            }
        }
    }

    private func highlighterByline(for h: HydratedHighlight) -> some View {
        HStack(spacing: 8) {
            AuthorAvatar(
                pubkey: h.highlight.pubkey,
                pictureURL: app.profileCache[h.highlight.pubkey]?.picture ?? "",
                displayInitial: initial(for: h.highlight.pubkey),
                size: 22
            )
            Text(displayName(for: h.highlight.pubkey))
                .font(.footnote.weight(.semibold))
                .foregroundStyle(Color.highlighterInkStrong)
                .lineLimit(1)
            if let rel = relativeDate(h.highlight.createdAt) {
                Text("·").foregroundStyle(Color.highlighterInkMuted)
                Text(rel)
                    .font(.footnote)
                    .foregroundStyle(Color.highlighterInkMuted)
                    .lineLimit(1)
            }
            Spacer(minLength: 0)
        }
        .task(id: h.highlight.pubkey) {
            await app.requestProfile(pubkeyHex: h.highlight.pubkey)
        }
    }

    // MARK: - Derived: artifact kind

    /// Canonical artifact kind for header rendering. Falls back to
    /// inspecting `artifactAddress` / `sourceUrl` when the highlight has
    /// no resolved artifact.
    private enum ArtifactKind {
        case article, web, podcast, book, video, paper, unknown
    }

    private var artifactKind: ArtifactKind {
        if let source = lead.artifact?.preview.source.lowercased(), !source.isEmpty {
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
        let extRef = lead.highlight.externalReference.trimmingCharacters(in: .whitespacesAndNewlines)
        if extRef.hasPrefix("isbn:") { return .book }
        let addr = lead.highlight.artifactAddress.trimmingCharacters(in: .whitespacesAndNewlines)
        if addr.hasPrefix("30023:") { return .article }
        if addr.hasPrefix("isbn:") { return .book }
        if !lead.highlight.sourceUrl.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            return .web
        }
        return .unknown
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

    // MARK: - Derived: resource fields

    private var resourceCoverURL: String? {
        if let img = lead.artifact?.preview.image, !img.isEmpty { return img }
        if artifactKind == .book, let img = bookPreview?.image, !img.isEmpty { return img }
        if artifactKind == .article, let img = sourceArticle?.image, !img.isEmpty { return img }
        if artifactKind == .web, let m = webMetadata {
            if !m.image.isEmpty { return m.image }
            if !m.favicon.isEmpty { return m.favicon }
        }
        return nil
    }

    private var resourceAuthorOrDomain: String {
        switch artifactKind {
        case .article:
            if let name = articleAuthorDisplayName, !name.isEmpty { return name }
            return lead.artifact?.preview.author ?? ""
        case .podcast:
            let show = lead.artifact?.preview.podcastShowTitle ?? ""
            if !show.isEmpty { return show }
            return lead.artifact?.preview.author ?? ""
        case .book:
            return lead.artifact?.preview.author ?? bookPreview?.author ?? ""
        case .web:
            if let m = webMetadata {
                if !m.siteName.isEmpty { return m.siteName }
                if !m.author.isEmpty { return m.author }
            }
            if let domain = lead.artifact?.preview.domain, !domain.isEmpty {
                return domain
            }
            return urlHost ?? ""
        case .video, .paper:
            return lead.artifact?.preview.author ?? (lead.artifact?.preview.domain ?? "")
        case .unknown:
            return urlHost ?? ""
        }
    }

    private var resourceTitle: String {
        switch artifactKind {
        case .article:
            if let t = sourceArticle?.title, !t.isEmpty { return t }
            if let t = lead.artifact?.preview.title, !t.isEmpty { return t }
            return "Untitled"
        case .podcast, .video, .paper:
            if let t = lead.artifact?.preview.title, !t.isEmpty { return t }
            return "Untitled"
        case .book:
            if let t = lead.artifact?.preview.title, !t.isEmpty { return t }
            if let t = bookPreview?.title, !t.isEmpty { return t }
            return "Untitled"
        case .web:
            if let m = webMetadata, !m.title.isEmpty { return m.title }
            if let t = lead.artifact?.preview.title, !t.isEmpty { return t }
            return urlHost ?? "Web page"
        case .unknown:
            if let t = lead.artifact?.preview.title, !t.isEmpty { return t }
            return urlHost ?? "Highlight"
        }
    }

    private var resourceTimeLabel: String? {
        switch artifactKind {
        case .article:
            guard let mins = articleReadMinutes else { return nil }
            return "\(mins) min"
        case .podcast:
            guard let secs = lead.artifact?.preview.durationSeconds, secs > 0 else { return nil }
            return formatDuration(seconds: Int(secs))
        default: return nil
        }
    }

    private var urlHost: String? {
        let raw = lead.highlight.sourceUrl.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !raw.isEmpty, let url = URL(string: raw), let host = url.host else { return nil }
        return host
    }

    /// Source URL the OG/favicon fetcher should hit. Only populated for
    /// the web kind — article/podcast/book branches own their own
    /// hydration path. Prefers the artifact's normalized URL (when a
    /// kind:11 share exists) over the raw highlight `sourceUrl` so the
    /// cache key matches what Rust would store.
    private var webMetadataURL: String? {
        guard artifactKind == .web else { return nil }
        if let u = lead.artifact?.preview.url, !u.isEmpty { return u }
        let raw = lead.highlight.sourceUrl.trimmingCharacters(in: .whitespacesAndNewlines)
        return raw.isEmpty ? nil : raw
    }

    /// Cached enrichment for the web URL (if any). Returns nil for
    /// non-web kinds. The cache key is whatever URL was passed to
    /// `requestWebMetadata` — Rust canonicalizes it, but stores the entry
    /// under the canonical key, so we reach in with the canonical URL too.
    /// In practice the artifact preview URL is already canonical (built
    /// by `normalize_artifact_url`), so this lookup is a direct hit.
    private var webMetadata: WebMetadata? {
        guard let url = webMetadataURL else { return nil }
        return app.webMetadataCache[url]
    }

    // MARK: - Derived: profile / article resolution

    /// Profile-resolved display name for a NIP-23 article author.
    /// Returns nil for non-article kinds or unresolved profiles.
    private var articleAuthorDisplayName: String? {
        guard let pubkey = articleAuthorPubkey else { return nil }
        let profile = app.profileCache[pubkey]
        if let dn = profile?.displayName, !dn.isEmpty { return dn }
        if let n = profile?.name, !n.isEmpty { return n }
        return nil
    }

    private var articleAuthorPubkey: String? {
        let addr = lead.highlight.artifactAddress.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !addr.isEmpty else { return nil }
        let parts = addr.split(separator: ":", maxSplits: 2, omittingEmptySubsequences: false)
        guard parts.count == 3, parts[0] == "30023" else { return nil }
        let pubkey = String(parts[1])
        return pubkey.isEmpty ? nil : pubkey
    }

    private var articleReadMinutes: Int? {
        guard let content = sourceArticle?.content, !content.isEmpty else { return nil }
        let words = content.split(whereSeparator: { $0.isWhitespace }).count
        guard words > 60 else { return nil }
        return max(1, words / 240)
    }

    private func formatDuration(seconds: Int) -> String {
        let h = seconds / 3600
        let m = (seconds % 3600) / 60
        if h > 0 { return "\(h)h \(m)m" }
        return "\(m)m"
    }

    // MARK: - Derived: highlighters

    private var uniqueHighlighters: [HydratedHighlight] {
        var seen = Set<String>()
        var out: [HydratedHighlight] = []
        for h in items {
            if seen.insert(h.highlight.pubkey).inserted {
                out.append(h)
            }
        }
        return out
    }

    private var showHighlightersStrip: Bool {
        items.count >= 2 && uniqueHighlighters.count >= 2
    }

    // MARK: - Derived: profile helpers

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
        case ..<3600: return "\(Int(delta / 60))m"
        case ..<86400: return "\(Int(delta / 3600))h"
        case ..<(86400 * 7): return "\(Int(delta / 86400))d"
        case ..<(86400 * 30): return "\(Int(delta / (86400 * 7)))w"
        default: return "\(Int(delta / (86400 * 30)))mo"
        }
    }

    private func resolveSource() async {
        sourceArticle = nil

        if let isbn = isbnFromLead {
            await app.requestIsbnPreview(isbn: isbn)
            return
        }

        let addr = lead.highlight.artifactAddress.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !addr.isEmpty else { return }

        let parts = addr.split(separator: ":", maxSplits: 2, omittingEmptySubsequences: false)
        guard parts.count == 3, parts[0] == "30023" else { return }
        let pubkey = String(parts[1])
        let dTag = String(parts[2])
        guard !pubkey.isEmpty, !dTag.isEmpty else { return }

        sourceArticle = try? await app.safeCore.getArticle(pubkeyHex: pubkey, dTag: dTag)
        await app.requestProfile(pubkeyHex: pubkey)
    }
}

// MARK: - Single quote card (used inside the reel)

/// One quote inside the horizontal reel of a multi-highlight module.
/// Shows the highlighter byline at the top, the quote with the accent
/// rail below, and the optional note. Width is fixed so the reel paces
/// consistently.
private struct HighlightQuoteCard: View {
    @Environment(HighlighterStore.self) private var app

    let highlight: HydratedHighlight

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            byline
                .padding(12)
                .padding(.bottom, 10)
                .overlay(alignment: .bottom) {
                    Rectangle()
                        .fill(Color.highlighterRule.opacity(0.5))
                        .frame(height: 1)
                        .padding(.horizontal, 12)
                }

            if let pageURL = pageImageURL {
                VStack(alignment: .leading, spacing: 8) {
                    HighlightPageImage(url: pageURL, treatment: .card)
                    quoteBlock
                }
                .padding(12)
            } else {
                HStack(alignment: .top, spacing: 10) {
                    Rectangle()
                        .fill(Color.highlighterAccent)
                        .frame(width: 3)
                        .clipShape(RoundedRectangle(cornerRadius: 1.5))

                    quoteBlock
                }
                .padding(12)
            }
        }
        .frame(width: 240, alignment: .topLeading)
        .background(
            RoundedRectangle(cornerRadius: 12, style: .continuous)
                .fill(Color.highlighterPaper)
        )
        .task(id: highlight.highlight.pubkey) {
            await app.requestProfile(pubkeyHex: highlight.highlight.pubkey)
        }
    }

    private var quoteBlock: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(highlight.highlight.quote.trimmingCharacters(in: .whitespacesAndNewlines))
                .font(.system(size: 14, design: .default).italic())
                .foregroundStyle(Color.highlighterInkStrong)
                .lineSpacing(3)
                .lineLimit(6)
                .truncationMode(.tail)
                .fixedSize(horizontal: false, vertical: true)
                .frame(maxWidth: .infinity, alignment: .leading)

            if !highlight.highlight.note.isEmpty {
                Text(highlight.highlight.note)
                    .font(.caption)
                    .foregroundStyle(Color.highlighterInkMuted)
                    .lineLimit(2)
                    .fixedSize(horizontal: false, vertical: true)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
    }

    private var pageImageURL: URL? {
        let raw = highlight.highlight.imageUrl.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !raw.isEmpty else { return nil }
        return URL(string: raw)
    }

    private var byline: some View {
        HStack(spacing: 7) {
            AuthorAvatar(
                pubkey: highlight.highlight.pubkey,
                pictureURL: app.profileCache[highlight.highlight.pubkey]?.picture ?? "",
                displayInitial: initial,
                size: 22
            )
            Text(name)
                .font(.caption.weight(.semibold))
                .foregroundStyle(Color.highlighterInkStrong)
                .lineLimit(1)
            if let rel = relative {
                Text("·")
                    .font(.caption2)
                    .foregroundStyle(Color.highlighterInkMuted)
                Text(rel)
                    .font(.caption2)
                    .foregroundStyle(Color.highlighterInkMuted)
                    .lineLimit(1)
            }
            Spacer(minLength: 0)
        }
    }

    private var name: String {
        let profile = app.profileCache[highlight.highlight.pubkey]
        if let dn = profile?.displayName, !dn.isEmpty { return dn }
        if let n = profile?.name, !n.isEmpty { return n }
        return String(highlight.highlight.pubkey.prefix(10))
    }

    private var initial: String {
        name.first.map { String($0).uppercased() } ?? "?"
    }

    private var relative: String? {
        guard let s = highlight.highlight.createdAt, s > 0 else { return nil }
        let now = Date().timeIntervalSince1970
        let delta = now - TimeInterval(s)
        guard delta >= 0 else { return nil }
        switch delta {
        case ..<60: return "just now"
        case ..<3600: return "\(Int(delta / 60))m"
        case ..<86400: return "\(Int(delta / 3600))h"
        case ..<(86400 * 7): return "\(Int(delta / 86400))d"
        case ..<(86400 * 30): return "\(Int(delta / (86400 * 7)))w"
        default: return "\(Int(delta / (86400 * 30)))mo"
        }
    }
}
