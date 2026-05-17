import SwiftUI

@main
struct NmpStressApp: App {
    @StateObject private var model = KernelModel()

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environmentObject(model)
        }
    }
}
