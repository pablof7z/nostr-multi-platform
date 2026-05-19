import SwiftUI

// MARK: - EpisodeRowContextMenu

/// Shared "long-press menu" for any episode row in the app.
///
/// Surfaces:
///   - Open episode details (NavigationLink with a caller-supplied route value).
///   - Mark as played / Mark as unplayed (toggle based on `episode.played`).
///   - Download / Cancel download / Remove download / Retry download
///     (state-aware via `EpisodeDownloadService.shared`).
///   - Share (`ShareLink` with the enclosure URL).
///
/// The route is generic over `Hashable` so Library can pass `LibraryEpisodeRoute`
/// and Home can pass `HomeEpisodeRoute` without either feature owning the menu.
///
/// Usage with `.contextMenu`:
/// ```swift
/// .contextMenu {
///     EpisodeRowContextMenu(
///         episode: ep,
///         store: store,
///         openDetailsRoute: LibraryEpisodeRoute(...)
///     )
/// }
/// ```
///
/// Usage with `.accessibilityActions` (mirror the same items so VoiceOver
/// users get the same affordances — see `episodeRowAccessibilityActions`):
struct EpisodeRowContextMenu<Route: Hashable>: View {
    let episode: Episode
    let store: AppStateStore
    let openDetailsRoute: Route
    /// Optional playback handle. When supplied, the menu surfaces
    /// "Add to Queue" / "Remove from Queue" affordances and a
    /// "Share with timestamp" target when this episode is the currently
    /// playing one and the playhead is meaningful. Defaults to `nil` so
    /// existing call sites (Home cards) keep their current behavior.
    var playback: PlaybackState? = nil

    /// Live download service — observed so the surfaced affordance flips between
    /// Download / Cancel / Remove / Retry as the underlying state moves.
    @State private var downloadService = EpisodeDownloadService.shared

    var body: some View {
        // No wrapping container view — `.contextMenu` walks the body looking
        // for menu items (Buttons / NavigationLinks / ShareLinks). A `Group`
        // works for content but any modifier attached to it (e.g. a
        // `.confirmationDialog`) would orphan because the Group isn't part of
        // the visible hierarchy. Keep the body item-only.
        NavigationLink(value: openDetailsRoute) {
            Label("Open episode details", systemImage: "info.circle")
        }

        Button {
            togglePlayed()
        } label: {
            Label(
                episode.played ? "Mark as unplayed" : "Mark as played",
                systemImage: episode.played ? "circle" : "checkmark.circle"
            )
        }

        Button {
            Haptics.light()
            store.toggleEpisodeStarred(episode.id)
        } label: {
            Label(
                episode.isStarred ? "Unstar" : "Star",
                systemImage: episode.isStarred ? "star.slash" : "star"
            )
        }

        queueButton

        downloadButton

        shareSection
    }

    private var sharePreviewTitle: String {
        let showName = store.podcast(id: episode.podcastID)?.title ?? ""
        return showName.isEmpty ? episode.title : "\(showName): \(episode.title)"
    }

    // MARK: - Queue affordance

    /// Surfaced only when a `PlaybackState` was supplied. The flip between
    /// "Add to Queue" and "Remove from Queue" reads the live `queue` so the
    /// label always matches the underlying state — a long-press immediately
    /// after a swipe-add lands on "Remove" without a second render pass.
    @ViewBuilder
    private var queueButton: some View {
        if let playback {
            let isQueued = playback.isQueued(episode.id)
            let isCurrent = playback.episode?.id == episode.id
            // The currently-playing episode is intentionally omitted from the
            // queue (`PlaybackState.enqueue` rejects it) so neither affordance
            // is meaningful — hide the row entirely instead of surfacing a
            // no-op button.
            if !isCurrent {
                Button {
                    if isQueued {
                        Haptics.light()
                        playback.removeFromQueue(episode.id)
                    } else {
                        Haptics.success()
                        playback.enqueue(episode.id)
                    }
                } label: {
                    Label(
                        isQueued ? "Remove from Queue" : "Add to Queue",
                        systemImage: isQueued ? "text.badge.minus" : "text.badge.plus"
                    )
                }
            }
        }
    }

    // MARK: - Share affordances

    /// Two share targets:
    ///   - Always: a `ShareLink` over the `podcastr://e/<guid>` deep link.
    ///   - When the episode is the currently-playing one with a meaningful
    ///     playhead: a second `ShareLink` carrying `?t=<seconds>` so the
    ///     recipient lands at the same point in time. Mirrors the player's
    ///     own share-sheet semantics in `PlayerShareSheet`.
    @ViewBuilder
    private var shareSection: some View {
        // Bare deep-link share. Falls back to the enclosure URL when the
        // deep-link string fails to parse (defensive — `podcastr://e/<guid>`
        // is generated from a non-empty GUID, so this branch is unreachable
        // in practice).
        if let deepLinkURL = URL(string: episodeDeepLink) {
            ShareLink(
                item: deepLinkURL,
                subject: Text(episode.title),
                preview: SharePreview(sharePreviewTitle, image: Image(systemName: "headphones"))
            ) {
                Label("Share", systemImage: "square.and.arrow.up")
            }
        } else {
            ShareLink(
                item: episode.enclosureURL,
                preview: SharePreview(sharePreviewTitle, image: Image(systemName: "headphones"))
            ) {
                Label("Share", systemImage: "square.and.arrow.up")
            }
        }

        if let timestampedURL = timestampedDeepLinkURL {
            ShareLink(
                item: timestampedURL,
                subject: Text(episode.title),
                preview: SharePreview(sharePreviewTitle, image: Image(systemName: "clock"))
            ) {
                Label("Share with timestamp", systemImage: "clock.arrow.circlepath")
            }
        }
    }

