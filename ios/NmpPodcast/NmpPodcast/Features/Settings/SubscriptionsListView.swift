import SwiftUI
import os.log

// MARK: - SubscriptionsListView
//
// Settings → Subscriptions. A management surface — distinct from the Library
// tab grid — that lets the user:
//   • see every show they're subscribed to,
//   • toggle per-show notifications,
//   • unsubscribe (with a confirmation),
//   • export the whole list as OPML via the system share sheet.
//
// Uses `store.sortedFollowedPodcasts` so order matches the Library grid.

struct SubscriptionsListView: View {
    @Environment(AppStateStore.self) private var store

    nonisolated private static let logger = Logger.app("SubscriptionsListView")

    @State private var pendingDelete: Podcast?
    /// Tmp-file URL of the latest exported OPML. Generated lazily inside
    /// `.task` (and re-generated whenever the subscription list mutates) so
    /// the `ShareLink` below has a real URL ready by the time the user
    /// reaches the export row. Kept on disk in `temporaryDirectory` so iOS
    /// can clean up after itself; small enough (~25 KB for ~50 subs) that
    /// the redundancy is harmless.
    @State private var opmlURL: URL?
    @State private var exportError: String?

    var body: some View {
        List {
            if store.sortedFollowedPodcasts.isEmpty {
                emptyStateSection
            } else {
                subscriptionsSection
            }
            exportSection
        }
        .settingsListStyle()
        .navigationTitle("Subscriptions")
        .navigationBarTitleDisplayMode(.inline)
        .alert(
            "Unsubscribe",
            isPresented: pendingDeleteBinding,
            presenting: pendingDelete
        ) { sub in
            Button("Unsubscribe", role: .destructive) {
                store.deletePodcast(podcastID: sub.id)
                Haptics.success()
                pendingDelete = nil
            }
            Button("Cancel", role: .cancel) { pendingDelete = nil }
        } message: { sub in
            Text("Remove \(sub.title) and all of its episodes from your library? This cannot be undone.")
        }
        .alert(
            "Couldn't export OPML",
            isPresented: Binding(
                get: { exportError != nil },
                set: { if !$0 { exportError = nil } }
            ),
            presenting: exportError
        ) { _ in
            Button("OK", role: .cancel) { exportError = nil }
        } message: { msg in
            Text(msg)
        }
        // Eagerly regenerate the OPML file when the screen appears (and
        // whenever the subscription list count changes). Keeps the
        // `ShareLink` in `exportSection` ready to fire without a roundtrip
        // through a SwiftUI `.sheet { ShareSheet(...) }`, which renders
        // blank on iOS 16+ when wrapping a `UIActivityViewController` in
        // a sheet container — the activity controller wants its own
        // presentation context, not a SwiftUI modal scope.
        .task(id: store.sortedFollowedPodcasts.count) {
            await regenerateOPMLIfNeeded()
        }
    }

    // MARK: - Sections

    private var emptyStateSection: some View {
        Section {
            VStack(spacing: AppTheme.Spacing.sm) {
                Image(systemName: "antenna.radiowaves.left.and.right")
                    .font(.system(size: 32, weight: .semibold))
                    .foregroundStyle(.secondary)
                Text("No subscriptions yet")
                    .font(AppTheme.Typography.headline)
                Text("Add a podcast from the Library tab to get started.")
                    .font(AppTheme.Typography.caption)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
            }
            .frame(maxWidth: .infinity)
            .padding(.vertical, AppTheme.Spacing.md)
        }
    }

    private var subscriptionsSection: some View {
        Section {
            ForEach(store.sortedFollowedPodcasts) { sub in
                row(for: sub)
            }
        } header: {
            Text("\(store.sortedFollowedPodcasts.count) show\(store.sortedFollowedPodcasts.count == 1 ? "" : "s")")
        }
    }

    private var exportSection: some View {
        Section {
            // Two-state row: ShareLink once the OPML file is ready, plain
            // disabled row while it's still being generated (rare — the
            // file lands in single-digit ms for typical libraries). Using
            // `ShareLink` directly instead of the previous
            // `Button + .sheet { ShareSheet(...) }` pattern because the
            // SwiftUI sheet wrapper renders the underlying
            // UIActivityViewController as a blank white sheet on iOS 16+.
            if let opmlURL, !store.sortedFollowedPodcasts.isEmpty {
                ShareLink(
                    item: opmlURL,
                    subject: Text("Podcastr Subscriptions"),
                    preview: SharePreview(
                        "Podcastr Subscriptions (\(store.sortedFollowedPodcasts.count) shows)",
                        image: Image(systemName: "list.bullet.rectangle")
                    )
                ) {
                    SettingsRow(
                        icon: "square.and.arrow.up",
                        tint: .teal,
                        title: "Export OPML",
                        subtitle: "Share with another podcast app"
                    )
                }
                .buttonStyle(.pressable)
            } else {
                SettingsRow(
                    icon: "square.and.arrow.up",
                    tint: .teal,
                    title: "Export OPML",
                    subtitle: store.sortedFollowedPodcasts.isEmpty
                        ? "No subscriptions to export"
                        : "Preparing export…"
                )
                .opacity(0.5)
            }
        } footer: {
            Text("Exports all subscribed feed URLs as a standard OPML 2.0 document.")
        }
    }

    // MARK: - Row

