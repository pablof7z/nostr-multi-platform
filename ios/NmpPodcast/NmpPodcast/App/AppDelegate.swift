import UIKit
import UserNotifications
import os.log

// MARK: - App Delegate

/// Handles UIKit lifecycle events that pure SwiftUI cannot receive:
/// - App-icon quick-action (home-screen shortcut) selection.
/// - Foreground notification presentation.
///
/// Wired in via `@UIApplicationDelegateAdaptor` in `AppMain`.
final class AppDelegate: NSObject, UIApplicationDelegate {
    private let logger = Logger.app("AppDelegate")

    // MARK: - Pending shortcut

    /// Shortcut selected while the app was not running (cold-launch path).
    /// `RootView` reads this on `.onAppear` and clears it after routing.
    var pendingShortcutURL: URL?

    // MARK: - UIApplicationDelegate

    func application(
        _ application: UIApplication,
        didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]?
    ) -> Bool {
        UNUserNotificationCenter.current().delegate = self
        // Bound Kingfisher's memory + disk caches so artwork doesn't grow
        // unchecked. See KingfisherConfiguration for the rationale.
        KingfisherConfiguration.configure()
        // Cold-launch path: UIKit hands the shortcut here. `RootView` reads
        // `pendingShortcutURL` on `.onAppear` and routes via DeepLinkHandler.
        if let shortcut = launchOptions?[.shortcutItem] as? UIApplicationShortcutItem,
           let url = Self.deepLinkURL(for: shortcut) {
            pendingShortcutURL = url
        }
        return true
    }

    /// Warm-app path: invoked when the user picks a quick action while the
    /// app is already running (foreground or backgrounded). Posts the URL
    /// so RootView's onReceive routes it on the main run loop.
    func application(
        _ application: UIApplication,
        performActionFor shortcutItem: UIApplicationShortcutItem,
        completionHandler: @escaping (Bool) -> Void
    ) {
        guard let url = Self.deepLinkURL(for: shortcutItem) else {
            logger.warning("Unhandled quick action: \(shortcutItem.type, privacy: .public)")
            completionHandler(false)
            return
        }
        NotificationCenter.default.post(name: Self.shortcutURLNotification, object: url)
        completionHandler(true)
    }

    func application(
        _ application: UIApplication,
        handleEventsForBackgroundURLSession identifier: String,
        completionHandler: @escaping () -> Void
    ) {
        Task { @MainActor in
            EpisodeDownloadService.shared.handleEventsForBackgroundURLSession(
                identifier: identifier,
                completionHandler: completionHandler
            )
        }
    }

    /// Maps an `UIApplicationShortcutItem.type` to a `podcastr://` deep-link
    /// the rest of the app already knows how to route via `DeepLinkHandler`.
    /// The bundle-id prefix is stripped so the suffix alone identifies the
    /// destination — keeps this in sync with whatever bundle ID `Project.swift`
    /// resolves to today.
    private static func deepLinkURL(for shortcut: UIApplicationShortcutItem) -> URL? {
        let bundleID = Bundle.main.bundleIdentifier ?? ""
        let prefix = bundleID + "."
        let suffix = shortcut.type.hasPrefix(prefix)
            ? String(shortcut.type.dropFirst(prefix.count))
            : shortcut.type
        switch suffix {
        case "open-agent": return URL(string: "podcastr://agent")
        case "feedback":   return URL(string: "podcastr://feedback")
        case "settings":   return URL(string: "podcastr://settings")
        default:           return nil
        }
    }
}

// MARK: - UNUserNotificationCenterDelegate

extension AppDelegate: UNUserNotificationCenterDelegate {

    /// Shows banners even when the app is in the foreground (e.g. during testing).
    nonisolated func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        willPresent notification: UNNotification,
        withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void
    ) {
        completionHandler([.banner, .sound, .badge])
    }

    /// Routes notification taps. Only new-episode notifications carry an
    /// `episodeID` payload — for those we synthesize a `podcastr://episode/<uuid>`
    /// deep-link and post it through `shortcutURLNotification`, which `RootView`
    /// already observes and routes via `handleDeepLink(_:)`.
    nonisolated func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        didReceive response: UNNotificationResponse,
        withCompletionHandler completionHandler: @escaping () -> Void
    ) {
        defer { completionHandler() }
        let userInfo = response.notification.request.content.userInfo
        guard let episodeID = userInfo[NotificationService.episodeIDUserInfoKey] as? String,
              UUID(uuidString: episodeID) != nil,
              let url = URL(string: "podcastr://episode/\(episodeID)")
        else { return }
        // Hop onto the main actor to post — RootView listens on the main queue.
        Task { @MainActor in
            NotificationCenter.default.post(
                name: AppDelegate.shortcutURLNotification,
                object: url
            )
        }
    }
}

// MARK: - Notification names

extension AppDelegate {
    /// Posted when a quick-action URL is ready to route.
    static let shortcutURLNotification = Notification.Name("AppDelegate.shortcutURL")
}
