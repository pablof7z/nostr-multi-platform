import SwiftUI
// OWNER: Phase-2 Agent B (Profile screen). Replace whole file.
// Init signature is FIXED by the nav contract: ProfileView(pubkey:).
struct ProfileView: View {
    let pubkey: String
    @EnvironmentObject private var model: KernelModel
    var body: some View {
        ChirpPlaceholder(systemImage: "person.crop.circle", title: "Profile",
            subtitle: "Agent B builds this.")
            .navigationTitle("Profile").navigationBarTitleDisplayMode(.inline)
    }
}
