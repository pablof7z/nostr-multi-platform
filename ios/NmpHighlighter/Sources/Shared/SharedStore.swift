import Foundation

/// App Group glue shared between the main app and the Share Extension.
/// Both targets compile this file; they talk to each other through the
/// App Group's `UserDefaults` suite and nothing else.
///
/// Design: the extension process is intentionally tiny. It does *not* load
/// the Rust core, touch the Keychain, or talk to any relay. It writes a
/// `PendingShare` into `ShareQueue` and opens the main app via the custom
/// URL scheme; the main app drains the queue on next foreground and
/// publishes via the Rust core using whichever signer is installed.
public enum AppGroup {
    public static let id = "group.com.highlighter.app"
    public static var defaults: UserDefaults? { UserDefaults(suiteName: id) }
}

/// Snapshot of one of the user's joined communities, flat enough for the
/// extension to render a picker without pulling the Rust core in.
public struct SharedCommunitySummary: Codable, Hashable {
    public let id: String
    public let name: String
    public let picture: String

    public init(id: String, name: String, picture: String) {
        self.id = id
        self.name = name
        self.picture = picture
    }
}

/// The list of joined communities the main app last observed. The main app
/// writes on every refresh; the extension reads on launch.
public enum SharedCommunitiesCache {
    private static let key = "joinedCommunitiesV1"

    public static func load() -> [SharedCommunitySummary] {
        guard let defaults = AppGroup.defaults,
              let data = defaults.data(forKey: key) else { return [] }
        return (try? JSONDecoder().decode([SharedCommunitySummary].self, from: data)) ?? []
    }

    public static func save(_ communities: [SharedCommunitySummary]) {
        guard let defaults = AppGroup.defaults else { return }
        if let data = try? JSONEncoder().encode(communities) {
            defaults.set(data, forKey: key)
        }
    }

    public static func clear() {
        AppGroup.defaults?.removeObject(forKey: key)
    }
}

/// A share the user submitted in the extension but hasn't been published
/// yet — the main app drains this on foreground.
public struct PendingShare: Codable, Hashable, Identifiable {
    public let id: UUID
    public let groupId: String
    public let url: String
    public let note: String
    public let createdAt: Date

    public init(
        id: UUID = UUID(),
        groupId: String,
        url: String,
        note: String,
        createdAt: Date = Date()
    ) {
        self.id = id
        self.groupId = groupId
        self.url = url
        self.note = note
        self.createdAt = createdAt
    }
}

public enum ShareQueue {
    private static let key = "pendingSharesV1"

    public static func enqueue(_ share: PendingShare) {
        var current = load()
        current.append(share)
        save(current)
    }

    public static func load() -> [PendingShare] {
        guard let defaults = AppGroup.defaults,
              let data = defaults.data(forKey: key) else { return [] }
        return (try? JSONDecoder().decode([PendingShare].self, from: data)) ?? []
    }

    public static func drain() -> [PendingShare] {
        let items = load()
        AppGroup.defaults?.removeObject(forKey: key)
        return items
    }

    public static func replace(_ items: [PendingShare]) {
        save(items)
    }

    private static func save(_ items: [PendingShare]) {
        guard let defaults = AppGroup.defaults else { return }
        if let data = try? JSONEncoder().encode(items) {
            defaults.set(data, forKey: key)
        }
    }
}

/// URL used by the extension to hand off control to the main app once a
/// share is enqueued. The main app's `.onOpenURL` handler recognizes this
/// and kicks off queue processing.
public enum ShareURLScheme {
    public static let scheme = "highlighter"
    public static let processShareHost = "process-share"

    public static var processShareURL: URL? {
        URL(string: "\(scheme)://\(processShareHost)")
    }

    public static func isProcessShare(_ url: URL) -> Bool {
        url.scheme == scheme && url.host == processShareHost
    }
}
