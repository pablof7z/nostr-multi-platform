import SwiftUI

@main
struct ChirpApp: App {
    @StateObject private var model = KernelModel()

    var body: some Scene {
        WindowGroup {
            RootShell()
                .environmentObject(model)
                .tint(ChirpColor.accent)
                .preferredColorScheme(.dark)
                .task { model.start() }
        }
    }
}
