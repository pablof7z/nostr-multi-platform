import Kingfisher
import SwiftUI
import UIKit

/// Full-screen NIP-23 long-form reader. Handles the gorgeous header (cover,
/// serif title, author row, metadata), renders the body via `ArticleBodyView`,
/// and orchestrates the text-selection → highlight flow.
struct ArticleReaderView: View {
    let target: ArticleReaderTarget

    @Environment(HighlighterStore.self) private var app
    @State private var store: ArticleReaderStore?
    @State private var pendingHighlight: PendingHighlight?
    @State private var highlightDetail: HighlightRecord?
    @State private var toast: String?
    @State private var scrollAnchor: ScrollAnchor = .idle
    @State private var shareTarget: ShareToCommunityTarget?

    enum ScrollAnchor: Equatable {
        case idle
        case footnote(number: Int)
        case footnoteBack(number: Int)
    }

    struct PendingHighlight: Identifiable {
        let id = UUID()
        let quote: String
        let context: String
    }

    var body: some View {
        Group {
            if let store {
                content(store: store)
            } else {
                ProgressView()
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .background(Color.highlighterPaper)
            }
        }
        .background(Color.highlighterPaper.ignoresSafeArea())
        .navigationBarTitleDisplayMode(.inline)
        .toolbar(.hidden, for: .tabBar)
        .toolbarBackground(.hidden, for: .navigationBar)
        .toolbar {
            if let article = store?.article {
                let address = "30023:\(article.pubkey):\(article.identifier)"
                ToolbarItem(placement: .topBarTrailing) {
                    BookmarkMenuButton(articleAddress: address)
                }
                ToolbarItem(placement: .topBarTrailing) {
                    Button {
                        shareTarget = .article(article)
                    } label: {
                        Image(systemName: "square.and.arrow.up")
                    }
                    .accessibilityLabel("Share to community")
                }
            }
        }
        .sheet(item: $shareTarget) { target in
            ShareToCommunitySheet(target: target)
                .presentationDetents([.medium, .large])
        }
        .task(id: target) {
            if store == nil {
                let s = ArticleReaderStore(
                    target: target,
                    safeCore: app.safeCore,
                    eventBridge: app.eventBridge
                )
                store = s
                await s.start()
            }
        }
        .task(id: target.pubkey) {
            await app.requestProfile(pubkeyHex: target.pubkey)
        }
        .onDisappear {
            store?.stop()
        }
        .sheet(item: $pendingHighlight) { pending in
            NoteComposerSheet(
                quote: pending.quote,
                onCancel: { pendingHighlight = nil },
                onSave: { note in
                    Task { await publish(quote: pending.quote, context: pending.context, note: note) }
                    pendingHighlight = nil
                }
            )
            .presentationDetents([.medium])
        }
        .sheet(item: Binding(
            get: { highlightDetail.map { IdentifiedHighlight(record: $0) } },
            set: { highlightDetail = $0?.record }
        )) { ih in
            HighlightDetailSheet(highlight: ih.record)
                .presentationDetents([.medium, .large])
        }
        .safeAreaInset(edge: .bottom) {
            if let toast {
                Text(toast)
                    .font(.footnote.weight(.medium))
                    .foregroundStyle(.white)
                    .padding(.horizontal, 14)
                    .padding(.vertical, 10)
                    .background(Color.highlighterAccent.opacity(0.95), in: Capsule())
                    .padding(.horizontal, 20)
                    .padding(.bottom, 12)
                    .transition(.move(edge: .bottom).combined(with: .opacity))
            }
        }
        .modifier(ArticleCommentsAttachmentModifier(article: store?.article, target: target))
    }

    // MARK: - Content

