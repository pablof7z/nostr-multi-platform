import SwiftUI
// OWNER: Phase-2 Agent A (Home timeline + note cell + compose entry).
// Replace this whole file. Use model.items / model.profile. Push
// ChirpRoute.profile / .thread via @EnvironmentObject ChirpRouter.
struct HomeFeedView: View {
    @EnvironmentObject private var model: KernelModel
    var body: some View {
        ChirpPlaceholder(systemImage: "house.fill", title: "Home",
            subtitle: "Timeline lands here (Agent A).")
            .navigationTitle("Chirp")
    }
}
