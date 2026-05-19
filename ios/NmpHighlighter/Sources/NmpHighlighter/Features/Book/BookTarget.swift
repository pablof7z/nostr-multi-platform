import Foundation

/// Navigation value for pushing BookView. Carries just the catalog ID
/// (e.g. "isbn:9780593716717") so the view can load its own preview.
/// Equality and hashing are by catalogId alone.
struct BookTarget: Hashable {
    let catalogId: String
}