    @ViewBuilder
    private func content(store: ArticleReaderStore) -> some View {
        if store.isLoadingInitial && store.article == nil {
            ProgressView()
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        } else if let article = store.article {
            ReaderScroll(
                article: article,
                authorProfile: app.profileCache[target.pubkey] ?? store.authorProfile,
                highlights: store.highlights,
                scrollAnchor: scrollAnchor,
                onPublishHighlight: { quote, context in
                    Task { await publish(quote: quote, context: context, note: "") }
                },
                onRequestNote: { quote, context in
                    pendingHighlight = PendingHighlight(quote: quote, context: context)
                },
                onHighlightTap: { highlightDetail = $0 },
                onFootnoteTap: { number in
                    scrollAnchor = .footnote(number: number)
                },
                onFootnoteBackTap: { number in
                    scrollAnchor = .footnoteBack(number: number)
                }
            )
        } else {
            ContentUnavailableView(
                "Couldn't load this article",
                systemImage: "doc.text",
                description: Text("We'll keep listening — it may arrive over the network in a moment.")
            )
        }
    }

    // MARK: - Actions

    private func publish(quote: String, context: String, note: String) async {
        guard let store else { return }
        do {
            _ = try await store.publishHighlight(
                quote: quote,
                note: note,
                context: context
            )
            withAnimation(.easeOut(duration: 0.2)) {
                toast = note.isEmpty ? "Highlighted" : "Highlighted with note"
            }
            try? await Task.sleep(nanoseconds: 1_800_000_000)
            withAnimation(.easeIn(duration: 0.2)) { toast = nil }
        } catch {
            withAnimation(.easeOut(duration: 0.2)) {
                toast = "Couldn't save — \(error.localizedDescription)"
            }
            try? await Task.sleep(nanoseconds: 2_800_000_000)
            withAnimation(.easeIn(duration: 0.2)) { toast = nil }
        }
    }
}

// MARK: - Comments attachment

/// Tiny adapter that mounts the premium NIP-22 comments toolbar + sheet
/// against an article. The article's address (`30023:<pubkey>:<d>`) is
/// the NIP-22 root scope, so we always have the artifact ref even
/// before the body finishes loading.
private struct ArticleCommentsAttachmentModifier: ViewModifier {
    let article: ArticleRecord?
    let target: ArticleReaderTarget

    func body(content: Content) -> some View {
        content.commentsAttachment(
            artifact: .article(addr: target.address),
            artifactAuthorPubkey: target.pubkey
        )
    }
}

// MARK: - Scroll container composing header + body

private struct ReaderScroll: View {
    let article: ArticleRecord
    let authorProfile: ProfileMetadata?
    let highlights: [HighlightRecord]
    let scrollAnchor: ArticleReaderView.ScrollAnchor
    var onPublishHighlight: (String, String) -> Void
    var onRequestNote: (String, String) -> Void
    var onHighlightTap: (HighlightRecord) -> Void
    var onFootnoteTap: (Int) -> Void
    var onFootnoteBackTap: (Int) -> Void

    @State private var rendered: MarkdownRenderer.Output?
    @State private var imageToOpen: IdentifiableURL?
    @State private var profileNavPubkey: String?
    @State private var profileNavActive = false
    @Environment(HighlighterStore.self) private var app

    private struct IdentifiableURL: Identifiable {
        let url: URL
        var id: String { url.absoluteString }
    }

