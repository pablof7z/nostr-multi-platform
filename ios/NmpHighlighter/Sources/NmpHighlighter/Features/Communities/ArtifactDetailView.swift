import SwiftUI

/// Dispatch view for an artifact row. Routes by `preview.source`:
/// - `podcast` → pushes `PodcastListeningView`, which loads the artifact
///   into the global player on appear. The MiniPlayer accessory still
///   surfaces (mounted on `MainTabView`); back chevron returns to the room.
/// - `article` → NIP-23 reader, built from the artifact's `a`-tag reference
///   (`30023:<pubkey>:<d>`).
/// - everything else → "Coming soon" placeholder.
struct ArtifactDetailView: View {
    let artifact: ArtifactRecord

    @Environment(HighlighterStore.self) private var app

    var body: some View {
        Group {
            switch artifact.preview.source {
            case "podcast":
                PodcastListeningView(presentation: .pushed, artifact: artifact)
            case "article":
                if let target = articleTarget {
                    ArticleReaderView(target: target)
                } else {
                    ContentUnavailableView(
                        "Missing article reference",
                        systemImage: "doc.text",
                        description: Text("This share doesn't carry a valid NIP-23 address.")
                    )
                }
            case "book":
                let catalogId = artifact.preview.catalogId.isEmpty
                    ? artifact.preview.highlightTagValue
                    : artifact.preview.catalogId
                BookView(catalogId: catalogId)
                    .environment(app)
            default:
                ContentUnavailableView(
                    "Coming soon",
                    systemImage: "doc.text",
                    description: Text("This artifact type doesn't have a dedicated view yet.")
                )
            }
        }
        .navigationTitle(artifact.preview.title.isEmpty ? "Artifact" : artifact.preview.title)
        .navigationBarTitleDisplayMode(.inline)
    }

    /// Parse the NIP-33 `a`-tag (`30023:<pubkey>:<d>`) out of the artifact's
    /// highlight reference. Falls back to the generic reference fields if
    /// the artifact wasn't explicitly tagged for highlights.
    private var articleTarget: ArticleReaderTarget? {
        let raw: String
        if artifact.preview.highlightTagName == "a", !artifact.preview.highlightTagValue.isEmpty {
            raw = artifact.preview.highlightTagValue
        } else if artifact.preview.referenceTagName == "a", !artifact.preview.referenceTagValue.isEmpty {
            raw = artifact.preview.referenceTagValue
        } else {
            return nil
        }
        let parts = raw.split(separator: ":", maxSplits: 2, omittingEmptySubsequences: false)
        guard parts.count == 3, parts[0] == "30023" else { return nil }
        let pubkey = String(parts[1])
        let dTag = String(parts[2])
        guard !pubkey.isEmpty, !dTag.isEmpty else { return nil }
        return ArticleReaderTarget(pubkey: pubkey, dTag: dTag, seed: nil)
    }
}
