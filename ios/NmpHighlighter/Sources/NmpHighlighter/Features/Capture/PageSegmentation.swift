import CoreGraphics
import Foundation

/// Detects whether a captured image actually contains two book pages
/// (an open book photographed end-to-end) and selects the dominant page
/// so the capture pipeline can auto-crop down to it.
///
/// `VNDocumentCameraViewController` happily returns the whole open book as
/// one rectangle, which means OCR pulls in text from the page the user
/// didn't intend to capture and the highlight selection ends up bleeding
/// across the gutter. The gutter is the widest vertical band of empty
/// space near the centre of the image, so we find it from OCR line
/// geometry and crop to whichever side has the most text.
enum PageSegmentation {
    /// Result of running detection on a set of OCR lines. `pageRect` is in
    /// Vision normalized coordinates (origin bottom-left), matching the
    /// coordinate space used elsewhere in the capture pipeline.
    struct Detection {
        let pageRect: CGRect
        let chosenSide: Side
    }

    enum Side {
        case left
        case right
    }

    /// Returns a crop rect for the dominant page when the OCR lines look
    /// like a two-page spread, or `nil` when the input is already a single
    /// page (or doesn't have enough text to decide confidently).
    static func detectActivePage(lines: [OCRLine]) -> Detection? {
        let usable = lines.filter { line in
            !line.bbox.isNull
                && !line.bbox.isEmpty
                && line.bbox.width > 0.02
                && line.bbox.height > 0.005
        }
        // Need a meaningful amount of text on the page to even attempt a
        // split — sparse OCR is too noisy to cluster reliably.
        guard usable.count >= 8 else { return nil }

        // Search for the gutter: an x-coordinate in the central band of
        // the image where lines fully to the left and lines fully to the
        // right both number at least a few, and the empty band between
        // the two clusters is wide.
        var bestGap: CGFloat = 0
        var bestSplit: (leftMaxX: CGFloat, rightMinX: CGFloat, gutter: CGFloat)?

        var probe: CGFloat = 0.30
        while probe <= 0.70 {
            let leftLines = usable.filter { $0.bbox.maxX < probe }
            let rightLines = usable.filter { $0.bbox.minX > probe }

            if leftLines.count >= 4, rightLines.count >= 4 {
                let leftMaxX = leftLines.map { $0.bbox.maxX }.max() ?? 0
                let rightMinX = rightLines.map { $0.bbox.minX }.min() ?? 1
                let gap = rightMinX - leftMaxX
                if gap > bestGap {
                    bestGap = gap
                    bestSplit = (leftMaxX, rightMinX, (leftMaxX + rightMinX) / 2)
                }
            }
            probe += 0.01
        }

        // Gutter must be a real gap, not just one column's normal margin.
        // 5% of image width is a conservative threshold — typical book
        // gutters in a phone capture are 6-12% wide.
        guard let split = bestSplit, bestGap >= 0.05 else { return nil }

        // Cluster every line by which side of the gutter centerline it
        // sits on, then pick the side with more total text area.
        let leftLines = usable.filter { $0.bbox.midX < split.gutter }
        let rightLines = usable.filter { $0.bbox.midX > split.gutter }

        let leftArea = leftLines.reduce(CGFloat(0)) { $0 + $1.bbox.width * $1.bbox.height }
        let rightArea = rightLines.reduce(CGFloat(0)) { $0 + $1.bbox.width * $1.bbox.height }
        let chosenIsRight = rightArea >= leftArea
        let chosenLines = chosenIsRight ? rightLines : leftLines

        // Need enough text on the chosen side for a meaningful crop.
        guard chosenLines.count >= 4 else { return nil }

        let chosenMinX = chosenLines.map { $0.bbox.minX }.min() ?? 0
        let chosenMaxX = chosenLines.map { $0.bbox.maxX }.max() ?? 1
        let chosenMinY = chosenLines.map { $0.bbox.minY }.min() ?? 0
        let chosenMaxY = chosenLines.map { $0.bbox.maxY }.max() ?? 1

        // Pad the crop. The gutter side gets a tight inset so we don't
        // bleed back across the spine; the outer side, top, and bottom
        // get a generous pad so headers, page numbers and trailing
        // punctuation aren't clipped.
        let outerPadX: CGFloat = 0.04
        let gutterPadX: CGFloat = 0.015
        let padY: CGFloat = 0.04

        let cropMinX: CGFloat
        let cropMaxX: CGFloat
        if chosenIsRight {
            cropMinX = max(0, min(chosenMinX, split.rightMinX) - gutterPadX)
            cropMaxX = min(1, chosenMaxX + outerPadX)
        } else {
            cropMinX = max(0, chosenMinX - outerPadX)
            cropMaxX = min(1, max(chosenMaxX, split.leftMaxX) + gutterPadX)
        }
        let cropMinY = max(0, chosenMinY - padY)
        let cropMaxY = min(1, chosenMaxY + padY)

        let rect = CGRect(
            x: cropMinX,
            y: cropMinY,
            width: cropMaxX - cropMinX,
            height: cropMaxY - cropMinY
        )

        // If the resulting crop keeps almost the whole image, the split
        // wasn't really there — bail rather than make a confusing
        // micro-crop.
        guard rect.width < 0.92, rect.width > 0.20, rect.height > 0.20 else {
            return nil
        }

        return Detection(
            pageRect: rect,
            chosenSide: chosenIsRight ? .right : .left
        )
    }

    /// Re-projects OCR lines onto a cropped page so their bboxes are
    /// normalized relative to the new image. Lines whose center falls
    /// outside the page rect are dropped (they belonged to the discarded
    /// page or to the gutter shadow).
    static func cropLines(_ lines: [OCRLine], to pageRect: CGRect) -> [OCRLine] {
        let pw = pageRect.width
        let ph = pageRect.height
        guard pw > 0, ph > 0 else { return lines }

        let unit = CGRect(x: 0, y: 0, width: 1, height: 1)

        return lines.compactMap { line -> OCRLine? in
            let center = CGPoint(x: line.bbox.midX, y: line.bbox.midY)
            guard pageRect.contains(center) else { return nil }

            let newBBox = CGRect(
                x: (line.bbox.minX - pageRect.minX) / pw,
                y: (line.bbox.minY - pageRect.minY) / ph,
                width: line.bbox.width / pw,
                height: line.bbox.height / ph
            ).intersection(unit)

            guard !newBBox.isNull, !newBBox.isEmpty else { return nil }

            let newWords = line.words.compactMap { word -> OCRWord? in
                let newWordBBox = CGRect(
                    x: (word.bbox.minX - pageRect.minX) / pw,
                    y: (word.bbox.minY - pageRect.minY) / ph,
                    width: word.bbox.width / pw,
                    height: word.bbox.height / ph
                ).intersection(unit)
                guard !newWordBBox.isNull, !newWordBBox.isEmpty else { return nil }
                return OCRWord(text: word.text, bbox: newWordBBox, confidence: word.confidence)
            }

            return OCRLine(
                text: line.text,
                bbox: newBBox,
                confidence: line.confidence,
                words: newWords
            )
        }
    }
}
