import SwiftUI
import UIKit

extension CapturePageView {
    // MARK: - Zoom gesture (two-finger pinch + pan)

    func zoomGesture(containerSize: CGSize) -> some Gesture {
        MagnifyGesture()
            .updating($isMagnifying) { _, state, _ in state = true }
            .onChanged { value in
                activeZoomScale = (zoomScale * value.magnification).clamped(to: 1.0...5.0)
            }
            .onEnded { value in
                let newScale = (zoomScale * value.magnification).clamped(to: 1.0...5.0)
                zoomScale = newScale
                activeZoomScale = newScale
                if newScale <= 1.0 {
                    zoomOffset = .zero
                    activeZoomOffset = .zero
                }
            }
            .simultaneously(with:
                DragGesture(minimumDistance: 0)
                    .onChanged { value in
                        guard activeZoomScale > 1.01 else { return }
                        let maxOffset = maxAllowedOffset(containerSize: containerSize, scale: activeZoomScale)
                        activeZoomOffset = CGSize(
                            width: (zoomOffset.width + value.translation.width).clamped(to: -maxOffset.width...maxOffset.width),
                            height: (zoomOffset.height + value.translation.height).clamped(to: -maxOffset.height...maxOffset.height)
                        )
                    }
                    .onEnded { _ in
                        guard activeZoomScale > 1.01 else { return }
                        zoomOffset = activeZoomOffset
                    }
            )
    }

    func maxAllowedOffset(containerSize: CGSize, scale: CGFloat) -> CGSize {
        CGSize(
            width: containerSize.width * (scale - 1) / 2,
            height: containerSize.height * (scale - 1) / 2
        )
    }

    // MARK: - OCR selection overlay drawing

    func drawSelectionOverlay(ctx: GraphicsContext, dispSize: CGSize, dispOffset: CGPoint) {
        guard let range = selectionRange else { return }
        for idx in range {
            guard idx < selectableWords.count else { continue }
            let word = selectableWords[idx]
            let rect = visionToScreen(word.bbox, size: dispSize, offset: dispOffset)
            let underline = CGRect(x: rect.minX, y: rect.maxY - 4, width: rect.width, height: 4)
            ctx.fill(Path(underline), with: .color(Color.yellow.opacity(0.85)))
            ctx.fill(Path(rect), with: .color(Color.yellow.opacity(0.25)))
        }
    }

    func drawCropOverlay(ctx: GraphicsContext, dispSize: CGSize, dispOffset: CGPoint) {
        guard store.stashedQuote != nil, let cropBox = store.highlightCropBox else { return }

        let imageRect = CGRect(origin: dispOffset, size: dispSize)
        let cropRect = visionToScreen(cropBox, size: dispSize, offset: dispOffset)
            .intersection(imageRect)
        guard !cropRect.isNull, !cropRect.isEmpty else { return }

        let dim = Color.black.opacity(0.34)
        let outsideRects = [
            CGRect(x: imageRect.minX, y: imageRect.minY, width: imageRect.width, height: cropRect.minY - imageRect.minY),
            CGRect(x: imageRect.minX, y: cropRect.maxY, width: imageRect.width, height: imageRect.maxY - cropRect.maxY),
            CGRect(x: imageRect.minX, y: cropRect.minY, width: cropRect.minX - imageRect.minX, height: cropRect.height),
            CGRect(x: cropRect.maxX, y: cropRect.minY, width: imageRect.maxX - cropRect.maxX, height: cropRect.height)
        ]
        for rect in outsideRects where rect.width > 0 && rect.height > 0 {
            ctx.fill(Path(rect), with: .color(dim))
        }

        var grid = Path()
        for fraction in [CGFloat(1.0 / 3.0), CGFloat(2.0 / 3.0)] {
            let x = cropRect.minX + cropRect.width * fraction
            grid.move(to: CGPoint(x: x, y: cropRect.minY))
            grid.addLine(to: CGPoint(x: x, y: cropRect.maxY))

            let y = cropRect.minY + cropRect.height * fraction
            grid.move(to: CGPoint(x: cropRect.minX, y: y))
            grid.addLine(to: CGPoint(x: cropRect.maxX, y: y))
        }
        ctx.stroke(grid, with: .color(Color.white.opacity(0.55)), lineWidth: 1)
        ctx.stroke(Path(cropRect), with: .color(Color.white.opacity(0.95)), lineWidth: 2)

        let handleSize: CGFloat = 12
        for point in [
            CGPoint(x: cropRect.minX, y: cropRect.minY),
            CGPoint(x: cropRect.maxX, y: cropRect.minY),
            CGPoint(x: cropRect.minX, y: cropRect.maxY),
            CGPoint(x: cropRect.maxX, y: cropRect.maxY)
        ] {
            let handle = CGRect(
                x: point.x - handleSize / 2,
                y: point.y - handleSize / 2,
                width: handleSize,
                height: handleSize
            )
            ctx.fill(Path(ellipseIn: handle), with: .color(.white))
            ctx.stroke(Path(ellipseIn: handle), with: .color(Color.black.opacity(0.25)), lineWidth: 1)
        }
    }

