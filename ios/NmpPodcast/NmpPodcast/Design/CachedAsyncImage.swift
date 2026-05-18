// import Kingfisher  // T-podcast-ios-RESTART: shim uses SwiftUI AsyncImage
import SwiftUI

/// Drop-in caching replacement for SwiftUI's `AsyncImage`, backed by
/// Kingfisher's memory + disk cache.
///
/// Same call signature as `AsyncImage` so swapping is a one-token edit:
///
/// ```swift
/// CachedAsyncImage(url: artworkURL) { phase in
///     switch phase {
///     case .success(let image): image.resizable().scaledToFill()
///     case .failure:            Color.secondary.opacity(0.1)
///     case .empty:              ProgressView()
///     @unknown default:         EmptyView()
///     }
/// }
/// ```
///
/// T-podcast-gap-005: Replace `import Kingfisher` and body when Kingfisher is
/// added as an SPM dep. Until then, falls back to SwiftUI's own `AsyncImage`
/// which re-downloads on each appearance but is functionally correct.
/// Verbatim copy from:
/// /Users/pablofernandez/Work/podcast/App/Sources/Design/CachedAsyncImage.swift
/// — only the `import Kingfisher` line and body are replaced by this shim.
struct CachedAsyncImage<Content: View>: View {

    let url: URL?
    let scale: CGFloat
    let targetSize: CGSize?
    @ViewBuilder let content: (AsyncImagePhase) -> Content

    init(
        url: URL?,
        scale: CGFloat = 1,
        targetSize: CGSize? = nil,
        @ViewBuilder content: @escaping (AsyncImagePhase) -> Content
    ) {
        self.url = url
        self.scale = scale
        self.targetSize = targetSize
        self.content = content
    }

    var body: some View {
        AsyncImage(url: url, scale: scale, content: content)
    }
}
