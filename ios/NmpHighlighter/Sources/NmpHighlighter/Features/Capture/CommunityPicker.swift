import Kingfisher
import SwiftUI

/// Pick which community to publish the capture into. Pulls from
/// `appStore.joinedCommunities` — no extra fetch needed.
struct CommunityPicker: View {
    @Environment(HighlighterStore.self) private var appStore
    @Environment(\.dismiss) private var dismiss

    @Binding var selection: String?

    var body: some View {
        NavigationStack {
            List(appStore.joinedCommunities, id: \.id) { community in
                Button {
                    selection = community.id
                    dismiss()
                } label: {
                    HStack(spacing: 12) {
                        if let url = URL(string: community.picture), !community.picture.isEmpty {
                            KFImage(url)
                                .placeholder { Color.highlighterPaper.opacity(0.5) }
                                .fade(duration: 0.15)
                                .resizable()
                                .scaledToFill()
                                .frame(width: 32, height: 32)
                                .clipShape(RoundedRectangle(cornerRadius: 6))
                        } else {
                            Image(systemName: "square.grid.2x2")
                                .frame(width: 32, height: 32)
                                .foregroundStyle(Color.highlighterInkMuted)
                        }
                        Text(community.name.isEmpty ? community.id : community.name)
                            .foregroundStyle(Color.highlighterInkStrong)
                        Spacer()
                        if selection == community.id {
                            Image(systemName: "checkmark")
                                .foregroundStyle(Color.highlighterAccent)
                        }
                    }
                }
            }
            .overlay {
                if appStore.joinedCommunities.isEmpty {
                    ContentUnavailableView(
                        "No communities yet",
                        systemImage: "square.grid.2x2",
                        description: Text("Join or create a community before publishing captures.")
                    )
                }
            }
            .navigationTitle("Choose a community")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                }
            }
        }
    }
}
