import Foundation

struct WhatsNewEntry: Decodable, Sendable, Identifiable, Equatable {
    let shippedAt: Date
    let lines: [String]

    var id: Date { shippedAt }

    private enum CodingKeys: String, CodingKey {
        case shippedAt = "shipped_at"
        case lines
    }
}

private struct WhatsNewPayload: Decodable {
    let schemaVersion: Int
    let entries: [WhatsNewEntry]

    private enum CodingKeys: String, CodingKey {
        case schemaVersion = "schema_version"
        case entries
    }
}

// Loads `whats-new.json` from the bundle and tracks a "last seen" timestamp
// in UserDefaults so entries are only surfaced once.
//
// First-install semantics: `seedIfNeeded()` silently marks the newest entry
// as seen, so brand-new installs never dump the full changelog at first launch.
@MainActor
enum WhatsNewService {

    static let lastSeenAtKey = "whatsNew.lastSeenAt"

    private static let resourceName = "whats-new"
    private static let resourceExtension = "json"

    // MARK: Loading

    /// Fail-closed: any error returns `[]` so launch never crashes.
    static func loadEntries(bundle: Bundle = .main) -> [WhatsNewEntry] {
        guard let url = bundle.url(forResource: resourceName, withExtension: resourceExtension) else {
            return []
        }
        do {
            let data = try Data(contentsOf: url)
            return try decode(data)
        } catch {
            return []
        }
    }

    /// Exposed (not private) so unit tests can call it with a JSON literal.
    static func decode(_ data: Data) throws -> [WhatsNewEntry] {
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .iso8601
        let payload = try decoder.decode(WhatsNewPayload.self, from: data)
        return payload.entries
    }

    // MARK: Marker

    static var lastSeenAt: Date? {
        guard let s = UserDefaults.standard.string(forKey: lastSeenAtKey), !s.isEmpty else {
            return nil
        }
        return iso8601.date(from: s)
    }

    static func markSeen(at date: Date) {
        UserDefaults.standard.set(iso8601.string(from: date), forKey: lastSeenAtKey)
    }

    /// On a fresh install (no marker yet), seeds the marker to the newest
    /// entry so the user doesn't see the full changelog on first launch.
    /// Idempotent once any marker is present.
    static func seedIfNeeded(entries: [WhatsNewEntry]? = nil) {
        guard UserDefaults.standard.string(forKey: lastSeenAtKey) == nil else { return }
        let sorted = (entries ?? loadEntries()).sorted { $0.shippedAt > $1.shippedAt }
        if let newest = sorted.first {
            markSeen(at: newest.shippedAt)
        }
    }

    // MARK: Diff

    /// Returns entries strictly newer than `lastSeenAt`, newest-first.
    /// Returns `[]` when marker is nil — `seedIfNeeded` handles first-install seeding.
    static func unseenEntries(
        lastSeenAt: Date?,
        entries: [WhatsNewEntry]? = nil
    ) -> [WhatsNewEntry] {
        guard let marker = lastSeenAt else { return [] }
        let all = entries ?? loadEntries()
        return all
            .filter { $0.shippedAt > marker }
            .sorted { $0.shippedAt > $1.shippedAt }
    }

    // MARK: Helpers

    private static let iso8601: ISO8601DateFormatter = {
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime]
        return f
    }()
}