    private var coverURL: URL? {
        guard !article.image.isEmpty else { return nil }
        return URL(string: article.image)
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 0) {
                if let coverURL {
                    HeroImage(url: coverURL)
                }

                Header(article: article, authorProfile: authorProfile)
                    .padding(.horizontal, 20)
                    .padding(.top, coverURL == nil ? 10 : 20)
                    .padding(.bottom, 12)

                if let rendered {
                    bodySegments(rendered)
                }

                NavigationLink(
                    destination: Group {
                        if let pk = profileNavPubkey {
                            ProfileView(pubkey: pk)
                        }
                    },
                    isActive: $profileNavActive
                ) { EmptyView() }
                    .hidden()
            }
        }
        .ignoresSafeArea(edges: coverURL == nil ? [] : .top)
        .fullScreenCover(item: $imageToOpen) { item in
            ImageZoomView(url: item.url, onDismiss: { imageToOpen = nil })
        }
        .task(id: "\(article.eventId)-\(highlights.count)-\(app.profileCache.count)") {
            let profileSnapshot = Dictionary(
                uniqueKeysWithValues: app.profileCache.compactMap { (pk, meta) -> (String, String)? in
                    let name = meta.displayName.isEmpty ? meta.name : meta.displayName
                    guard !name.isEmpty else { return nil }
                    return (pk, name)
                }
            )
            let safeCore = app.safeCore
            rendered = await Task.detached(priority: .userInitiated) {
                MarkdownRenderer.render(
                    content: article.content,
                    highlights: highlights,
                    accent: UIColor(Color.highlighterAccent),
                    tint: UIColor(Color.highlighterAccent),
                    ink: UIColor(Color.highlighterInkStrong),
                    muted: UIColor(Color.highlighterInkMuted),
                    nostrDecoder: { input in try? safeCore.decodeNostrEntity(input) },
                    profileNames: profileSnapshot
                )
            }.value
        }
    }

    @ViewBuilder
    private func bodySegments(_ output: MarkdownRenderer.Output) -> some View {
        ForEach(Array(output.segments.enumerated()), id: \.offset) { idx, segment in
            switch segment {
            case .text(let attrStr):
                let isLast = idx == output.segments.count - 1
                ArticleBodyView(
                    attributedText: isLast ? withFootnotes(attrStr, output) : attrStr,
                    footnoteAnchors: isLast ? output.footnoteAnchors : [:],
                    footnoteBackAnchors: [:],
                    highlightsById: output.highlightsById,
                    paperColor: UIColor(Color.highlighterPaper),
                    onPublishHighlight: onPublishHighlight,
                    onRequestNote: onRequestNote,
                    onHighlightTap: onHighlightTap,
                    onFootnoteTap: onFootnoteTap,
                    onFootnoteBackTap: onFootnoteBackTap,
                    onImageTap: { url in imageToOpen = IdentifiableURL(url: url) },
                    onProfileTap: { pk in
                        profileNavPubkey = pk
                        profileNavActive = true
                    }
                )
                .frame(maxWidth: .infinity)
            case .image(let url, let alt):
                InlineArticleImage(url: url, alt: alt)
            case .nostrEntity(let ref):
                NostrEntityCard(entity: ref)
                    .padding(.horizontal, 20)
                    .padding(.vertical, 4)
            }
        }
    }

    private func withFootnotes(_ body: NSAttributedString, _ output: MarkdownRenderer.Output) -> NSAttributedString {
        guard output.footnotes.length > 0 else { return body }
        let out = NSMutableAttributedString(attributedString: body)
        out.append(NSAttributedString(
            string: "\n———\n\n",
            attributes: [
                .font: UIFont.systemFont(ofSize: 14, weight: .semibold),
                .foregroundColor: UIColor(Color.highlighterInkMuted)
            ]
        ))
        out.append(NSAttributedString(
            string: "Footnotes\n\n",
            attributes: [
                .font: UIFont.systemFont(ofSize: 12, weight: .bold),
                .foregroundColor: UIColor(Color.highlighterInkMuted),
                .kern: 0.6
            ]
        ))
        out.append(output.footnotes)
        return out
    }
}

// MARK: - Inline image

private struct InlineArticleImage: View {
    let url: URL
    let alt: String

    @State private var showFullScreen = false

    var body: some View {
        KFImage(url)
            .placeholder {
                Color.highlighterRule.opacity(0.4)
                    .frame(height: 200)
            }
            .fade(duration: 0.2)
            .resizable()
            .scaledToFit()
            .frame(maxWidth: .infinity)
            .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
            .contentShape(Rectangle())
            .onTapGesture { showFullScreen = true }
            .padding(.horizontal, 20)
            .padding(.vertical, 8)
            .fullScreenCover(isPresented: $showFullScreen) {
                ImageZoomView(url: url, onDismiss: { showFullScreen = false })
            }
    }
}

// MARK: - Hero image

