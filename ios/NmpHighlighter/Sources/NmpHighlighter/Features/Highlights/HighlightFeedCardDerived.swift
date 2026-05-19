import Kingfisher
import SwiftUI

extension HighlightFeedCardView {
    // MARK: - Derived: artifact kind

    /// Canonical artifact kind for header rendering. Falls back to
    /// inspecting `artifactAddress` / `sourceUrl` when the highlight has
    /// no resolved artifact.
    enum ArtifactKind {
        case article, web, podcast, book, video, paper, unknown
    }

    var artifactKind: ArtifactKind {
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

    var kindIconName: String {
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

    var resourceCoverURL: String? {
        if let img = lead.artifact?.preview.image, !img.isEmpty { return img }
        if artifactKind == .book, let img = bookPreview?.image, !img.isEmpty { return img }
        if artifactKind == .article, let img = sourceArticle?.image, !img.isEmpty { return img }
        if artifactKind == .web, let m = webMetadata {
            if !m.image.isEmpty { return m.image }
            if !m.favicon.isEmpty { return m.favicon }
        }
        return nil
    }

    var resourceAuthorOrDomain: String {
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

    var resourceTitle: String {
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

    var resourceTimeLabel: String? {
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

    var urlHost: String? {
        let raw = lead.highlight.sourceUrl.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !raw.isEmpty, let url = URL(string: raw), let host = url.host else { return nil }
        return host
    }

    /// Source URL the OG/favicon fetcher should hit. Only populated for
    /// the web kind — article/podcast/book branches own their own
    /// hydration path. Prefers the artifact's normalized URL (when a
    /// kind:11 share exists) over the raw highlight `sourceUrl` so the
    /// cache key matches what Rust would store.
    var webMetadataURL: String? {
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
    var webMetadata: WebMetadata? {
        guard let url = webMetadataURL else { return nil }
        return app.webMetadataCache[url]
    }

    // MARK: - Derived: profile / article resolution

    /// Profile-resolved display name for a NIP-23 article author.
    /// Returns nil for non-article kinds or unresolved profiles.
    var articleAuthorDisplayName: String? {
        guard let pubkey = articleAuthorPubkey else { return nil }
        let profile = app.profileCache[pubkey]
        if let dn = profile?.displayName, !dn.isEmpty { return dn }
        if let n = profile?.name, !n.isEmpty { return n }
        return nil
    }

    var articleAuthorPubkey: String? {
        let addr = lead.highlight.artifactAddress.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !addr.isEmpty else { return nil }
        let parts = addr.split(separator: ":", maxSplits: 2, omittingEmptySubsequences: false)
        guard parts.count == 3, parts[0] == "30023" else { return nil }
        let pubkey = String(parts[1])
        return pubkey.isEmpty ? nil : pubkey
    }

    var articleReadMinutes: Int? {
        guard let content = sourceArticle?.content, !content.isEmpty else { return nil }
        let words = content.split(whereSeparator: { $0.isWhitespace }).count
        guard words > 60 else { return nil }
        return max(1, words / 240)
    }

    func formatDuration(seconds: Int) -> String {
        let h = seconds / 3600
        let m = (seconds % 3600) / 60
        if h > 0 { return "\(h)h \(m)m" }
        return "\(m)m"
    }

    // MARK: - Derived: highlighters

    var uniqueHighlighters: [HydratedHighlight] {
        var seen = Set<String>()
        var out: [HydratedHighlight] = []
        for h in items {
            if seen.insert(h.highlight.pubkey).inserted {
                out.append(h)
            }
        }
        return out
    }

    var showHighlightersStrip: Bool {
        items.count >= 2 && uniqueHighlighters.count >= 2
    }

    // MARK: - Derived: profile helpers

    func displayName(for pubkey: String) -> String {
        let profile = app.profileCache[pubkey]
        if let dn = profile?.displayName, !dn.isEmpty { return dn }
        if let n = profile?.name, !n.isEmpty { return n }
        return String(pubkey.prefix(10))
    }

    func initial(for pubkey: String) -> String {
        displayName(for: pubkey).first.map { String($0).uppercased() } ?? "?"
    }

    func relativeDate(_ seconds: UInt64?) -> String? {
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

    func resolveSource() async {
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
}

