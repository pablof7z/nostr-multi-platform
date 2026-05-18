import SwiftUI

struct CachedAsyncImage<Placeholder: View>: View {
    let url: URL?
    let placeholder: () -> Placeholder

    @State private var image: UIImage?
    @State private var isLoading = false

    init(url: URL?, @ViewBuilder placeholder: @escaping () -> Placeholder) {
        self.url = url
        self.placeholder = placeholder
    }

    var body: some View {
        Group {
            if let image {
                Image(uiImage: image)
                    .resizable()
            } else {
                placeholder()
                    .onAppear { loadImage() }
            }
        }
    }

    private func loadImage() {
        guard let url, !isLoading else { return }
        isLoading = true

        Task {
            let loaded = await ImageCache.shared.image(for: url)
            await MainActor.run {
                self.image = loaded
                self.isLoading = false
            }
        }
    }
}
