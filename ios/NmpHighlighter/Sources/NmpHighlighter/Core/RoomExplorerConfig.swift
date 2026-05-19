import Foundation

/// Configuration for the rooms explorer's curated shelves. The curator
/// pubkey signs NIP-51 kind:10012 lists that drive the "Featured" row; in
/// production this is `relay.highlighter.com`'s own pubkey, so the relay
/// admin UI ("croissant") is the single source of editorial curation.
///
/// The pubkey is discovered at runtime from the relay's NIP-11 document
/// the first time the explorer appears, then cached in `UserDefaults` so
/// later appearances don't re-fetch. An empty cached value means the
/// document hasn't been retrieved yet — the featured shelf will be empty
/// in that case and populates on the next appear.
enum RoomExplorerConfig {
    static let curatorRelayURL = URL(string: "https://relay.highlighter.com")!

    /// Hardcoded fallback — relay.highlighter.com's stable pubkey.
    /// Used immediately on first install so the featured shelf never
    /// blocks on a NIP-11 network round-trip.
    static let defaultCuratorPubkeyHex = "7e1eabe25256545cfe0c534a99bfa5c6cd224e04b614182a9993feff54196c95"

    private static let cachedCuratorKey = "highlighter.explorer.curatorPubkeyHex"

    /// The curator pubkey. Falls back to the hardcoded default when no
    /// UserDefaults value has been persisted yet.
    static var cachedCuratorPubkeyHex: String {
        get {
            let stored = UserDefaults.standard.string(forKey: cachedCuratorKey) ?? ""
            return stored.isEmpty ? defaultCuratorPubkeyHex : stored
        }
        set { UserDefaults.standard.set(newValue, forKey: cachedCuratorKey) }
    }

    /// Fetch the curator relay's NIP-11 info document and return its pubkey.
    /// Caches the result in `UserDefaults` on success. Returns `nil` on any
    /// failure (network, malformed JSON, missing pubkey field); callers
    /// should treat that as "featured shelf unavailable this session".
    static func fetchCuratorPubkey() async -> String? {
        var request = URLRequest(url: curatorRelayURL)
        request.setValue("application/nostr+json", forHTTPHeaderField: "Accept")
        do {
            let (data, _) = try await URLSession.shared.data(for: request)
            guard
                let object = try JSONSerialization.jsonObject(with: data) as? [String: Any],
                let pubkey = object["pubkey"] as? String,
                !pubkey.isEmpty
            else {
                return nil
            }
            cachedCuratorPubkeyHex = pubkey
            return pubkey
        } catch {
            return nil
        }
    }
}
