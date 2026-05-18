import SwiftUI

@main
struct NmpPulseApp: App {
    @StateObject private var model = KernelModel()

    var body: some Scene {
        WindowGroup {
            RootView()
                .environmentObject(model)
        }
    }
}
