import SwiftUI
import UIKit
import UserNotifications
import os.log

// MARK: - NotificationSettingsView
//
// Settings → Notifications. Two surfaces:
//   1. System authorization status with a deep-link to iOS Settings when denied.
//   2. Per-kind toggles persisted to `Settings` so the user can opt in/out
//      independently for new-episode alerts and briefing-ready alerts.
//
// Per-show notification toggles live on each `PodcastSubscription` and are
// surfaced in `SubscriptionsListView`. The toggles here gate the *kind* of
// notification, not the per-show targeting.

struct NotificationSettingsView: View {
    @Environment(\.openURL) private var openURL
    @Environment(AppStateStore.self) private var store

    nonisolated private static let logger = Logger.app("NotificationSettingsView")

    @State private var authStatus: UNAuthorizationStatus = .notDetermined
    @State private var isLoading = true

    var body: some View {
        List {
            permissionSection
            kindSection
        }
        .settingsListStyle()
        .navigationTitle("Notifications")
        .navigationBarTitleDisplayMode(.inline)
        .task { await refreshAuthStatus() }
        // The "Open Settings" button hands off to iOS Settings; when
        // the user flips the permission and switches back, refresh
        // so the banner reflects the new state instead of caching
        // "Denied" until the view is fully re-entered.
        .onReceive(NotificationCenter.default.publisher(
            for: UIApplication.willEnterForegroundNotification
        )) { _ in
            Task { await refreshAuthStatus() }
        }
    }

    // MARK: - Sections

    private var permissionSection: some View {
        Section {
            HStack {
                Image(systemName: authStatus.iconName)
                    .foregroundStyle(authStatus.tint)
                VStack(alignment: .leading, spacing: 2) {
                    Text(authStatus.title)
                        .font(.body)
                    Text(authStatus.subtitle)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                Spacer()
                if authStatus == .notDetermined {
                    Button("Allow") {
                        Task {
                            _ = await NotificationService.requestAuthorization()
                            await refreshAuthStatus()
                        }
                    }
                    .buttonStyle(.bordered)
                } else if authStatus == .denied,
                          let url = URL(string: UIApplication.openSettingsURLString) {
                    Button("Open Settings") { openURL(url) }
                        .buttonStyle(.bordered)
                }
            }
        } header: {
            Text("Permission")
        } footer: {
            Text("iOS controls whether the app can deliver notifications. Toggles below filter by category once permission is granted.")
        }
    }

    private var kindSection: some View {
        Section {
            Toggle(isOn: newEpisodesBinding) {
                Label("New episode alerts", systemImage: "antenna.radiowaves.left.and.right")
            }
            Toggle(isOn: briefingReadyBinding) {
                Label("Briefing-ready alerts", systemImage: "sparkles.tv.fill")
            }
        } header: {
            Text("Categories")
        } footer: {
            Text("New-episode alerts also respect each show's individual notification toggle (see Subscriptions). Briefing alerts fire when a new daily / weekly briefing is ready to play.")
        }
        .disabled(!isAuthorized)
        .opacity(isAuthorized ? 1.0 : 0.6)
    }

    // MARK: - Bindings

    private var newEpisodesBinding: Binding<Bool> {
        Binding(
            get: { store.state.settings.notifyOnNewEpisodes },
            set: { v in
                var s = store.state.settings
                s.notifyOnNewEpisodes = v
                store.updateSettings(s)
                Haptics.selection()
            }
        )
    }

    private var briefingReadyBinding: Binding<Bool> {
        Binding(
            get: { store.state.settings.notifyOnBriefingReady },
            set: { v in
                var s = store.state.settings
                s.notifyOnBriefingReady = v
                store.updateSettings(s)
                Haptics.selection()
            }
        )
    }

    // MARK: - Derived

    private var isAuthorized: Bool {
        switch authStatus {
        case .authorized, .provisional, .ephemeral: return true
        default: return false
        }
    }

    // MARK: - Loading

    private func refreshAuthStatus() async {
        let settings = await UNUserNotificationCenter.current().notificationSettings()
        authStatus = settings.authorizationStatus
        isLoading = false
    }
}

// MARK: - Auth status presentation

private extension UNAuthorizationStatus {
    var iconName: String {
        switch self {
        case .authorized, .provisional, .ephemeral: "bell.fill"
        case .denied: "bell.slash.fill"
        case .notDetermined: "bell"
        @unknown default: "bell"
        }
    }

    var tint: Color {
        switch self {
        case .authorized, .provisional, .ephemeral: .green
        case .denied: .red
        case .notDetermined: .secondary
        @unknown default: .secondary
        }
    }

    var title: String {
        switch self {
        case .authorized: "Allowed"
        case .provisional: "Allowed (Provisional)"
        case .ephemeral: "Allowed (Ephemeral)"
        case .denied: "Denied"
        case .notDetermined: "Not yet asked"
        @unknown default: "Unknown"
        }
    }

    var subtitle: String {
        switch self {
        case .authorized: "Notifications are enabled for this app."
        case .provisional: "Notifications are delivered quietly."
        case .ephemeral: "Notifications are temporary."
        case .denied: "Enable in iOS Settings to receive new-episode and briefing alerts."
        case .notDetermined: "We'll ask the first time something needs to notify you."
        @unknown default: ""
        }
    }
}
