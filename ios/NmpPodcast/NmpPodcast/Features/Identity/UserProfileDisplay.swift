import Foundation

// MARK: - UserProfileDisplay
//
// Represents the user's kind-0 profile for display purposes.
// `from(identity:)` prefers stored kind-0 fields fetched from relays and
// falls back to the deterministic generated profile when none are available.

struct UserProfileDisplay {

    let displayName: String
    let slug: String
    let about: String
    let pictureURLString: String

    /// Derive a generated profile purely from the pubkey.
    init(publicKeyHex: String) {
        let seed = String(publicKeyHex.prefix(16))
        let index = Self.stableIndex(seed)
        let adjectives = ["Bright", "Quiet", "Swift", "Kind", "Clear", "North"]
        let nouns = ["Signal", "Notebook", "Harbor", "Lantern", "Thread", "Field"]
        let adjective = adjectives[index % adjectives.count]
        let noun = nouns[(index / adjectives.count) % nouns.count]
        self.displayName = "\(adjective) \(noun)"
        self.slug = "\(adjective.lowercased())-\(noun.lowercased())-\(publicKeyHex.prefix(4))"
        self.about = ""
        self.pictureURLString = "https://api.dicebear.com/9.x/personas/svg?seed=\(seed)"
    }

    init(displayName: String, slug: String, about: String, pictureURLString: String) {
        self.displayName = displayName
        self.slug = slug
        self.about = about
        self.pictureURLString = pictureURLString
    }

    /// Prefers real kind-0 fields from the identity store; falls back to
    /// the deterministic generated profile when the fetch hasn't completed.
    @MainActor
    static func from(identity: UserIdentityStore) -> UserProfileDisplay? {
        guard let hex = identity.publicKeyHex, !hex.isEmpty else { return nil }
        let generated = UserProfileDisplay(publicKeyHex: hex)
        guard identity.profileDisplayName != nil
           || identity.profileName != nil
           || identity.profileAbout != nil
           || identity.profilePicture != nil
        else { return generated }
        return UserProfileDisplay(
            displayName:     identity.profileDisplayName ?? generated.displayName,
            slug:            identity.profileName        ?? generated.slug,
            about:           identity.profileAbout       ?? generated.about,
            pictureURLString: identity.profilePicture    ?? generated.pictureURLString
        )
    }

    /// Convenience: returns `nil` when no identity exists yet.
    static func from(publicKeyHex: String?) -> UserProfileDisplay? {
        guard let hex = publicKeyHex, !hex.isEmpty else { return nil }
        return UserProfileDisplay(publicKeyHex: hex)
    }

    var pictureURL: URL? {
        guard let s = pictureURLString.isEmpty ? nil : pictureURLString,
              let url = URL(string: s),
              let scheme = url.scheme?.lowercased(),
              scheme == "http" || scheme == "https" else { return nil }
        return url
    }

    // MARK: - Hash helper

    /// Mirrors `UserIdentityStore.stableProfileIndex` so the picked
    /// adjective/noun pair stays stable between store and UI.
    private static func stableIndex(_ seed: String) -> Int {
        seed.utf8.reduce(0) { partial, byte in
            (partial &* 31 &+ Int(byte)) & 0x7fffffff
        }
    }
}

// MARK: - DicebearStyle (curated 6, per identity-05-synthesis §4.4)

/// The six curated dicebear styles offered to the user. Order matches the
/// horizontal rail in §4.4: personas, notionists, lorelei, shapes, glass,
/// identicon. Selected style is currently held only in local UI state — Slice B
/// will persist the user's pick into the kind-0 picture URL.
enum DicebearStyle: String, CaseIterable, Identifiable {
    case personas
    case notionists
    case lorelei
    case shapes
    case glass
    case identicon

    var id: String { rawValue }

    var displayName: String {
        switch self {
        case .personas:   "Personas"
        case .notionists: "Notionist"
        case .lorelei:    "Lorelei"
        case .shapes:     "Shapes"
        case .glass:      "Glass"
        case .identicon:  "Identicon"
        }
    }

    /// Builds the canonical dicebear URL for a pubkey-derived seed.
    func url(seed: String) -> URL? {
        URL(string: "https://api.dicebear.com/9.x/\(rawValue)/svg?seed=\(seed)")
    }
}
