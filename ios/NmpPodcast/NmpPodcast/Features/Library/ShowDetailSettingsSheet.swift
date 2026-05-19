import SwiftUI

// MARK: - ShowDetailSettingsSheet

/// "Settings for this show" sheet. Real toggles for notifications, the
/// per-show auto-download policy, and a destructive unsubscribe action.
struct ShowDetailSettingsSheet: View {
    let podcast: Podcast
    let store: AppStateStore
    let onDismiss: () -> Void
    let onUnsubscribe: () -> Void

    @State private var notificationsEnabled: Bool
    @State private var autoDownloadChoice: AutoDownloadChoice
    @State private var latestNCount: Int
    @State private var wifiOnly: Bool

    /// Picker-friendly enum that flattens `AutoDownloadPolicy.Mode`'s
    /// associated value into a stepper-driven count. `latestN` covers the
    /// "keep the most recent N" case; the count is held in a separate
    /// `@State` so the picker selection stays clean and the stepper is only
    /// shown when the user picks `latestN`.
    enum AutoDownloadChoice: String, CaseIterable, Identifiable {
        case off
        case latestN
        case allNew

        var id: String { rawValue }

        var label: String {
            switch self {
            case .off:     return "Off"
            case .latestN: return "Latest"
            case .allNew:  return "All new"
            }
        }
    }

    init(
        podcast: Podcast,
        store: AppStateStore,
        onDismiss: @escaping () -> Void,
        onUnsubscribe: @escaping () -> Void
    ) {
        self.podcast = podcast
        self.store = store
        self.onDismiss = onDismiss
        self.onUnsubscribe = onUnsubscribe
        // Hydrate from the live subscription row when the user follows
        // this podcast; otherwise fall back to defaults (the sheet still
        // renders for read-only inspection of the feed metadata).
        let subscription = store.subscription(podcastID: podcast.id)
        _notificationsEnabled = State(initialValue: subscription?.notificationsEnabled ?? true)
        let policy = subscription?.autoDownload ?? .default
        switch policy.mode {
        case .off:
            _autoDownloadChoice = State(initialValue: .off)
            _latestNCount = State(initialValue: 5)
        case .latestN(let n):
            _autoDownloadChoice = State(initialValue: .latestN)
            _latestNCount = State(initialValue: n)
        case .allNew:
            _autoDownloadChoice = State(initialValue: .allNew)
            _latestNCount = State(initialValue: 5)
        }
        _wifiOnly = State(initialValue: policy.wifiOnly)
    }

    var body: some View {
        NavigationStack {
            Form {
                Section("Notifications") {
                    Toggle("Notify me when new episodes drop", isOn: $notificationsEnabled)
                        .onChange(of: notificationsEnabled) { _, newValue in
                            store.setSubscriptionNotificationsEnabled(
                                podcast.id,
                                enabled: newValue
                            )
                        }
                }
                Section("Auto-download") {
                    LiquidGlassSegmentedPicker(
                        "New episodes",
                        selection: $autoDownloadChoice,
                        segments: AutoDownloadChoice.allCases.map { ($0, $0.label) }
                    )
                    .listRowBackground(Color.clear)
                    .listRowInsets(AppTheme.Layout.cardRowInsetsSM)
                    .onChange(of: autoDownloadChoice) { _, _ in persistPolicy() }

                    if autoDownloadChoice == .latestN {
                        // `onEditingChanged` only fires on press-and-hold to
                        // auto-repeat — single taps on + / - update the
                        // binding but won't trip the closure. Use `.onChange`
                        // on the count instead so every value change persists.
                        Stepper(value: $latestNCount, in: 1...50) {
                            HStack {
                                Text("Keep latest")
                                Spacer()
                                Text("\(latestNCount)")
                                    .foregroundStyle(.secondary)
                                    .monospacedDigit()
                            }
                        }
                        .onChange(of: latestNCount) { _, _ in persistPolicy() }
                    }

                    if autoDownloadChoice != .off {
                        Toggle("Wi-Fi only", isOn: $wifiOnly)
                            .onChange(of: wifiOnly) { _, _ in persistPolicy() }
                    }
                }
                Section("Feed") {
                    if let feedURL = podcast.feedURL {
                        LabeledContent("URL") {
                            Text(feedURL.absoluteString)
                                .font(AppTheme.Typography.monoCaption)
                                .foregroundStyle(.secondary)
                                .lineLimit(2)
                                .multilineTextAlignment(.trailing)
                                .textSelection(.enabled)
                                .copyableTextMenu(feedURL.absoluteString)
                        }
                    }
                    if let refreshed = podcast.lastRefreshedAt {
                        LabeledContent("Last refreshed") {
                            Text(refreshed.formatted(date: .abbreviated, time: .shortened))
                                .foregroundStyle(.secondary)
                        }
                    }
                }
                Section {
                    Button(role: .destructive) {
                        Haptics.warning()
                        onDismiss()
                        onUnsubscribe()
                    } label: {
                        Label("Unsubscribe", systemImage: "xmark.circle")
                    }
                }
            }
            .navigationTitle(podcast.title)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Done") { onDismiss() }
                }
            }
        }
        .presentationBackground(.thinMaterial)
        .presentationDetents([.medium, .large])
        .presentationDragIndicator(.visible)
    }

    /// Composes the three sheet-local fields back into the canonical
    /// `AutoDownloadPolicy` and writes through the store. Called from every
    /// path that mutates one of the inputs — keeping all the round-trip
    /// logic in one place avoids drift between the picker and the stepper.
    private func persistPolicy() {
        let mode: AutoDownloadPolicy.Mode
        switch autoDownloadChoice {
        case .off:     mode = .off
        case .latestN: mode = .latestN(latestNCount)
        case .allNew:  mode = .allNew
        }
        store.setSubscriptionAutoDownload(
            podcast.id,
            policy: AutoDownloadPolicy(mode: mode, wifiOnly: wifiOnly)
        )
    }
}
