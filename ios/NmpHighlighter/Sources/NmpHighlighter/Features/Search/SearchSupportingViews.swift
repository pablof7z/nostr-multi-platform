import SwiftUI

struct SearchSectionHeader: View {
    let title: String
    let count: Int
    let target: SearchSeeAllTarget?

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Text(title)
                .font(.system(.title3, design: .default).weight(.semibold))
                .foregroundStyle(Color.highlighterInkStrong)
            if count > 0 {
                Text("\(count)")
                    .font(.caption.weight(.semibold).monospacedDigit())
                    .foregroundStyle(Color.highlighterInkMuted)
                    .padding(.horizontal, 6)
                    .padding(.vertical, 2)
                    .background {
                        Capsule()
                            .fill(Color.highlighterRule.opacity(0.55))
                    }
            }
            Spacer()
            if let target {
                NavigationLink(value: target) {
                    HStack(spacing: 4) {
                        Text("See all")
                        Image(systemName: "chevron.right")
                            .font(.caption2.weight(.semibold))
                    }
                    .font(.footnote.weight(.semibold))
                    .foregroundStyle(Color.highlighterAccent)
                }
            }
        }
        .padding(.bottom, 4)
    }
}

// MARK: - Destination targets

enum SearchSeeAllTarget: Hashable {
    case highlights(query: String)
    case articles(query: String)
    case communities(query: String)
    case people(query: String)

    var title: String {
        switch self {
        case .highlights: "Highlights"
        case .articles: "Articles"
        case .communities: "Communities"
        case .people: "People"
        }
    }

    var query: String {
        switch self {
        case .highlights(let q), .articles(let q), .communities(let q), .people(let q): q
        }
    }
}

// MARK: - Shared building blocks

struct SectionKicker: View {
    let text: String

    var body: some View {
        HStack(spacing: 10) {
            Rectangle()
                .fill(Color.highlighterAccent)
                .frame(width: 14, height: 1.5)
                .clipShape(RoundedRectangle(cornerRadius: 0.5))
            Text(text.uppercased())
                .font(.caption2.weight(.semibold).monospaced())
                .tracking(1.2)
                .foregroundStyle(Color.highlighterInkMuted)
        }
    }
}

struct RoomMiniTile: View {
    @Environment(HighlighterStore.self) private var app
    let room: CommunitySummary

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            RoomCoverArt(picture: room.picture, name: room.name, size: 86)
            Text(room.name)
                .font(.footnote.weight(.semibold))
                .foregroundStyle(Color.highlighterInkStrong)
                .lineLimit(2)
                .frame(width: 86, alignment: .leading)
        }
    }
}

struct RoomCoverArt: View {
    let picture: String
    let name: String
    let size: CGFloat

    var body: some View {
        ZStack {
            RoundedRectangle(cornerRadius: 12, style: .continuous)
                .fill(
                    LinearGradient(
                        colors: [
                            Color.highlighterAccent.opacity(0.35),
                            Color.highlighterTintPale
                        ],
                        startPoint: .topLeading,
                        endPoint: .bottomTrailing
                    )
                )
            if let url = URL(string: picture), !picture.isEmpty {
                AsyncImage(url: url) { phase in
                    if case .success(let image) = phase {
                        image.resizable().aspectRatio(contentMode: .fill)
                    }
                }
                .frame(width: size, height: size)
                .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
            } else {
                Text(String(name.prefix(1)))
                    .font(.system(size: size * 0.38, design: .default).weight(.semibold))
                    .foregroundStyle(Color.highlighterInkStrong.opacity(0.75))
            }
        }
        .frame(width: size, height: size)
        .overlay {
            RoundedRectangle(cornerRadius: 12, style: .continuous)
                .stroke(Color.highlighterRule, lineWidth: 0.5)
        }
    }
}

// MARK: - Row views

struct SearchHighlightRow: View {
    let highlight: HighlightRecord
    let query: String

    var body: some View {
        HStack(alignment: .top, spacing: 14) {
            if let pageURL = pageImageURL {
                HighlightPageImage(url: pageURL, treatment: .row)
            } else {
                Rectangle()
                    .fill(Color.highlighterAccent)
                    .frame(width: 2.5)
                    .clipShape(RoundedRectangle(cornerRadius: 1.25))
            }
            VStack(alignment: .leading, spacing: 6) {
                HighlightMatchedText(
                    text: highlight.quote,
                    query: query,
                    font: .system(size: 18, design: .default).italic()
                )
                .foregroundStyle(Color.highlighterInkStrong)
                .lineSpacing(3)
                .lineLimit(4)

                if !highlight.note.isEmpty {
                    Text("— " + highlight.note)
                        .font(.footnote.italic())
                        .foregroundStyle(Color.highlighterInkMuted)
                        .lineLimit(2)
                }
            }
        }
        .padding(.vertical, 14)
        .contentShape(Rectangle())
    }

    private var pageImageURL: URL? {
        let raw = highlight.imageUrl.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !raw.isEmpty else { return nil }
        return URL(string: raw)
    }
}

struct SearchCommunityRow: View {
    let community: CommunitySummary

