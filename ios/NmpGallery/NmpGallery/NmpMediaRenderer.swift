import SwiftUI

/// Extensibility seam for media rendering. Inject via
/// `.environment(\.nmpMediaRenderer, custom)` at the app or feed level.
/// Apps can plug in Kingfisher, Nuke, SDWebImage, or a custom AVKit player.
///
/// Example — Kingfisher:
/// ```swift
/// ContentView()
///     .environment(\.nmpMediaRenderer, NmpMediaRenderer(
///         imageView: { url in AnyView(KFImage(url).resizable().scaledToFit()) },
///         videoView: NmpMediaRenderer.default.videoView
///     ))
/// ```
// @unchecked because closures are only ever invoked on @MainActor SwiftUI views.
struct NmpMediaRenderer: @unchecked Sendable {
    var imageView: (URL) -> AnyView
    var videoView: (URL) -> AnyView

    static let `default` = NmpMediaRenderer(
        imageView: { url in
            AnyView(
                AsyncImage(url: url) { phase in
                    switch phase {
                    case .success(let img):
                        img.resizable()
                            .scaledToFit()
                            .frame(maxWidth: .infinity, maxHeight: 400)
                            .clipShape(RoundedRectangle(cornerRadius: 10))
                    case .failure:
                        Label("Failed to load", systemImage: "photo.slash")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                            .frame(maxWidth: .infinity, minHeight: 60)
                            .background(Color.secondary.opacity(0.08))
                            .clipShape(RoundedRectangle(cornerRadius: 10))
                    case .empty:
                        ProgressView()
                            .frame(maxWidth: .infinity, minHeight: 80)
                    @unknown default:
                        EmptyView()
                    }
                }
            )
        },
        videoView: { url in
            AnyView(
                HStack(spacing: 10) {
                    Image(systemName: "play.rectangle.fill")
                        .font(.title2)
                        .foregroundStyle(.white)
                    VStack(alignment: .leading, spacing: 2) {
                        Text("Video")
                            .font(.caption.bold())
                            .foregroundStyle(.white)
                        Text(url.lastPathComponent)
                            .font(.caption2.monospaced())
                            .foregroundStyle(.white.opacity(0.7))
                            .lineLimit(1)
                    }
                    Spacer()
                }
                .padding(12)
                .frame(maxWidth: .infinity)
                .background(Color.black.opacity(0.72))
                .clipShape(RoundedRectangle(cornerRadius: 10))
            )
        }
    )
}

private struct NmpMediaRendererKey: EnvironmentKey {
    static let defaultValue: NmpMediaRenderer = .default
}

extension EnvironmentValues {
    var nmpMediaRenderer: NmpMediaRenderer {
        get { self[NmpMediaRendererKey.self] }
        set { self[NmpMediaRendererKey.self] = newValue }
    }
}
