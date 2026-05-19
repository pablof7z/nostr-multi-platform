import Kingfisher
import SwiftUI

/// Swipeable featured-room hero. Full-bleed cover with the room name and a
/// one-line pitch overlaid. Taps hand the room up to the parent as a
/// preview-sheet request.
struct ExplorerHeroView: View {
    let rooms: [CommunitySummary]
    let onTap: (CommunitySummary) -> Void

    @State private var index: Int = 0

    var body: some View {
        TabView(selection: $index) {
            ForEach(Array(rooms.enumerated()), id: \.element.id) { pair in
                Button {
                    onTap(pair.element)
                } label: {
                    heroCard(for: pair.element)
                }
                .buttonStyle(.plain)
                .tag(pair.offset)
                .padding(.horizontal, 18)
            }
        }
        .tabViewStyle(.page(indexDisplayMode: rooms.count > 1 ? .automatic : .never))
        .indexViewStyle(.page(backgroundDisplayMode: .interactive))
        .frame(height: 280)
    }

    private func heroCard(for room: CommunitySummary) -> some View {
        ZStack(alignment: .bottomLeading) {
            // Cover backdrop
            if let url = URL(string: room.picture), !room.picture.isEmpty {
                KFImage(url)
                    .placeholder { coverFallback }
                    .fade(duration: 0.2)
                    .resizable()
                    .scaledToFill()
            } else {
                coverFallback
            }

            // Darkening gradient for text legibility
            LinearGradient(
                colors: [
                    .black.opacity(0.0),
                    .black.opacity(0.15),
                    .black.opacity(0.72),
                ],
                startPoint: .top,
                endPoint: .bottom
            )

            VStack(alignment: .leading, spacing: 6) {
                Text("Featured".uppercased())
                    .font(.caption.weight(.semibold))
                    .tracking(1.2)
                    .foregroundStyle(Color.white.opacity(0.78))
                Text(room.name)
                    .font(.system(.title2, design: .default).weight(.semibold))
                    .foregroundStyle(.white)
                    .lineLimit(2)
                if !room.about.isEmpty {
                    Text(room.about)
                        .font(.subheadline)
                        .foregroundStyle(Color.white.opacity(0.82))
                        .lineLimit(2)
                }
                HStack(spacing: 8) {
                    Image(systemName: "hand.tap")
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(Color.white)
                    Text("Tap to preview")
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(Color.white)
                }
                .padding(.top, 4)
            }
            .padding(20)
        }
        .frame(height: 260)
        .clipShape(RoundedRectangle(cornerRadius: 20))
        .overlay(
            RoundedRectangle(cornerRadius: 20)
                .stroke(Color.black.opacity(0.08), lineWidth: 0.5)
        )
    }

    private var coverFallback: some View {
        LinearGradient(
            colors: [Color.highlighterAccent.opacity(0.8), Color.highlighterAccent.opacity(0.45)],
            startPoint: .topLeading,
            endPoint: .bottomTrailing
        )
    }
}