    /// Spec literal: `podcastr://e/<guid>` — matches `PlayerShareSheet`'s
    /// `episodeDeepLink` so a long-press share and a player-sheet share point
    /// at the same canonical URL.
    private var episodeDeepLink: String {
        DeepLinkHandler.episodeGUIDDeepLink(guid: episode.guid)
            ?? episode.enclosureURL.absoluteString
    }

    /// `nil` when the episode isn't the currently-playing one or when the
    /// playhead is too close to the start to be meaningful — the player
    /// share-sheet uses the same gate (`hasMeaningfulPlayhead`) and we
    /// match its threshold here so the two surfaces stay in lockstep.
    private var timestampedDeepLinkURL: URL? {
        guard let playback else { return nil }
        guard playback.episode?.id == episode.id else { return nil }
        let currentTime = playback.currentTime
        guard PlayerShareSheet.isMeaningfulPlayhead(currentTime) else { return nil }
        return DeepLinkHandler.episodeGUIDURL(guid: episode.guid, startTime: currentTime)
    }

    // MARK: - Download affordance

    @ViewBuilder
    private var downloadButton: some View {
        switch episode.downloadState {
        case .notDownloaded, .queued:
            Button {
                Haptics.light()
                EpisodeDownloadService.shared.attach(appStore: store)
                EpisodeDownloadService.shared.download(episodeID: episode.id)
            } label: {
                Label("Download", systemImage: "arrow.down.circle")
            }
        case .downloading:
            Button {
                Haptics.light()
                EpisodeDownloadService.shared.attach(appStore: store)
                EpisodeDownloadService.shared.cancel(episodeID: episode.id)
            } label: {
                Label("Cancel download", systemImage: "xmark.circle")
            }
        case .downloaded:
            // Match the trailing-swipe behavior — remove immediately. The
            // detail view's `EpisodeDetailActionsMenu` keeps the confirmation
            // dialog because it's hosted on a real `Menu` parent. Inside
            // `.contextMenu` the dialog modifier orphans (the menu items don't
            // land in the visible hierarchy), so we drop it here for parity.
            Button(role: .destructive) {
                Haptics.warning()
                EpisodeDownloadService.shared.attach(appStore: store)
                EpisodeDownloadService.shared.delete(episodeID: episode.id)
            } label: {
                Label("Remove download", systemImage: "trash")
            }
        case .failed:
            Button {
                Haptics.light()
                EpisodeDownloadService.shared.attach(appStore: store)
                EpisodeDownloadService.shared.download(episodeID: episode.id)
            } label: {
                Label("Retry download", systemImage: "arrow.clockwise")
            }
        }
    }

    // MARK: - Mutations

    private func togglePlayed() {
        if episode.played {
            Haptics.itemReopen()
            store.markEpisodeUnplayed(episode.id)
        } else {
            Haptics.itemComplete()
            store.markEpisodePlayed(episode.id)
        }
    }
}

// MARK: - Accessibility-action mirror

/// `.accessibilityActions` builder that mirrors `EpisodeRowContextMenu` so
/// VoiceOver users get the same options the long-press menu surfaces. Kept
/// in this file so the menu and the custom-actions stay in lockstep when one
/// changes — divergence is the failure mode the brief flagged.
///
/// Note: `accessibilityActions` cannot host a `NavigationLink`, so the
/// "Open episode details" affordance is delegated to a closure the caller
/// wires up via the route binding. We deliberately omit the share affordance
/// here — `ShareLink` does not surface reliably as a VoiceOver custom action
/// (UIActivityViewController presentation from a custom-action context fails
/// silently). The long-press menu retains Share for sighted users.
struct EpisodeRowAccessibilityActions: View {
    let episode: Episode
    let store: AppStateStore
    let onOpenDetails: () -> Void

    var body: some View {
        Button("Open episode details") { onOpenDetails() }
        Button(episode.played ? "Mark as unplayed" : "Mark as played") {
            togglePlayed()
        }
        // Download/Cancel/Remove/Retry are now exposed via the trailing-edge
        // swipe action (`EpisodeRowDownloadSwipeAction`), which SwiftUI mirrors
        // into VoiceOver's custom-actions list automatically. Listing the same
        // affordance here would render the rotor entry twice — easy to do
        // accidentally because `.swipeActions` looks like a sighted-only API.
    }

    private func togglePlayed() {
        if episode.played {
            store.markEpisodeUnplayed(episode.id)
        } else {
            store.markEpisodePlayed(episode.id)
        }
    }
}

// Swipe-action helpers (`EpisodeRowLeadingSwipeAction`,
// `EpisodeRowTrailingSwipeAction`, `EpisodeRowDownloadSwipeAction`) live in
// `EpisodeRowSwipeActions.swift` so this file stays focused on the
// long-press menu and its accessibility mirror.