    @ViewBuilder
    private func row(for sub: Podcast) -> some View {
        VStack(spacing: AppTheme.Spacing.xs) {
            HStack(spacing: AppTheme.Spacing.sm) {
                artwork(for: sub)
                VStack(alignment: .leading, spacing: 2) {
                    Text(sub.title.isEmpty ? (sub.feedURL?.host ?? "Untitled") : sub.title)
                        .font(AppTheme.Typography.body)
                        .lineLimit(1)
                    if !sub.author.isEmpty {
                        Text(sub.author)
                            .font(AppTheme.Typography.caption)
                            .foregroundStyle(.secondary)
                            .lineLimit(1)
                    }
                    // Status row: episode count + auto-download mode (when
                    // not Off). Surfaces here so a user managing many subs
                    // can see at a glance which feeds are pulling bytes
                    // automatically without diving into per-show settings.
                    statusRow(for: sub)
                }
                Spacer(minLength: 0)
            }

            HStack {
                Toggle(isOn: notificationsBinding(for: sub)) {
                    Label("Episode alerts", systemImage: "bell.fill")
                        .font(AppTheme.Typography.caption)
                        .labelStyle(.titleAndIcon)
                }
                .toggleStyle(.switch)
                .controlSize(.mini)
            }
        }
        .swipeActions(edge: .trailing, allowsFullSwipe: false) {
            Button(role: .destructive) {
                pendingDelete = sub
            } label: {
                Label("Unsubscribe", systemImage: "trash")
            }
        }
    }

    private func statusRow(for sub: Podcast) -> some View {
        let count = store.episodes(forPodcast: sub.id).count
        let countLabel = count == 1 ? "1 episode" : "\(count) episodes"
        let autoDownloadLabel = store.subscription(podcastID: sub.id)?.autoDownload.summaryLabel
        return HStack(spacing: 6) {
            Text(countLabel)
                .font(AppTheme.Typography.caption2)
                .foregroundStyle(.secondary)
                .monospacedDigit()
            if let policy = autoDownloadLabel {
                Text("·")
                    .font(AppTheme.Typography.caption2)
                    .foregroundStyle(.tertiary)
                Label(policy, systemImage: "arrow.down.circle")
                    .labelStyle(StatusBadgeLabelStyle())
                    .font(AppTheme.Typography.caption2)
                    .foregroundStyle(.tint)
            }
        }
    }

    private func artwork(for sub: Podcast) -> some View {
        Group {
            if let url = sub.imageURL {
                CachedAsyncImage(url: url) { phase in
                    switch phase {
                    case .success(let image):
                        image.resizable().scaledToFill()
                    default:
                        artworkPlaceholder
                    }
                }
            } else {
                artworkPlaceholder
            }
        }
        .frame(width: 36, height: 36)
        .clipShape(RoundedRectangle(cornerRadius: 6, style: .continuous))
    }

    private var artworkPlaceholder: some View {
        ZStack {
            RoundedRectangle(cornerRadius: 6, style: .continuous)
                .fill(Color.secondary.opacity(0.2))
            Image(systemName: "headphones")
                .foregroundStyle(.secondary)
        }
    }

    // MARK: - Bindings

    private var pendingDeleteBinding: Binding<Bool> {
        Binding(
            get: { pendingDelete != nil },
            set: { if !$0 { pendingDelete = nil } }
        )
    }

    private func notificationsBinding(for sub: Podcast) -> Binding<Bool> {
        Binding(
            get: { store.subscription(podcastID: sub.id)?.notificationsEnabled ?? true },
            set: { store.setSubscriptionNotificationsEnabled(sub.id, enabled: $0) }
        )
    }

    // MARK: - OPML export

    /// Generates a fresh OPML file and stores its URL in `opmlURL` so the
    /// `ShareLink` lights up. Skips the work when the subscription list is
    /// empty (the export row stays disabled in that case). Errors land in
    /// `exportError` and surface via the `.alert` modifier.
    private func regenerateOPMLIfNeeded() async {
        guard !store.sortedFollowedPodcasts.isEmpty else {
            opmlURL = nil
            return
        }
        let subs = store.sortedFollowedPodcasts
        let exporter = OPMLExport()
        let data = exporter.exportOPML(podcasts: subs)
        let filename = "Podcastr-Subscriptions-\(Self.dateStamp()).opml"
        let url = FileManager.default.temporaryDirectory.appendingPathComponent(filename)
        do {
            try data.write(to: url, options: [.atomic])
            opmlURL = url
        } catch {
            Self.logger.error("OPML export write failed: \(error, privacy: .public)")
            exportError = error.localizedDescription
            Haptics.error()
        }
    }

    private static func dateStamp() -> String {
        let f = DateFormatter()
        f.dateFormat = "yyyy-MM-dd-HHmm"
        f.locale = Locale(identifier: "en_US_POSIX")
        f.timeZone = TimeZone(identifier: "UTC")
        return f.string(from: Date())
    }
}

/// Compact horizontal label so the auto-download badge on the
/// subscriptions list reads as a single inline chip — default `Label`
/// stacks the icon at a heavier weight than this row needs.
private struct StatusBadgeLabelStyle: LabelStyle {
    func makeBody(configuration: Configuration) -> some View {
        HStack(spacing: 3) {
            configuration.icon
                .font(.system(size: 9, weight: .semibold))
            configuration.title
        }
    }
}
