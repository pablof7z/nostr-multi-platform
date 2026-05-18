import SwiftUI

/// The search destination. Tap the liquid-glass search button in any tab's
/// toolbar to land here.
///
/// Layout follows Apple's search-fields HIG for a dedicated discovery
/// destination:
///
/// - The field starts unfocused so the editorial empty state can breathe.
/// - Suggested terms and recent searches sit above a curated browse section
///   (so an empty query isn't a dead screen).
/// - As the user types, results appear in sections — Highlights, Articles,
///   Communities, People — each with a "See all" row that drills into a
///   kind-specific sub-screen.
/// - NIP-50 relay results fade into the Articles section as the relays
///   reply; there's no separate "web results" bucket to make the user cross
///   between local and remote.
struct SearchView: View {
    @Environment(HighlighterStore.self) private var app

    @State private var store: SearchStore?
    @FocusState private var focusedField: Bool
    @State private var recentQueries: [String] = RecentSearches.all()

    var body: some View {
        NavigationStack {
            ZStack {
                Color.highlighterPaper.ignoresSafeArea()
                if let store {
                    content(store: store)
                } else {
                    Color.clear
                }
            }
            .navigationTitle("Search")
            .navigationBarTitleDisplayMode(.large)
            .searchable(
                text: Binding(
                    get: { store?.query ?? "" },
                    set: { new in store?.query = new }
                ),
                placement: .navigationBarDrawer(displayMode: .always),
                prompt: Text("Quotes, essays, people, rooms")
            )
            .searchFocused($focusedField)
            .onSubmit(of: .search) {
                commitRecentQuery()
            }
            .navigationDestination(for: ArticleReaderTarget.self) { target in
                ArticleReaderView(target: target)
            }
            .navigationDestination(for: WebReaderTarget.self) { target in
                WebReaderView(target: target)
            }
            .navigationDestination(for: ProfileDestination.self) { destination in
                if case .pubkey(let pk) = destination {
                    ProfileView(pubkey: pk)
                }
            }
            .navigationDestination(for: String.self) { groupId in
                RoomHomeView(groupId: groupId)
            }
            .navigationDestination(for: SearchSeeAllTarget.self) { target in
                if let store {
                    SearchSeeAllView(target: target, store: store)
                }
            }
            .globalUserToolbar()
        }
        .task {
            if store == nil {
                let s = SearchStore(safeCore: app.safeCore, eventBridge: app.eventBridge)
                store = s
                await s.start()
            }
        }
        .onDisappear {
            store?.stop()
        }
    }

    // MARK: - Content switcher

    @ViewBuilder
    private func content(store: SearchStore) -> some View {
        let q = store.query.trimmingCharacters(in: .whitespacesAndNewlines)
        if q.isEmpty {
            emptyState(store: store)
        } else {
            results(store: store)
        }
    }

    // MARK: - Empty (discovery) state

