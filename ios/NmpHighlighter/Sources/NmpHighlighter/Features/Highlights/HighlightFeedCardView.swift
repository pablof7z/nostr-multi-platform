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
    @Environment(HighlighterStore.self) var app

    let items: [HydratedHighlight]

    /// The lead item drives the resource header and tasks. All items in
    /// the array share the same source (grouping invariant), so any of
    /// them resolves to the same artifact metadata.
    var lead: HydratedHighlight { items[0] }

    @State var sourceArticle: ArticleRecord?

    var bookPreview: ArtifactPreview? {
        guard let isbn = isbnFromLead else { return nil }
        return app.isbnPreviewCache[isbn]
    }

    var isbnFromLead: String? {
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

}