    // MARK: - One-finger selection gesture

    func canvasInteractionGesture(
        containerSize: CGSize,
        dispSize: CGSize,
        dispOffset: CGPoint
    ) -> some Gesture {
        DragGesture(minimumDistance: 0)
            .onChanged { value in
                if store.stashedQuote != nil {
                    updateCropBox(
                        start: value.startLocation,
                        current: value.location,
                        containerSize: containerSize,
                        dispSize: dispSize,
                        dispOffset: dispOffset
                    )
                } else {
                    updateSelection(
                        start: value.startLocation,
                        current: value.location,
                        containerSize: containerSize,
                        dispSize: dispSize,
                        dispOffset: dispOffset
                    )
                }
            }
            .onEnded { _ in
                if store.stashedQuote != nil {
                    finishCropAdjustment()
                } else {
                    commitSelection()
                }
            }
    }

    func updateSelection(
        start: CGPoint, current: CGPoint,
        containerSize: CGSize,
        dispSize: CGSize, dispOffset: CGPoint
    ) {
        guard !selectableWords.isEmpty else { return }
        let vStart = screenToVision(start, containerSize: containerSize, dispSize: dispSize, dispOffset: dispOffset)
        let vCurrent = screenToVision(current, containerSize: containerSize, dispSize: dispSize, dispOffset: dispOffset)

        let anchor = nearestWordIndex(to: vStart)
        let cursor = nearestWordIndex(to: vCurrent)

        selectionRange = min(anchor, cursor)...max(anchor, cursor)
    }

    func updateCropBox(
        start: CGPoint,
        current: CGPoint,
        containerSize: CGSize,
        dispSize: CGSize,
        dispOffset: CGPoint
    ) {
        guard let cropBox = store.highlightCropBox else { return }
        let vStart = screenToVision(start, containerSize: containerSize, dispSize: dispSize, dispOffset: dispOffset)
        let vCurrent = screenToVision(current, containerSize: containerSize, dispSize: dispSize, dispOffset: dispOffset)

        if activeCropDrag == nil {
            let mode = cropDragMode(at: start, cropBox: cropBox, dispSize: dispSize, dispOffset: dispOffset)
            activeCropDrag = ActiveCropDrag(mode: mode, startCropBox: cropBox, startPoint: vStart)
        }
        guard let drag = activeCropDrag else { return }

        let delta = CGPoint(x: vCurrent.x - drag.startPoint.x, y: vCurrent.y - drag.startPoint.y)
        store.updateHighlightCropBox(adjustedCropBox(from: drag.startCropBox, mode: drag.mode, delta: delta), reupload: false)
    }

    func finishCropAdjustment() {
        activeCropDrag = nil
        guard let cropBox = store.highlightCropBox else { return }
        store.updateHighlightCropBox(cropBox, reupload: true)
    }

    func commitSelection() {
        guard let range = selectionRange, !selectableWords.isEmpty else {
            selectionRange = nil
            return
        }
        let selected = Array(selectableWords[range])
        let quote = joinedQuote(from: selected)
            .trimmingCharacters(in: .whitespacesAndNewlines)

        // Keep selectionRange so the yellow highlight stays visible.
        guard !quote.isEmpty else { return }
        store.stashHighlight(quote: quote, context: "", selectedBoxes: selected.map(\.bbox))
    }

    func clearHighlightSelection() {
        selectionRange = nil
        store.clearStash()
        activeCropDrag = nil
    }

    func nearestWordIndex(to pt: CGPoint) -> Int {
        selectableWords.indices.min(by: {
            pointToBBoxDistance(pt, bbox: selectableWords[$0].bbox)
                < pointToBBoxDistance(pt, bbox: selectableWords[$1].bbox)
        }) ?? 0
    }

    func joinedQuote(from words: [OCRWord]) -> String {
        words.map(\.text)
            .joined(separator: " ")
            .replacingOccurrences(of: " ,", with: ",")
            .replacingOccurrences(of: " .", with: ".")
            .replacingOccurrences(of: " ;", with: ";")
            .replacingOccurrences(of: " :", with: ":")
            .replacingOccurrences(of: " !", with: "!")
            .replacingOccurrences(of: " ?", with: "?")
    }

