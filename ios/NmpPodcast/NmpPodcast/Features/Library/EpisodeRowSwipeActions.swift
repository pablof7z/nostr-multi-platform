import SwiftUI

// MARK: - Swipe-action helpers
//
// Extracted from `EpisodeRowContextMenu.swift` to keep that file under the
// 300-line soft cap once the menu itself grew to cover queue + share-with-
// timestamp affordances. The swipe actions are the same ones declared
// previously and are imported by `ShowDetailEpisodeList` /
// `HomeRecentEpisodeRow` unchanged.

/// Leading-edge swipe action: "Add to Queue".
///
/// Appends the episode to `PlaybackState.queue` so it's picked up by the
/// "Up Next" rail. No-op (silently absorbed by `PlaybackState.enqueue`)
/// when the episode is already queued or is the currently-playing item —
/// the swipe still resolves visually so the user gets affordance feedback.
///
/// Apply via `.swipeActions(edge: .leading, allowsFullSwipe: true) { ... }`
/// on a `List` row — the enclosing call site decides where this lives so
/// non-`List` surfaces (the Home rail card) can opt out cleanly. The
/// download / mark-played affordances that previously lived on the swipe
/// edges are still available via `EpisodeRowContextMenu` (long-press).
struct EpisodeRowLeadingSwipeAction: View {
    let episode: Episode
    let playback: PlaybackState

    var body: some View {
        Button {
            Haptics.success()
            playback.enqueue(episode.id)
        } label: {
            Label("Add to Queue", systemImage: "text.badge.plus")
        }
        .tint(AppTheme.Tint.agentSurface)
    }
}

/// Trailing-edge swipe action: destructive "Remove" — drops the episode
/// from the visible list.
///
/// "Remove from list" semantically means "treat as done": calling
/// `markEpisodePlayed` removes the episode from `recentEpisodes`
/// (filters on `!played`) and from `inProgressEpisodes`, which is what
/// the user actually wants when they swipe away an item they're not
/// going to listen to. The mark-unplayed affordance remains available
/// via `EpisodeRowContextMenu` (long-press).
struct EpisodeRowTrailingSwipeAction: View {
    let episode: Episode
    let store: AppStateStore

    var body: some View {
        Button(role: .destructive) {
            Haptics.warning()
            store.markEpisodePlayed(episode.id)
        } label: {
            Label("Remove", systemImage: "trash")
        }
    }
}

/// Trailing-edge swipe action: state-aware Download / Cancel / Remove / Retry.
///
/// Pairs with `EpisodeRowTrailingSwipeAction` on the trailing edge so the
/// download affordance is discoverable without long-pressing. Order matters:
/// SwiftUI lays the first declared button rightmost (closest to the swipe
/// edge), so the destructive `Remove` action sits outermost and Download
/// occupies the inner slot — a deliberate trade-off so a quick partial
/// swipe still surfaces the more dangerous action behind a deliberate tap
/// while the safer Download is one tap further in.
struct EpisodeRowDownloadSwipeAction: View {
    let episode: Episode
    let store: AppStateStore

    var body: some View {
        switch episode.downloadState {
        case .notDownloaded, .queued:
            Button {
                Haptics.light()
                EpisodeDownloadService.shared.attach(appStore: store)
                EpisodeDownloadService.shared.download(episodeID: episode.id)
            } label: {
                Label("Download", systemImage: "arrow.down.circle")
            }
            .tint(.blue)
        case .downloading:
            Button {
                Haptics.light()
                EpisodeDownloadService.shared.attach(appStore: store)
                EpisodeDownloadService.shared.cancel(episodeID: episode.id)
            } label: {
                Label("Cancel", systemImage: "xmark.circle")
            }
            .tint(AppTheme.Tint.warning)
        case .downloaded:
            // Not `role: .destructive` — that paints the swipe button red and
            // makes it visually identical to the existing "Remove" (mark-played)
            // action that sits next to it. Removing the local audio file just
            // frees storage; the episode and its progress survive. A neutral
            // gray tint signals "secondary cleanup" instead of "destroy data".
            Button {
                Haptics.light()
                EpisodeDownloadService.shared.attach(appStore: store)
                EpisodeDownloadService.shared.delete(episodeID: episode.id)
            } label: {
                Label("Free up", systemImage: "internaldrive")
            }
            .tint(.gray)
        case .failed:
            Button {
                Haptics.light()
                EpisodeDownloadService.shared.attach(appStore: store)
                EpisodeDownloadService.shared.download(episodeID: episode.id)
            } label: {
                Label("Retry", systemImage: "arrow.clockwise")
            }
            .tint(.blue)
        }
    }
}
