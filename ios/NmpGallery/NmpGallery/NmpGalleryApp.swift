import SwiftUI

@main
struct NmpGalleryApp: App {
    var body: some Scene {
        WindowGroup {
            RootView()
        }
    }
}

struct RootView: View {
    private let result = BundleLoader.load()

    var body: some View {
        switch result {
        case let .success(bundle):
            GalleryView(bundle: bundle)
        case let .failure(error):
            VStack(spacing: 12) {
                Image(systemName: "exclamationmark.triangle")
                    .font(.largeTitle)
                Text("Bundle load failed")
                    .font(.headline)
                Text(error.message)
                    .font(.footnote)
                    .multilineTextAlignment(.center)
                    .padding()
            }
        }
    }
}