    func cropDragMode(
        at point: CGPoint,
        cropBox: CGRect,
        dispSize: CGSize,
        dispOffset: CGPoint
    ) -> CropDragMode {
        let rect = visionToScreen(cropBox, size: dispSize, offset: dispOffset)
        let threshold: CGFloat = 34
        let corners: [(CGPoint, CropDragMode)] = [
            (CGPoint(x: rect.minX, y: rect.minY), .minXMaxY),
            (CGPoint(x: rect.maxX, y: rect.minY), .maxXMaxY),
            (CGPoint(x: rect.minX, y: rect.maxY), .minXMinY),
            (CGPoint(x: rect.maxX, y: rect.maxY), .maxXMinY)
        ]
        if let nearest = corners.min(by: { distance(point, $0.0) < distance(point, $1.0) }),
           distance(point, nearest.0) <= threshold {
            return nearest.1
        }
        return .move
    }

    func adjustedCropBox(from start: CGRect, mode: CropDragMode, delta: CGPoint) -> CGRect {
        let minSize: CGFloat = 0.08

        if mode == .move {
            let x = (start.minX + delta.x).clamped(to: 0...(1 - start.width))
            let y = (start.minY + delta.y).clamped(to: 0...(1 - start.height))
            return CGRect(x: x, y: y, width: start.width, height: start.height)
        }

        var minX = start.minX
        var minY = start.minY
        var maxX = start.maxX
        var maxY = start.maxY

        switch mode {
        case .move:
            break
        case .minXMinY:
            minX = (start.minX + delta.x).clamped(to: 0...max(0, start.maxX - minSize))
            minY = (start.minY + delta.y).clamped(to: 0...max(0, start.maxY - minSize))
        case .minXMaxY:
            minX = (start.minX + delta.x).clamped(to: 0...max(0, start.maxX - minSize))
            maxY = (start.maxY + delta.y).clamped(to: min(1, start.minY + minSize)...1)
        case .maxXMinY:
            maxX = (start.maxX + delta.x).clamped(to: min(1, start.minX + minSize)...1)
            minY = (start.minY + delta.y).clamped(to: 0...max(0, start.maxY - minSize))
        case .maxXMaxY:
            maxX = (start.maxX + delta.x).clamped(to: min(1, start.minX + minSize)...1)
            maxY = (start.maxY + delta.y).clamped(to: min(1, start.minY + minSize)...1)
        }

        return CGRect(x: minX, y: minY, width: maxX - minX, height: maxY - minY)
    }

    func pointToBBoxDistance(_ pt: CGPoint, bbox: CGRect) -> CGFloat {
        let cx = min(max(pt.x, bbox.minX), bbox.maxX)
        let cy = min(max(pt.y, bbox.minY), bbox.maxY)
        return sqrt((pt.x - cx) * (pt.x - cx) + (pt.y - cy) * (pt.y - cy))
    }

    func distance(_ lhs: CGPoint, _ rhs: CGPoint) -> CGFloat {
        sqrt((lhs.x - rhs.x) * (lhs.x - rhs.x) + (lhs.y - rhs.y) * (lhs.y - rhs.y))
    }

    // MARK: - Coordinate helpers

    func computeLayout(thumbnail: UIImage, container: CGSize) -> (size: CGSize, offset: CGPoint) {
        let iw = thumbnail.size.width
        let ih = thumbnail.size.height
        guard iw > 0, ih > 0 else { return (.zero, .zero) }
        let imageAR = iw / ih
        let containerAR = container.width / container.height

        let dispSize: CGSize
        if imageAR > containerAR {
            dispSize = CGSize(width: container.width, height: container.width / imageAR)
        } else {
            dispSize = CGSize(width: container.height * imageAR, height: container.height)
        }
        let offset = CGPoint(
            x: (container.width - dispSize.width) / 2,
            y: (container.height - dispSize.height) / 2
        )
        return (dispSize, offset)
    }

    /// Vision normalized coords (bottom-left origin) → screen rect.
    func visionToScreen(_ bbox: CGRect, size: CGSize, offset: CGPoint) -> CGRect {
        CGRect(
            x: offset.x + bbox.minX * size.width,
            y: offset.y + (1.0 - bbox.maxY) * size.height,
            width: bbox.width * size.width,
            height: bbox.height * size.height
        )
    }

    /// Screen point → Vision normalized coords (bottom-left origin).
    /// Inverts the active zoom/pan transform so a touch on the zoomed canvas
    /// maps back to the correct image-space coordinate.
    func screenToVision(
        _ pt: CGPoint,
        containerSize: CGSize,
        dispSize: CGSize,
        dispOffset: CGPoint
    ) -> CGPoint {
        let cx = containerSize.width / 2
        let cy = containerSize.height / 2
        // Undo pan, then undo scale (pivot = container center)
        let unscaled = CGPoint(
            x: (pt.x - activeZoomOffset.width - cx) / activeZoomScale + cx,
            y: (pt.y - activeZoomOffset.height - cy) / activeZoomScale + cy
        )
        return CGPoint(
            x: (unscaled.x - dispOffset.x) / dispSize.width,
            y: 1.0 - (unscaled.y - dispOffset.y) / dispSize.height
        )
    }

}

// MARK: - Comparable clamped

}

