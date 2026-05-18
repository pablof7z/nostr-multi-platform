import SwiftUI
// OWNER: Phase-2 Agent D (Wallet — polished "Coming in Chirp CX2" surface;
// no wallet FFI at v1. Show lud16-style teaser, zap explainer).
struct WalletView: View {
    @EnvironmentObject private var model: KernelModel
    var body: some View {
        ChirpPlaceholder(systemImage: "bolt.fill", title: "Wallet",
            subtitle: "Agent D builds this.")
            .navigationTitle("Wallet")
    }
}