    private func emptyState(store: SearchStore) -> some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 28) {
                if !recentQueries.isEmpty {
                    recentSection
                }
                suggestedSection(store: store)
                browseRoomsSection
                browseHighlightsPreviewSection(store: store)
                browseRelaysFootnote(store: store)
            }
            .padding(.horizontal, 20)
            .padding(.top, 8)
            .padding(.bottom, 40)
        }
        .scrollDismissesKeyboard(.interactively)
    }

    private var recentSection: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(alignment: .firstTextBaseline) {
                SectionKicker(text: "Recent")
                Spacer()
                Button("Clear") {
                    RecentSearches.clear()
                    recentQueries = []
                }
                .font(.caption.weight(.semibold))
                .foregroundStyle(Color.highlighterInkMuted)
            }
            VStack(spacing: 0) {
                ForEach(Array(recentQueries.enumerated()), id: \.element) { index, q in
                    Button {
                        store?.submit(q)
                    } label: {
                        HStack(spacing: 12) {
                            Image(systemName: "clock")
                                .font(.footnote)
                                .foregroundStyle(Color.highlighterInkMuted)
                            Text(q)
                                .font(.callout)
                                .foregroundStyle(Color.highlighterInkStrong)
                            Spacer()
                            Image(systemName: "arrow.up.left")
                                .font(.caption)
                                .foregroundStyle(Color.highlighterInkMuted.opacity(0.8))
                        }
                        .padding(.vertical, 10)
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)

                    if index < recentQueries.count - 1 {
                        Rectangle()
                            .fill(Color.highlighterRule)
                            .frame(height: 0.5)
                    }
                }
            }
        }
    }

    @ViewBuilder
    private func suggestedSection(store: SearchStore) -> some View {
        let suggestions = suggestedQueries()
        if !suggestions.isEmpty {
            VStack(alignment: .leading, spacing: 12) {
                SectionKicker(text: "Try")
                FlowLayout(spacing: 10, runSpacing: 10) {
                    ForEach(suggestions, id: \.self) { term in
                        Button {
                            store.submit(term)
                        } label: {
                            Text(term)
                                .font(.callout.weight(.medium))
                                .foregroundStyle(Color.highlighterInkStrong)
                                .padding(.horizontal, 14)
                                .padding(.vertical, 9)
                                .background {
                                    Capsule()
                                        .fill(Color.highlighterTintPale)
                                }
                                .overlay {
                                    Capsule()
                                        .stroke(Color.highlighterRule, lineWidth: 0.5)
                                }
                        }
                        .buttonStyle(.plain)
                    }
                }
            }
        }
    }

    private var browseRoomsSection: some View {
        VStack(alignment: .leading, spacing: 12) {
            SectionKicker(text: "Your rooms")
            if app.joinedCommunities.isEmpty {
                Text("Rooms you join appear here.")
                    .font(.footnote)
                    .foregroundStyle(Color.highlighterInkMuted)
            } else {
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 12) {
                        ForEach(app.joinedCommunities.prefix(12)) { room in
                            NavigationLink(value: room.id) {
                                RoomMiniTile(room: room)
                            }
                            .buttonStyle(.plain)
                        }
                    }
                    .padding(.horizontal, 2)
                }
                .scrollClipDisabled()
            }
        }
    }

    @ViewBuilder
    private func browseHighlightsPreviewSection(store: SearchStore) -> some View {
        SectionKicker(text: "The library")
        Text("Your nostrdb cache holds every highlight, article, community, and profile you've ever loaded. Search finds them instantly. Anything not yet on your device — searched across your configured search relays.")
            .font(.system(.subheadline, design: .default))
            .foregroundStyle(Color.highlighterInkMuted)
            .lineSpacing(4)
            .padding(.top, -4)
    }

    @ViewBuilder
    private func browseRelaysFootnote(store: SearchStore) -> some View {
        if !store.searchRelays.isEmpty {
            VStack(alignment: .leading, spacing: 8) {
                SectionKicker(text: "Search relays")
                ForEach(store.searchRelays, id: \.self) { url in
                    HStack(spacing: 10) {
                        Circle()
                            .fill(Color.highlighterAccent.opacity(0.7))
                            .frame(width: 5, height: 5)
                        Text(displayRelay(url))
                            .font(.footnote.monospaced())
                            .foregroundStyle(Color.highlighterInkMuted)
                    }
                }
            }
            .padding(.top, 8)
        }
    }

    // MARK: - Results state

    private func results(store: SearchStore) -> some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 28) {
                if store.isLocalLoading && allEmpty(store: store) {
                    loadingSkeleton
                } else if allEmpty(store: store) && !store.isRelayLoading {
                    noResults(store: store)
                } else {
                    highlightsResultsSection(store: store)
                    articlesResultsSection(store: store)
                    communitiesResultsSection(store: store)
                    peopleResultsSection(store: store)
                    if store.isRelayLoading {
                        relayLoadingHint
                    }
                }
            }
            .padding(.horizontal, 20)
            .padding(.top, 12)
            .padding(.bottom, 40)
        }
        .scrollDismissesKeyboard(.interactively)
    }

    private var loadingSkeleton: some View {
        VStack(alignment: .leading, spacing: 16) {
            ForEach(0..<3, id: \.self) { _ in
                RoundedRectangle(cornerRadius: 4)
                    .fill(Color.highlighterRule.opacity(0.5))
                    .frame(height: 14)
                    .frame(maxWidth: .infinity)
                    .padding(.trailing, CGFloat.random(in: 40...160))
            }
        }
        .padding(.vertical, 20)
    }

    private func noResults(store: SearchStore) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            Rectangle()
                .fill(Color.highlighterAccent.opacity(0.6))
                .frame(width: 3, height: 24)
                .clipShape(RoundedRectangle(cornerRadius: 1.5))
            Text("Nothing yet for \u{201C}\(store.query)\u{201D}")
                .font(.system(.title3, design: .default).weight(.semibold))
                .foregroundStyle(Color.highlighterInkStrong)
            Text("Relay search is still running in the background — results may arrive in a moment.")
                .font(.footnote)
                .foregroundStyle(Color.highlighterInkMuted)
        }
        .padding(.top, 36)
    }

    private var relayLoadingHint: some View {
        HStack(spacing: 10) {
            ProgressView()
                .controlSize(.small)
            Text("Searching the web…")
                .font(.footnote)
                .foregroundStyle(Color.highlighterInkMuted)
        }
        .padding(.top, 8)
    }

    // MARK: - Sections

    @ViewBuilder
    private func highlightsResultsSection(store: SearchStore) -> some View {
        if !store.highlights.isEmpty {
            SearchSectionHeader(
                title: "Highlights",
                count: store.highlights.count,
                target: store.highlights.count > 4
                    ? .highlights(query: store.query) : nil
            )
            VStack(spacing: 0) {
                ForEach(Array(store.highlights.prefix(4).enumerated()), id: \.element.eventId) { idx, highlight in
                    highlightRow(highlight)
                    if idx < min(store.highlights.count, 4) - 1 {
                        Rectangle()
                            .fill(Color.highlighterRule)
                            .frame(height: 0.5)
                    }
                }
            }
        }
    }

    @ViewBuilder
    private func articlesResultsSection(store: SearchStore) -> some View {
        if !store.articles.isEmpty {
            SearchSectionHeader(
                title: "Articles",
                count: store.articles.count,
                target: store.articles.count > 4
                    ? .articles(query: store.query) : nil
            )
            VStack(spacing: 0) {
                ForEach(Array(store.articles.prefix(4).enumerated()), id: \.element.eventId) { idx, article in
                    articleRow(article)
                    if idx < min(store.articles.count, 4) - 1 {
                        Rectangle()
                            .fill(Color.highlighterRule)
                            .frame(height: 0.5)
                    }
                }
            }
        }
    }

    @ViewBuilder
    private func communitiesResultsSection(store: SearchStore) -> some View {
        if !store.communities.isEmpty {
            SearchSectionHeader(
                title: "Communities",
                count: store.communities.count,
                target: store.communities.count > 3
                    ? .communities(query: store.query) : nil
            )
            VStack(spacing: 0) {
                ForEach(Array(store.communities.prefix(3).enumerated()), id: \.element.id) { idx, c in
                    NavigationLink(value: c.id) {
                        SearchCommunityRow(community: c)
                    }
                    .buttonStyle(.plain)
                    if idx < min(store.communities.count, 3) - 1 {
                        Rectangle()
                            .fill(Color.highlighterRule)
                            .frame(height: 0.5)
                    }
                }
            }
        }
    }

    @ViewBuilder
    private func peopleResultsSection(store: SearchStore) -> some View {
        if !store.profiles.isEmpty {
            SearchSectionHeader(
                title: "People",
                count: store.profiles.count,
                target: store.profiles.count > 5
                    ? .people(query: store.query) : nil
            )
            VStack(spacing: 0) {
                ForEach(Array(store.profiles.prefix(5).enumerated()), id: \.element.pubkey) { idx, profile in
                    NavigationLink(value: ProfileDestination.pubkey(profile.pubkey)) {
                        SearchProfileRow(profile: profile)
                    }
                    .buttonStyle(.plain)
                    if idx < min(store.profiles.count, 5) - 1 {
                        Rectangle()
                            .fill(Color.highlighterRule)
                            .frame(height: 0.5)
                    }
                }
            }
        }
    }

    // MARK: - Rows (dispatch to shared components)

    @ViewBuilder
    private func highlightRow(_ h: HighlightRecord) -> some View {
        if let target = articleReaderTarget(for: h) {
            NavigationLink(value: target) {
                SearchHighlightRow(highlight: h, query: store?.query ?? "")
            }
            .buttonStyle(.plain)
        } else {
            SearchHighlightRow(highlight: h, query: store?.query ?? "")
        }
    }

    @ViewBuilder
    private func articleRow(_ a: ArticleRecord) -> some View {
        NavigationLink(value: ArticleReaderTarget(pubkey: a.pubkey, dTag: a.identifier, seed: nil)) {
            ArticleCardView(article: a)
        }
        .buttonStyle(.plain)
        .articleRowActions(article: a)
    }

    // MARK: - Helpers

    private func commitRecentQuery() {
        let q = (store?.query ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
        guard !q.isEmpty else { return }
        RecentSearches.record(q)
        recentQueries = RecentSearches.all()
    }

    private func allEmpty(store: SearchStore) -> Bool {
        store.highlights.isEmpty
            && store.articles.isEmpty
            && store.communities.isEmpty
            && store.profiles.isEmpty
    }

    private func articleReaderTarget(for h: HighlightRecord) -> ArticleReaderTarget? {
        let addr = h.artifactAddress.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !addr.isEmpty else { return nil }
        let parts = addr.split(separator: ":", maxSplits: 2, omittingEmptySubsequences: false)
        guard parts.count == 3, parts[0] == "30023" else { return nil }
        return ArticleReaderTarget(pubkey: String(parts[1]), dTag: String(parts[2]), seed: nil)
    }

    private func suggestedQueries() -> [String] {
        var out: [String] = []
        var seen = Set<String>()
        let push: (String) -> Void = { term in
            let trimmed = term.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !trimmed.isEmpty else { return }
            let key = trimmed.lowercased()
            if seen.insert(key).inserted {
                out.append(trimmed)
            }
        }
        // Room names are the user's strongest existing signal.
        for c in app.joinedCommunities.prefix(4) {
            push(c.name)
        }
        // A handful of evergreen topics so the chips never feel empty.
        for term in ["Dostoevsky", "Bitcoin", "Attention", "Borges", "Philosophy"] {
            if out.count >= 8 { break }
            push(term)
        }
        return Array(out.prefix(8))
    }

    private func displayRelay(_ url: String) -> String {
        url
            .replacingOccurrences(of: "wss://", with: "")
            .replacingOccurrences(of: "ws://", with: "")
    }
}

// MARK: - Section header

private struct SearchSectionHeader: View {
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

private struct SectionKicker: View {
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

private struct RoomMiniTile: View {
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

private struct RoomCoverArt: View {
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

private struct SearchHighlightRow: View {
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

private struct SearchCommunityRow: View {
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

private struct SearchProfileRow: View {
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

private extension ProfileMetadata {
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
private struct HighlightMatchedText: View {
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
private struct FlowLayout: Layout {
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

