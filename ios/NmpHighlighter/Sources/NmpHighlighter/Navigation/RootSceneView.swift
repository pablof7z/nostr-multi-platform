import SwiftUI

struct RootSceneView: View {
    @Environment(HighlighterStore.self) private var store
    @Environment(\.scenePhase) private var scenePhase
    @State private var feedbackPresented: Bool = false
    /// Debounce repeated `motionEnded` callbacks for the same physical shake;
    /// iOS often delivers two within ~250ms.
    @State private var lastShakeAt: Date = .distantPast

    @AppStorage("onboardingComplete") private var isOnboardingComplete: Bool = false

    var body: some View {
        Group {
            if store.isLoggedIn {
                MainTabView()
            } else if isOnboardingComplete {
                NavigationStack { LoginView() }
            } else {
                NavigationStack { OnboardingView() }
            }
        }
        .task {
            await store.bootstrap()
        }
        .onChange(of: scenePhase) { _, newPhase in
            if newPhase == .active {
                Task { await ShareQueueProcessor.drain(app: store) }
                // iOS suspends WebSockets while we're backgrounded; nostr-sdk's
                // `connect()` is idempotent and skips relays it still believes
                // are connected, so disconnect first to force a fresh socket
                // and subscription re-issue. Without this the NIP-46
                // nostrconnect:// flow misses Primal's response when the user
                // comes back from the signer app.
                Task {
                    try? await store.safeCore.disconnectAll()
                    try? await store.safeCore.reconnectAll()
                }
            }
        }
        .overlay(alignment: .top) {
            if let toast = store.shareToast {
                ShareToastBanner(text: toast) {
                    store.shareToast = nil
                }
                .padding(.top, 8)
                .transition(.move(edge: .top).combined(with: .opacity))
            }
        }
        .animation(.easeInOut(duration: 0.25), value: store.shareToast)
        .onShake { handleShake() }
        .sheet(isPresented: $feedbackPresented) {
            FeedbackThreadsView()
        }
    }

    private func handleShake() {
        guard store.isLoggedIn else { return }
        let now = Date()
        if now.timeIntervalSince(lastShakeAt) < 1.0 { return }
        lastShakeAt = now
        if !feedbackPresented {
            feedbackPresented = true
        }
    }
}

private struct ShareToastBanner: View {
    let text: String
    let onDismiss: () -> Void

    var body: some View {
        HStack {
            Image(systemName: "checkmark.circle.fill")
                .foregroundStyle(.white)
            Text(text)
                .foregroundStyle(.white)
                .font(.subheadline.weight(.medium))
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 10)
        .background(Color.green.opacity(0.9), in: .capsule)
        .shadow(radius: 6)
        .task {
            try? await Task.sleep(nanoseconds: 3 * 1_000_000_000)
            onDismiss()
        }
    }
}
