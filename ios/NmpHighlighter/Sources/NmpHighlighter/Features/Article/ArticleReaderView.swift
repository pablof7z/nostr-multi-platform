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