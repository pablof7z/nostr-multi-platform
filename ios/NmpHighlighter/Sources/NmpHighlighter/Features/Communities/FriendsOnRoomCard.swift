import Kingfisher
import SwiftUI

/// 3:4 cover card with an overlapping avatar cluster in the bottom-left —
/// the social-proof shelf for "Friends are here". Caption reads
/// "@alice, @bob + 2" or just "@alice + 1" depending on count.
struct FriendsOnRoomCard: View {
    let recommendation: RoomRecommendation

    @Environment(HighlighterStore.self) private var store

    private let width: CGFloat = 96

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            ZStack(alignment: .bottomLeading) {
                cover
                    .frame(width: width, height: width)
                    .clipped()
                    .clipShape(RoundedRectangle(cornerRadius: 14))
                    .overlay(
                        RoundedRectangle(cornerRadius: 14)
                            .stroke(Color.highlighterRule, lineWidth: 0.5)
                    )

                avatarCluster
                    .padding(8)
            }

            VStack(alignment: .leading, spacing: 2) {
                Text(recommendation.summary.name)
                    .font(.caption.weight(.medium))
                    .foregroundStyle(Color.highlighterInkStrong)
                    .lineLimit(2)
                    .multilineTextAlignment(.leading)

                Text(friendsByline)
                    .font(.caption2)
                    .foregroundStyle(Color.highlighterInkMuted)
                    .lineLimit(1)
            }
            .frame(width: width, alignment: .leading)
        }
        .task {
            // Warm the profile cache for the friends shown in the cluster
            // so avatars render with actual pictures, not initials.
            for pubkey in recommendation.reasonPubkeys.prefix(3) {
                await store.requestProfile(pubkeyHex: pubkey)
            }
        }
    }

    private var friendsByline: String {
        let total = recommendation.reasonPubkeys.count
        if total == 0 { return recommendation.summary.about.isEmpty ? "Rooms you may like" : recommendation.summary.about }
        let firstHandle = handle(for: recommendation.reasonPubkeys[0])
        if total == 1 {
            return "@\(firstHandle) is here"
        }
        if total == 2 {
            return "@\(firstHandle) + 1 you follow"
        }
        return "@\(firstHandle) + \(total - 1) you follow"
    }

    private func handle(for pubkey: String) -> String {
        if let profile = store.profileCache[pubkey] {
            if !profile.name.isEmpty { return profile.name }
            if !profile.displayName.isEmpty { return profile.displayName }
        }
        return String(pubkey.prefix(6))
    }

    @ViewBuilder
    private var cover: some View {
        if let url = URL(string: recommendation.summary.picture), !recommendation.summary.picture.isEmpty {
            KFImage(url)
                .placeholder { coverFallback }
                .fade(duration: 0.15)
                .resizable()
                .scaledToFill()
        } else {
            coverFallback
        }
    }

    private var coverFallback: some View {
        LinearGradient(
            colors: [
                Color.highlighterAccent.opacity(0.38),
                Color.highlighterAccent.opacity(0.14),
            ],
            startPoint: .topLeading,
            endPoint: .bottomTrailing
        )
    }

    private var avatarCluster: some View {
        let show = recommendation.reasonPubkeys.prefix(3)
        return HStack(spacing: -8) {
            ForEach(Array(show.enumerated()), id: \.offset) { item in
                AuthorAvatar(
                    pubkey: item.element,
                    pictureURL: store.profileCache[item.element]?.picture ?? "",
                    displayInitial: String(item.element.prefix(1)),
                    size: 26
                )
                .overlay(
                    Circle().stroke(Color.white, lineWidth: 2)
                )
            }
            if recommendation.reasonPubkeys.count > 3 {
                ZStack {
                    Circle().fill(Color.black.opacity(0.55))
                    Text("+\(recommendation.reasonPubkeys.count - 3)")
                        .font(.caption2.weight(.bold))
                        .foregroundStyle(.white)
                }
                .frame(width: 26, height: 26)
                .overlay(Circle().stroke(Color.white, lineWidth: 2))
            }
        }
    }
}
