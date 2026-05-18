import Foundation
import Observation

/// Canonical identity for the article reader. Pass this to
/// `ArticleReaderView`; the store derives everything from `pubkey` + `dTag`
/// and falls back to the seed for the first paint while ndb catches up.
struct ArticleReaderTarget: Hashable, Sendable {
    let pubkey: String
    let dTag: String
    /// Optional seed used for the first paint (article cards already hold an
    /// `ArticleRecord`; reusing it avoids a blank flash while ndb answers).
    let seed: ArticleRecord?

    /// Canonical NIP-33 `a`-tag value.
    var address: String { "30023:\(pubkey):\(dTag)" }

    static func == (lhs: ArticleReaderTarget, rhs: ArticleReaderTarget) -> Bool {
        lhs.pubkey == rhs.pubkey && lhs.dTag == rhs.dTag
    }

    func hash(into hasher: inout Hasher) {
        hasher.combine(pubkey)
        hasher.combine(dTag)
    }
}

/// View-scoped store for the article reader. Lifetime matches the
/// `ArticleReaderView` that owns it — created in `.task`, torn down in
/// `.onDisappear`. Subscribes via `subscribe_article` so live kind:30023
/// supersessions and new kind:9802 highlights trigger re-queries.
///
/// Architecture: **nostrdb is the source of truth.** The store never holds
/// data that isn't already in (or en-route to) ndb.
@MainActor
@Observable
final class ArticleReaderStore {
    // Reactive state driving the view.
    var article: ArticleRecord?
    var authorProfile: ProfileMetadata?
    var highlights: [HighlightRecord] = []
    var isLoadingInitial: Bool = true
    var loadError: String?
    /// Transient flash when a highlight the user just published echoes back.
    var lastPublishedHighlightId: String?

    // Plumbing.
    @ObservationIgnored let target: ArticleReaderTarget
    @ObservationIgnored let safeCore: SafeHighlighterCore
    @ObservationIgnored weak var eventBridge: EventBridge?
    @ObservationIgnored private var subscriptionHandle: UInt64?

    init(
        target: ArticleReaderTarget,
        safeCore: SafeHighlighterCore,
        eventBridge: EventBridge?
    ) {
        self.target = target
        self.safeCore = safeCore
        self.eventBridge = eventBridge
        self.article = target.seed
    }

    func start() async {
        await loadAll()
        isLoadingInitial = false
        await installSubscription()
    }

    func stop() {
        if let handle = subscriptionHandle {
            Task { [safeCore] in await safeCore.unsubscribe(handle) }
            eventBridge?.unregister(handle: handle)
            subscriptionHandle = nil
        }
    }

    // MARK: - Loads

    func loadAll() async {
        async let articleTask: ArticleRecord? = {
            try? await safeCore.getArticle(pubkeyHex: target.pubkey, dTag: target.dTag)
        }()
        async let highlightsTask: [HighlightRecord] = {
            (try? await safeCore.getHighlightsForArticle(address: target.address)) ?? []
        }()
        async let profileTask: ProfileMetadata? = {
            try? await safeCore.getUserProfile(pubkeyHex: target.pubkey)
        }()

        let (article, highlights, profile) = await (articleTask, highlightsTask, profileTask)
        if let article {
            self.article = article
        }
        self.highlights = highlights
        if let profile {
            self.authorProfile = profile
        }
    }

    /// Called by `EventBridge` when an `ArticleUpdated` delta arrives.
    /// Re-queries only the slice affected by the event kind.
    func applyUpdate(kind: UInt32) async {
        switch kind {
        case 30023:
            if let article = try? await safeCore.getArticle(
                pubkeyHex: target.pubkey,
                dTag: target.dTag
            ) {
                self.article = article
            }
        case 9802:
            if let list = try? await safeCore.getHighlightsForArticle(address: target.address) {
                self.highlights = list
            }
        default:
            break
        }
    }

    // MARK: - Writes

    /// Publish a solo NIP-84 highlight for the currently loaded article.
    /// Returns the record so the view can flash the new overlay without
    /// waiting for the subscription to echo back.
    ///
    /// Errors bubble up so the caller can surface them in a toast — we avoid
    /// swallowing them to keep ndb the single source of truth about what
    /// has actually been persisted.
    func publishHighlight(quote: String, note: String, context: String) async throws -> HighlightRecord {
        guard let article else {
            throw NSError(
                domain: "ArticleReaderStore",
                code: 1,
                userInfo: [NSLocalizedDescriptionKey: "Article not yet loaded."]
            )
        }
        let artifact = articleAsArtifact(article)
        let draft = HighlightDraft(
            quote: quote,
            context: context,
            note: note,
            clipStartSeconds: nil,
            clipEndSeconds: nil,
            clipSpeaker: "",
            clipTranscriptSegmentIds: [],
            image: nil
        )
        let record = try await safeCore.publishHighlight(draft: draft, artifact: artifact)
        // Optimistically inject into the local list so the overlay appears
        // immediately; the subscription delta will reconcile shortly.
        if !highlights.contains(where: { $0.eventId == record.eventId }) {
            highlights.insert(record, at: 0)
        }
        lastPublishedHighlightId = record.eventId
        return record
    }

    // MARK: - Private

    private func installSubscription() async {
        guard subscriptionHandle == nil, let bridge = eventBridge else { return }
        do {
            let handle = try await safeCore.subscribeArticle(
                pubkeyHex: target.pubkey,
                dTag: target.dTag
            )
            subscriptionHandle = handle
            bridge.registerArticle(self, handle: handle)
        } catch {
            // Non-fatal: cold ndb path still shows the seeded article and
            // its cached highlights. Live updates will resume on the next
            // visit.
        }
    }

    /// Build the `ArtifactRecord` shape the Rust `publish_highlight` path
    /// expects. For NIP-23 articles the `highlight_tag_name` is `"a"` and
    /// the value is the article address.
    private func articleAsArtifact(_ article: ArticleRecord) -> ArtifactRecord {
        let preview = ArtifactPreview(
            id: article.identifier,
            url: "",
            title: article.title,
            author: "",
            image: article.image,
            description: article.summary,
            source: "article",
            domain: "",
            catalogId: "",
            catalogKind: "",
            podcastGuid: "",
            podcastItemGuid: "",
            podcastShowTitle: "",
            audioUrl: "",
            audioPreviewUrl: "",
            transcriptUrl: "",
            feedUrl: "",
            publishedAt: article.publishedAt.map { String($0) } ?? "",
            durationSeconds: nil,
            referenceTagName: "a",
            referenceTagValue: target.address,
            referenceKind: "30023",
            highlightTagName: "a",
            highlightTagValue: target.address,
            highlightReferenceKey: "a:\(target.address)",
            chapters: []
        )
        return ArtifactRecord(
            preview: preview,
            groupId: "",
            shareEventId: "",
            pubkey: article.pubkey,
            createdAt: article.createdAt,
            note: ""
        )
    }
}