/// Full-bleed cover that extends behind the status bar / notch. Sized by
/// GeometryReader so it scales to the device width even when the parent
/// ScrollView is `.ignoresSafeArea(.top)`.
private struct HeroImage: View {
    let url: URL

    @State private var showFullScreen = false

    var body: some View {
        GeometryReader { proxy in
            KFImage(url)
                .placeholder { Color.highlighterRule.opacity(0.5) }
                .fade(duration: 0.2)
                .resizable()
                .scaledToFill()
                .frame(width: proxy.size.width, height: proxy.size.height)
                .clipped()
                .onTapGesture { showFullScreen = true }
        }
        .frame(height: 320)
        .fullScreenCover(isPresented: $showFullScreen) {
            ImageZoomView(url: url, onDismiss: { showFullScreen = false })
        }
    }
}

// MARK: - Header

private struct Header: View {
    let article: ArticleRecord
    let authorProfile: ProfileMetadata?

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text(article.title.isEmpty ? "Untitled" : article.title)
                .font(.largeTitle.weight(.bold))
                .foregroundStyle(Color.highlighterInkStrong)
                .fixedSize(horizontal: false, vertical: true)

            if !article.summary.isEmpty {
                Text(article.summary)
                    .font(.system(.title3, design: .default))
                    .foregroundStyle(Color.highlighterInkMuted)
                    .fixedSize(horizontal: false, vertical: true)
            }

            authorRow

            if !article.hashtags.isEmpty {
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 8) {
                        ForEach(article.hashtags.prefix(12), id: \.self) { tag in
                            Text("#\(tag)")
                                .font(.caption.weight(.medium))
                                .foregroundStyle(Color.highlighterAccent)
                                .padding(.horizontal, 10)
                                .padding(.vertical, 4)
                                .background(
                                    Capsule().fill(Color.highlighterAccent.opacity(0.08))
                                )
                        }
                    }
                }
            }

            Rectangle()
                .fill(Color.highlighterRule)
                .frame(height: 1)
                .padding(.top, 6)
        }
    }

    @ViewBuilder
    private var authorRow: some View {
        NavigationLink(value: ProfileDestination.pubkey(article.pubkey)) {
            HStack(spacing: 12) {
                AuthorAvatar(
                    pubkey: article.pubkey,
                    pictureURL: authorProfile?.picture ?? "",
                    displayInitial: initial,
                    size: 40,
                    ringWidth: 2
                )

                VStack(alignment: .leading, spacing: 2) {
                    Text(authorDisplayName)
                        .font(.subheadline.weight(.semibold))
                        .foregroundStyle(Color.highlighterInkStrong)
                    HStack(spacing: 6) {
                        if let date = displayDate {
                            Text(date)
                        }
                        if let mins = readTimeMinutes {
                            Text("·")
                            Text("\(mins) min read")
                        }
                    }
                    .font(.caption)
                    .foregroundStyle(Color.highlighterInkMuted)
                }
                Spacer(minLength: 0)
            }
        }
        .buttonStyle(.plain)
    }

    private var initial: String {
        authorDisplayName.first.map { String($0).uppercased() } ?? "?"
    }

    private var authorDisplayName: String {
        let dn = authorProfile?.displayName ?? ""
        if !dn.isEmpty { return dn }
        let n = authorProfile?.name ?? ""
        if !n.isEmpty { return n }
        return String(article.pubkey.prefix(10))
    }

    private var displayDate: String? {
        let seconds = article.publishedAt ?? article.createdAt ?? 0
        guard seconds > 0 else { return nil }
        let date = Date(timeIntervalSince1970: TimeInterval(seconds))
        let formatter = DateFormatter()
        formatter.dateStyle = .medium
        formatter.timeStyle = .none
        return formatter.string(from: date)
    }

    /// Rough read-time estimate: 240 wpm.
    private var readTimeMinutes: Int? {
        let words = article.content.split(whereSeparator: { $0.isWhitespace }).count
        guard words > 60 else { return nil }
        return max(1, words / 240)
    }
}

