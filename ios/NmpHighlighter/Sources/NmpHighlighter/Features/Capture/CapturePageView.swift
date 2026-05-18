import SwiftUI
import UIKit

/// Review screen after capture. The corrected photo is the primary surface:
/// the user drags across text to underline and select it, then taps Next →
/// to proceed to the destination/metadata sheet.
///
/// Layout:
///   ┌─ close/title bar ─────────────────────┐
///   │  photo canvas (drag-to-select,        │
///   │               pinch-to-zoom)          │
///   │                                       │
///   │                   [ Next → ]          │
///   └───────────────────────────────────────┘
struct CapturePageView: View {
    @Bindable var store: CaptureStore
    let onDismiss: () -> Void

    @Environment(HighlighterStore.self) var appStore

    enum CropDragMode: Equatable {
        case move
        case minXMinY
        case minXMaxY
        case maxXMinY
        case maxXMaxY
    }

    struct ActiveCropDrag {
        let mode: CropDragMode
        let startCropBox: CGRect
        let startPoint: CGPoint
    }

    // Drag-select state
    @State var sortedLines: [OCRLine] = []
    @State var selectableWords: [OCRWord] = []
    @State var selectionRange: ClosedRange<Int>? = nil
    @State var activeCropDrag: ActiveCropDrag?

    // Spring-in animation
    @State var imageScale: CGFloat = 0.88
    @State var imageOpacity: Double = 0

    // Zoom / pan state — committed values
    @State var zoomScale: CGFloat = 1.0
    @State var zoomOffset: CGSize = .zero
    // Active values (updated live during gesture)
    @State var activeZoomScale: CGFloat = 1.0
    @State var activeZoomOffset: CGSize = .zero

    // Tracks whether a magnify gesture is in progress to suppress one-finger selection
    @GestureState var isMagnifying: Bool = false

    // Metadata sheet
    @State var showMetadataSheet = false

    var body: some View {
        content
            .sheet(isPresented: $showMetadataSheet) {
                CaptureMetadataSheet(store: store, onPublish: {
                    showMetadataSheet = false
                    store.publish()
                })
                .environment(appStore)
            }
            .overlay { publishingOverlay }
    }

    var content: some View {
        ZStack(alignment: .bottom) {
            Color.black.ignoresSafeArea()

            VStack(spacing: 0) {
                titleBar
                photoCanvas
            }
            .ignoresSafeArea(edges: .bottom)

            bottomControls
                .padding(.bottom, 48)
                .padding(.horizontal, 20)
        }
        .onAppear { setupLines() }
        .onAppear { triggerSpringIfReady() }
        .onChange(of: store.ocrLines) { _, lines in rebuildSelectionTargets(from: lines) }
        .onChange(of: store.thumbnail) { old, new in
            guard new != nil else { return }
            if old == nil {
                springIn()
            } else {
                imageScale = 1.0
                imageOpacity = 1.0
            }
            zoomScale = 1.0
            zoomOffset = .zero
            activeZoomScale = 1.0
            activeZoomOffset = .zero
        }
    }

    func setupLines() {
        rebuildSelectionTargets(from: store.ocrLines)
    }

    func rebuildSelectionTargets(from lines: [OCRLine]) {
        sortedLines = lines.sorted { lhs, rhs in
            if abs(lhs.bbox.midY - rhs.bbox.midY) < 0.006 {
                return lhs.bbox.minX < rhs.bbox.minX
            }
            return lhs.bbox.midY > rhs.bbox.midY
        }
        selectableWords = sortedLines.flatMap { line -> [OCRWord] in
            let words = line.words.filter { !$0.text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }
            if words.isEmpty {
                return [OCRWord(text: line.text, bbox: line.bbox, confidence: line.confidence)]
            }
            return words.sorted { $0.bbox.minX < $1.bbox.minX }
        }
    }

    func triggerSpringIfReady() {
        if store.thumbnail != nil { springIn() }
    }

    func springIn() {
        withAnimation(.spring(response: 0.45, dampingFraction: 0.72)) {
            imageScale = 1.0
            imageOpacity = 1.0
        }
    }

    @ViewBuilder
    var publishingOverlay: some View {
        if store.phase == .publishing {
            ZStack {
                Color.black.opacity(0.35).ignoresSafeArea()
                VStack(spacing: 8) {
                    ProgressView().tint(.white)
                    Text("Publishing…").font(.footnote).foregroundStyle(.white)
                }
            }
            .transition(.opacity)
        }
    }

    // MARK: - Title bar