    var body: some View {
        HStack(alignment: .center, spacing: 14) {
            RoomCoverArt(picture: community.picture, name: community.name, size: 54)
            VStack(alignment: .leading, spacing: 4) {
                Text(community.name)
                    .font(.callout.weight(.semibold))
                    .foregroundStyle(Color.highlighterInkStrong)
                    .lineLimit(1)
                if !community.about.isEmpty {
                    Text(community.about)
                        .font(.footnote)
                        .foregroundStyle(Color.highlighterInkMuted)
                        .lineLimit(2)
                }
                HStack(spacing: 6) {
                    Text(community.visibility.capitalized)
                        .font(.caption2.weight(.semibold))
                        .tracking(0.6)
                    Text("·")
                    Text(community.access.capitalized)
                        .font(.caption2.weight(.semibold))
                        .tracking(0.6)
                    if let count = community.memberCount {
                        Text("·")
                        Text("\(count) members")
                            .font(.caption2)
                    }
                }
                .foregroundStyle(Color.highlighterInkMuted)
            }
            Spacer()
            Image(systemName: "chevron.right")
                .font(.caption.weight(.semibold))
                .foregroundStyle(Color.highlighterInkMuted.opacity(0.6))
        }
        .padding(.vertical, 10)
        .contentShape(Rectangle())
    }
}

struct SearchProfileRow: View {
    @Environment(HighlighterStore.self) private var app
    let profile: ProfileMetadata

    var body: some View {
        HStack(spacing: 14) {
            AuthorAvatar(
                pubkey: profile.pubkey,
                pictureURL: profile.picture,
                displayInitial: String((profile.displayNameOrName).prefix(1)),
                size: 44
            )
            VStack(alignment: .leading, spacing: 2) {
                Text(profile.displayNameOrName)
                    .font(.callout.weight(.semibold))
                    .foregroundStyle(Color.highlighterInkStrong)
                    .lineLimit(1)
                if !profile.nip05.isEmpty {
                    Text(profile.nip05)
                        .font(.caption)
                        .foregroundStyle(Color.highlighterInkMuted)
                        .lineLimit(1)
                } else if !profile.about.isEmpty {
                    Text(profile.about)
                        .font(.caption)
                        .foregroundStyle(Color.highlighterInkMuted)
                        .lineLimit(1)
                }
            }
            Spacer()
            Image(systemName: "chevron.right")
                .font(.caption.weight(.semibold))
                .foregroundStyle(Color.highlighterInkMuted.opacity(0.6))
        }
        .padding(.vertical, 10)
        .contentShape(Rectangle())
    }
}

extension ProfileMetadata {
    var displayNameOrName: String {
        if !displayName.isEmpty { return displayName }
        if !name.isEmpty { return name }
        return String(pubkey.prefix(8))
    }
}

// MARK: - Matched-text rendering

/// Renders `text` with every case-insensitive occurrence of `query` wrapped in
/// a subtle highlighted span (terracotta ink, very faint background). Falls
/// back to plain text when the query is empty.
struct HighlightMatchedText: View {
    let text: String
    let query: String
    let font: Font

    var body: some View {
        Text(attributed)
            .font(font)
    }

    private var attributed: AttributedString {
        var out = AttributedString(text)
        let trimmed = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return out }

        let lowerText = text.lowercased()
        let lowerQuery = trimmed.lowercased()
        var searchRange = lowerText.startIndex..<lowerText.endIndex
        while let match = lowerText.range(of: lowerQuery, range: searchRange) {
            let startOffset = lowerText.distance(from: lowerText.startIndex, to: match.lowerBound)
            let endOffset = lowerText.distance(from: lowerText.startIndex, to: match.upperBound)
            if let s = out.index(out.startIndex, offsetByCharacters: startOffset),
               let e = out.index(out.startIndex, offsetByCharacters: endOffset),
               s < e {
                out[s..<e].foregroundColor = .highlighterAccent
                out[s..<e].backgroundColor = Color.laneArticleHighlightFill
            }
            searchRange = match.upperBound..<lowerText.endIndex
        }
        return out
    }
}

private extension AttributedString {
    /// Convenience — characters-based offset into the attributed string.
    func index(_ base: AttributedString.Index, offsetByCharacters n: Int) -> AttributedString.Index? {
        var idx = base
        var remaining = n
        while remaining > 0 {
            guard idx < endIndex else { return nil }
            idx = characters.index(after: idx)
            remaining -= 1
        }
        return idx
    }
}

// MARK: - Flow layout (chips)

/// Minimal flow layout that wraps child views left-to-right. Used for the
/// suggested-searches chip row so long terms wrap cleanly without the
/// default `HStack` cramming.
struct SearchFlowLayout: Layout {
    var spacing: CGFloat = 8
    var runSpacing: CGFloat = 8

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let maxWidth = proposal.width ?? .infinity
        var x: CGFloat = 0
        var y: CGFloat = 0
        var runHeight: CGFloat = 0
        var totalHeight: CGFloat = 0

        for sub in subviews {
            let size = sub.sizeThatFits(.unspecified)
            if x + size.width > maxWidth, x > 0 {
                y += runHeight + runSpacing
                x = 0
                runHeight = 0
            }
            x += size.width + spacing
            runHeight = max(runHeight, size.height)
            totalHeight = y + runHeight
        }
        return CGSize(width: maxWidth.isFinite ? maxWidth : x, height: totalHeight)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        let maxWidth = bounds.width
        var x: CGFloat = 0
        var y: CGFloat = 0
        var runHeight: CGFloat = 0
        for sub in subviews {
            let size = sub.sizeThatFits(.unspecified)
            if x + size.width > maxWidth, x > 0 {
                y += runHeight + runSpacing
                x = 0
                runHeight = 0
            }
            sub.place(at: CGPoint(x: bounds.minX + x, y: bounds.minY + y), proposal: .unspecified)
            x += size.width + spacing
            runHeight = max(runHeight, size.height)
        }
    }
}

