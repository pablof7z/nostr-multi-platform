import CoreSpotlight
import SwiftUI

extension RootView {

    func handleDeepLink(_ url: URL) {
        guard let link = DeepLinkHandler.resolve(url) else { return }
        switch link {
        case .settings:
            showSettings = true
        case .feedback:
            showFeedback = true
        case .agent:
            openAgentChat()
        case .addFriend(let npub, let name):
            showSettings = true
            Task { @MainActor in
                store.pendingFriendInvite = PendingFriendInvite(npub: npub, name: name)
            }
        case .episode(let uuid):
            spotlightSheet = .episode(uuid)
        case .episodeByGUID(let guid, let startTime):
            if let episode = store.state.episodes.first(where: { $0.id.uuidString == guid || $0.guid == guid }) {
                if let startTime {
                    playbackState.setEpisode(episode)
                    playbackState.navigationalSeek(to: startTime)
                    playbackState.play()
                }
                spotlightSheet = .episode(episode.id)
            }
        case .subscription(let uuid):
            spotlightSheet = .subscription(uuid)
        case .clip(let clipID):
            if let clip = store.clip(id: clipID),
               let episode = store.episode(id: clip.episodeID) {
                playbackState.setEpisode(episode)
                playbackState.navigationalSeek(to: clip.startSeconds)
                playbackState.play()
                spotlightSheet = .episode(episode.id)
            }
        }
    }

    func handleSpotlight(_ activity: NSUserActivity) {
        guard let link = SpotlightIndexer.deepLink(from: activity) else { return }
        spotlightSheet = link
    }

    @ViewBuilder
    func spotlightDetailView(for link: SpotlightIndexer.DeepLink) -> some View {
        switch link {
        case .note(let id):
            AgentNotesView(spotlightTargetID: id)
        case .memory(let id):
            AgentMemoriesView(spotlightTargetID: id)
        case .subscription(let id):
            if let podcast = store.podcast(id: id) {
                ShowDetailView(podcast: podcast)
            } else {
                spotlightMissing("Show not found", "This subscription is no longer in your library.")
            }
        case .episode(let id):
            if store.episode(id: id) != nil {
                EpisodeDetailView(episodeID: id)
            } else {
                spotlightMissing("Episode not found", "This episode is no longer in your library.")
            }
        }
    }

    func spotlightMissing(_ title: String, _ subtitle: String) -> some View {
        ContentUnavailableView(
            title,
            systemImage: "questionmark.folder",
            description: Text(subtitle)
        )
    }
}
