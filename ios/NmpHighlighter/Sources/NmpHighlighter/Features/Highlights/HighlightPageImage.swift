import Kingfisher
import SwiftUI

/// The page-photo accompaniment to a highlight (NIP-92 `imeta` URL on a
/// kind:9802 event). When the user captures a passage from a physical book
/// or paper, the published highlight carries a Blossom URL pointing at the
/// scan with the passage marked in yellow. This view renders that scan
/// edge-to-edge and opens an `ImageZoomView` on tap.
///
/// Three sizes match the surfaces that render highlights:
///   - `.feature` — full-bleed lead image inside a single-highlight feed
///     module. Caps height so a portrait page photo doesn't dominate.
///   - `.card`    — the fixed-width quote card inside the multi-highlight
///     reel. Smaller cap; the card sets the width.
///   - `.row`     — a square thumbnail for compact list rows (search,
///     profile). Replaces the accent rail when an image is present.
struct HighlightPageImage: View {
    enum Treatment {
        case feature
        case card
        case row
    }

    let url: URL
    let treatment: Treatment

    @State private var isZoomed: Bool = false

    var body: some View {
        Button {
            isZoomed = true
        } label: {
            container
        }
        .buttonStyle(.plain)
        .fullScreenCover(isPresented: $isZoomed) {
            ImageZoomView(url: url, onDismiss: { isZoomed = false })
        }
    }

    @ViewBuilder
    private var container: some View {
        switch treatment {
        case .feature:
            image
                .frame(maxWidth: .infinity)
                .frame(maxHeight: 420)
                .clipShape(RoundedRectangle(cornerRadius: 6, style: .continuous))
                .overlay(
                    RoundedRectangle(cornerRadius: 6, style: .continuous)
                        .stroke(Color.highlighterRule.opacity(0.6), lineWidth: 0.5)
                )
                .shadow(color: Color.black.opacity(0.10), radius: 6, x: 0, y: 2)
        case .card:
            image
                .frame(maxWidth: .infinity)
                .frame(maxHeight: 160)
                .clipShape(RoundedRectangle(cornerRadius: 5, style: .continuous))
                .overlay(
                    RoundedRectangle(cornerRadius: 5, style: .continuous)
                        .stroke(Color.highlighterRule.opacity(0.6), lineWidth: 0.5)
                )
        case .row:
            image
                .frame(width: 64, height: 80)
                .clipShape(RoundedRectangle(cornerRadius: 4, style: .continuous))
                .overlay(
                    RoundedRectangle(cornerRadius: 4, style: .continuous)
                        .stroke(Color.highlighterRule.opacity(0.6), lineWidth: 0.5)
                )
        }
    }

    private var image: some View {
        KFImage(url)
            .placeholder { placeholder }
            .fade(duration: 0.18)
            .resizable()
            .scaledToFill()
            .clipped()
    }

    private var placeholder: some View {
        Rectangle()
            .fill(Color.highlighterPaperTint)
            .overlay(
                Image(systemName: "book.pages")
                    .font(.system(size: 18, weight: .regular))
                    .foregroundStyle(Color.highlighterInkMuted.opacity(0.5))
            )
    }
}

