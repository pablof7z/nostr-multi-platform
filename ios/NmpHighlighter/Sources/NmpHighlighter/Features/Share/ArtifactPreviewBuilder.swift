import Foundation

/// Builders for `ArtifactPreview` — the payload `publishArtifact` takes to
/// post into a room. These exist so the reads feed, room library, and article
/// reader can all feed the same share sheet without duplicating the struct
/// initialization.
enum ArtifactPreviewBuilder {
    /// Build a NIP-23 article preview from an `ArticleRecord`. The highlight
    /// + reference tag point at the article's `a`-address
    /// (`30023:<pubkey>:<d>`) so downstream consumers resolve it correctly.
    static func from(article: ArticleRecord) -> ArtifactPreview {
        let address = "30023:\(article.pubkey):\(article.identifier)"
        return ArtifactPreview(
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
            referenceTagValue: address,
            referenceKind: "30023",
            highlightTagName: "a",
            highlightTagValue: address,
            highlightReferenceKey: "a:\(address)",
            chapters: []
        )
    }

    /// Re-share an existing artifact — we already have a hydrated
    /// `ArtifactPreview`, so just thread it through.
    static func from(artifact: ArtifactRecord) -> ArtifactPreview {
        artifact.preview
    }
}
