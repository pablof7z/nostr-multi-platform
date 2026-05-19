import Foundation
import os.log

extension Logger {
    /// Creates a `Logger` scoped to this app's bundle identifier, with the given category.
    ///
    /// Use this instead of repeating `Logger(subsystem: Bundle.main.bundleIdentifier ?? "Podcastr", category: …)`
    /// at every call site.
    ///
    /// ```swift
    /// private let logger = Logger.app("MyView")
    /// private static let logger = Logger.app("MyService")
    /// ```
    static func app(_ category: String) -> Logger {
        Logger(subsystem: subsystem, category: category)
    }

    /// Cached once. `Bundle.main.bundleIdentifier` is invariant for the
    /// lifetime of the process; the previous shape repeated the
    /// dictionary lookup against Info.plist on every `Logger.app(_:)`
    /// call, which the codebase makes from many static lets.
    private static let subsystem: String = Bundle.main.bundleIdentifier ?? "Podcastr"
}
