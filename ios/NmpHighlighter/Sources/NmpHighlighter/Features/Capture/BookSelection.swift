import Foundation

/// What the user has chosen to highlight from. Either an artifact that's
/// already been shared (kind:11 exists on relay), or a preview we'll publish
/// on their behalf the moment they hit Publish.
///
/// Carrying both cases through the store keeps the publish path unified: the
/// picker never has to decide "have I shared this yet?" — it just hands the
/// store a `BookSelection` and the store resolves the kind:11 side at
/// publish time.
enum BookSelection: Equatable {
    case existing(ArtifactRecord)
    case pending(ArtifactPreview)

    var title: String {
        switch self {
        case .existing(let record): return record.preview.title
        case .pending(let preview): return preview.title
        }
    }

    var author: String {
        switch self {
        case .existing(let record): return record.preview.author
        case .pending(let preview): return preview.author
        }
    }

    var coverURL: String {
        switch self {
        case .existing(let record): return record.preview.image
        case .pending(let preview): return preview.image
        }
    }

    var catalogId: String {
        switch self {
        case .existing(let record): return record.preview.catalogId
        case .pending(let preview): return preview.catalogId
        }
    }

    /// Stable key for dedup across `existing` and `pending` — two selections
    /// that point at the same ISBN should compare equal by this key even if
    /// one has a published share and the other doesn't yet.
    var referenceKey: String {
        switch self {
        case .existing(let record): return record.preview.highlightReferenceKey
        case .pending(let preview): return preview.highlightReferenceKey
        }
    }
}
