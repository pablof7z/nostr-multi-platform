import Foundation

/// UserDefaults-backed ring buffer of the last N search queries. Newest first.
/// Kept as a standalone type (not part of `SearchStore`) so other surfaces —
/// e.g. a future spotlight — can read/write the same list without importing
/// the store.
enum RecentSearches {
    private static let key = "com.highlighter.recentSearches.v1"
    private static let maxCount = 8

    static func all() -> [String] {
        let defaults = UserDefaults.standard
        guard let list = defaults.stringArray(forKey: key) else { return [] }
        return list
    }

    static func record(_ query: String) {
        let trimmed = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }
        var current = all().filter { $0.lowercased() != trimmed.lowercased() }
        current.insert(trimmed, at: 0)
        if current.count > maxCount {
            current = Array(current.prefix(maxCount))
        }
        UserDefaults.standard.set(current, forKey: key)
    }

    static func clear() {
        UserDefaults.standard.removeObject(forKey: key)
    }
}