// MARK: - Note composer sheet

private struct NoteComposerSheet: View {
    let quote: String
    var onCancel: () -> Void
    var onSave: (String) -> Void

    @State private var note: String = ""
    @FocusState private var focused: Bool

    var body: some View {
        NavigationStack {
            VStack(alignment: .leading, spacing: 12) {
                Text(quote)
                    .font(.system(.body, design: .default).italic())
                    .foregroundStyle(Color.highlighterInkStrong)
                    .padding(12)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(Color.highlighterAccent.opacity(0.12), in: RoundedRectangle(cornerRadius: 10))

                TextField("Add a note…", text: $note, axis: .vertical)
                    .lineLimit(3...8)
                    .focused($focused)
                    .textFieldStyle(.roundedBorder)

                Spacer(minLength: 0)
            }
            .padding(.horizontal, 20)
            .padding(.top, 20)
            .navigationTitle("Highlight")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button("Cancel", action: onCancel)
                }
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Save") { onSave(note.trimmingCharacters(in: .whitespacesAndNewlines)) }
                        .fontWeight(.semibold)
                }
            }
            .onAppear { focused = true }
        }
    }
}

// MARK: - Highlight detail sheet

private struct IdentifiedHighlight: Identifiable {
    var id: String { record.eventId }
    let record: HighlightRecord
}

private struct HighlightDetailSheet: View {
    let highlight: HighlightRecord

    @Environment(HighlighterStore.self) private var app
    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    authorRow
                    quoteBlock
                    if !highlight.note.isEmpty {
                        noteBlock
                    }
                    if let ts = highlight.createdAt {
                        Text(Date(timeIntervalSince1970: TimeInterval(ts)).formatted(date: .abbreviated, time: .shortened))
                            .font(.caption)
                            .foregroundStyle(Color.highlighterInkMuted)
                    }
                }
                .padding(.horizontal, 20)
                .padding(.top, 24)
                .padding(.bottom, 40)
            }
            .background(Color.highlighterPaper)
            .navigationBarTitleDisplayMode(.inline)
        }
        .task(id: highlight.pubkey) {
            await app.requestProfile(pubkeyHex: highlight.pubkey)
        }
    }

    private var authorRow: some View {
        HStack(spacing: 12) {
            AuthorAvatar(
                pubkey: highlight.pubkey,
                pictureURL: app.profileCache[highlight.pubkey]?.picture ?? "",
                displayInitial: initial,
                size: 40,
                ringWidth: 2
            )
            VStack(alignment: .leading, spacing: 2) {
                Text(displayName)
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(Color.highlighterInkStrong)
                Text("highlighted")
                    .font(.caption)
                    .foregroundStyle(Color.highlighterInkMuted)
            }
            Spacer(minLength: 0)
        }
    }

    private var quoteBlock: some View {
        HStack(alignment: .top, spacing: 0) {
            Rectangle()
                .fill(Color.highlighterAccent)
                .frame(width: 3)
            Text(highlight.quote)
                .font(.system(.body, design: .default))
                .foregroundStyle(Color.highlighterInkStrong)
                .padding(14)
                .frame(maxWidth: .infinity, alignment: .leading)
                .fixedSize(horizontal: false, vertical: true)
        }
        .background(Color.highlighterAccent.opacity(0.08), in: RoundedRectangle(cornerRadius: 10))
    }

    private var noteBlock: some View {
        Text(highlight.note)
            .font(.body)
            .foregroundStyle(Color.highlighterInkMuted)
            .frame(maxWidth: .infinity, alignment: .leading)
            .fixedSize(horizontal: false, vertical: true)
    }

    private var displayName: String {
        let profile = app.profileCache[highlight.pubkey]
        let dn = profile?.displayName ?? ""
        if !dn.isEmpty { return dn }
        let n = profile?.name ?? ""
        if !n.isEmpty { return n }
        return String(highlight.pubkey.prefix(10))
    }

    private var initial: String {
        displayName.first.map { String($0).uppercased() } ?? "?"
    }
}