    var titleBar: some View {
        HStack {
            Button(action: onDismiss) {
                Image(systemName: "xmark")
                    .font(.body.weight(.medium))
                    .foregroundStyle(.white)
                    .frame(width: 36, height: 36)
                    .background(.ultraThinMaterial, in: Circle())
            }
            Spacer()
            if store.phase == .processing {
                HStack(spacing: 6) {
                    ProgressView().scaleEffect(0.7).tint(.white)
                    Text("Reading the page…")
                        .font(.footnote.weight(.medium))
                        .foregroundStyle(.white.opacity(0.8))
                }
            } else if store.stashedQuote != nil {
                Text("Highlight ready")
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(.white)
            } else if !sortedLines.isEmpty {
                Text("Drag to select")
                    .font(.subheadline)
                    .foregroundStyle(.white.opacity(0.7))
            }
            Spacer()
            // Zoom reset — visible only when zoomed
            if zoomScale > 1.01 {
                Button {
                    withAnimation(.spring(response: 0.35, dampingFraction: 0.75)) {
                        zoomScale = 1.0
                        zoomOffset = .zero
                        activeZoomScale = 1.0
                        activeZoomOffset = .zero
                    }
                } label: {
                    Image(systemName: "arrow.up.left.and.arrow.down.right")
                        .font(.caption.weight(.medium))
                        .foregroundStyle(.white)
                        .frame(width: 36, height: 36)
                        .background(.ultraThinMaterial, in: Circle())
                }
            } else {
                Color.clear.frame(width: 36, height: 36)
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
        .background(
            LinearGradient(
                colors: [.black.opacity(0.6), .clear],
                startPoint: .top,
                endPoint: .bottom
            )
            .ignoresSafeArea(edges: .top)
        )
    }

    // MARK: - Next button

    var bottomControls: some View {
        HStack(spacing: 12) {
            if store.stashedQuote != nil {
                resetButton
            }
            Spacer(minLength: 0)
            nextButton
        }
    }

    var resetButton: some View {
        Button {
            clearHighlightSelection()
        } label: {
            Label("Reset", systemImage: "arrow.counterclockwise")
                .font(.body.weight(.semibold))
                .foregroundStyle(.white)
                .padding(.horizontal, 18)
                .padding(.vertical, 14)
                .background(.ultraThinMaterial, in: Capsule())
        }
        .buttonStyle(.plain)
        .accessibilityLabel("Reset highlight")
    }

    var nextButton: some View {
        Button {
            showMetadataSheet = true
        } label: {
            HStack(spacing: 8) {
                Image(systemName: store.stashedQuote != nil ? "highlighter" : "photo")
                Text("Next")
                    .fontWeight(.semibold)
                Image(systemName: "arrow.right")
            }
            .font(.body)
            .foregroundStyle(.white)
            .padding(.horizontal, 24)
            .padding(.vertical, 14)
            .background(
                store.canPublish ? Color.highlighterAccent : Color.black.opacity(0.55),
                in: Capsule()
            )
            .overlay(
                Capsule()
                    .stroke(Color.white.opacity(store.canPublish ? 0 : 0.25), lineWidth: 1)
            )
        }
    }

    // MARK: - Photo canvas

    @ViewBuilder
    var photoCanvas: some View {
        if store.phase == .processing && store.thumbnail == nil {
            loadingState
        } else if let thumbnail = store.thumbnail {
            GeometryReader { geo in
                let (dispSize, dispOffset) = computeLayout(thumbnail: thumbnail, container: geo.size)

                ZStack(alignment: .topLeading) {
                    Color.black

                    Image(uiImage: thumbnail)
                        .resizable()
                        .scaledToFit()
                        .frame(width: dispSize.width, height: dispSize.height)
                        .offset(x: dispOffset.x, y: dispOffset.y)
                        .scaleEffect(imageScale)
                        .opacity(imageOpacity)
                        .scaleEffect(activeZoomScale, anchor: .center)
                        .offset(activeZoomOffset)

                    // OCR + crop overlay — follows the same zoom/pan transform.
                    if !selectableWords.isEmpty {
                        Canvas { ctx, _ in
                            drawSelectionOverlay(ctx: ctx, dispSize: dispSize, dispOffset: dispOffset)
                            drawCropOverlay(ctx: ctx, dispSize: dispSize, dispOffset: dispOffset)
                        }
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                        .contentShape(Rectangle())
                        .scaleEffect(activeZoomScale, anchor: .center)
                        .offset(activeZoomOffset)
                        .gesture(
                            isMagnifying ? nil : canvasInteractionGesture(
                                containerSize: geo.size,
                                dispSize: dispSize,
                                dispOffset: dispOffset
                            )
                        )
                    }

                }
                .gesture(zoomGesture(containerSize: geo.size))
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        } else {
            emptyState
        }
    }

    var loadingState: some View {
        VStack(spacing: 12) {
            ProgressView().tint(.white)
            Text("Reading the page…")
                .font(.footnote)
                .foregroundStyle(.white.opacity(0.7))
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    var emptyState: some View {
        VStack(spacing: 12) {
            Image(systemName: "text.viewfinder")
                .font(.system(size: 40, weight: .light))
                .foregroundStyle(.white.opacity(0.5))
            Text("No text recognized")
                .font(.headline)
                .foregroundStyle(.white)
            Text("Drag to add a note, or share as a photo.")
                .font(.footnote)
                .foregroundStyle(.white.opacity(0.6))
                .multilineTextAlignment(.center)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

}
