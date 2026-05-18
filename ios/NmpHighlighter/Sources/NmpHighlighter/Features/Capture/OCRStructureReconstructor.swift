import CoreGraphics
import Foundation

/// A single recognized word from Vision. `bbox` is in normalized image
/// coordinates, origin bottom-left.
struct OCRWord: Sendable, Equatable {
    let text: String
    let bbox: CGRect
    let confidence: Float
}

/// A single recognized line from Vision with the geometry we need to
/// reconstruct page structure. `bbox` is in normalized image coordinates,
/// origin bottom-left (matches `VNRecognizedTextObservation.boundingBox`).
struct OCRLine: Sendable, Equatable {
    let text: String
    let bbox: CGRect
    let confidence: Float
    let words: [OCRWord]

    init(text: String, bbox: CGRect, confidence: Float, words: [OCRWord] = []) {
        self.text = text
        self.bbox = bbox
        self.confidence = confidence
        self.words = words
    }
}

/// Turns Vision's line-by-line output into structured markdown that respects
/// the original page's paragraphs, headings, lists, and block quotes.
///
/// Vision returns observations roughly in reading order with bounding boxes;
/// a naive `joined(separator: "\n")` shreds paragraphs because every visual
/// line becomes a hard break. This reconstructor uses line geometry (vertical
/// gap, indentation, size relative to body) to emit coherent paragraphs and
/// headings the markdown renderer can then typeset beautifully.
///
/// All heuristics are ratio-based against per-page statistics so they work
/// across different zoom levels, devices, and book sizes.
enum OCRStructureReconstructor {
    static func toMarkdown(_ lines: [OCRLine]) -> String {
        guard !lines.isEmpty else { return "" }

        let normalized = lines.map { line in
            OCRLine(
                text: normalize(line.text),
                bbox: line.bbox,
                confidence: line.confidence
            )
        }.filter { !$0.text.isEmpty }

        guard !normalized.isEmpty else { return "" }

        let ordered = readingOrder(normalized)
        let stats = PageStats(lines: ordered)
        let trimmed = stripRunningHeadersAndFooters(ordered, stats: stats)
        return assembleMarkdown(trimmed, stats: stats)
    }

    // MARK: - Normalization

    private static func normalize(_ raw: String) -> String {
        var s = raw
        // Common OCR ligature artifacts that Vision sometimes preserves.
        let ligatures: [(String, String)] = [
            ("\u{FB00}", "ff"), ("\u{FB01}", "fi"), ("\u{FB02}", "fl"),
            ("\u{FB03}", "ffi"), ("\u{FB04}", "ffl")
        ]
        for (from, to) in ligatures { s = s.replacingOccurrences(of: from, with: to) }
        // Strip zero-width characters that can sneak in.
        s = s.replacingOccurrences(of: "\u{200B}", with: "")
        return s.trimmingCharacters(in: .whitespaces)
    }

    // MARK: - Reading order (two-column aware)

    /// Clusters lines by horizontal position; if two dense x-clusters exist,
    /// the left cluster is read top-to-bottom before the right cluster.
    /// Otherwise falls back to a pure top-to-bottom sort.
    private static func readingOrder(_ lines: [OCRLine]) -> [OCRLine] {
        let minX = lines.map { $0.bbox.minX }.sorted()
        guard let lo = minX.first, let hi = minX.last else { return lines }
        let spread = hi - lo

        // 1D two-means on minX: split at midpoint, iterate a few times.
        if spread > 0.25 {
            var split = (lo + hi) / 2
            for _ in 0..<6 {
                let left = minX.filter { $0 < split }
                let right = minX.filter { $0 >= split }
                guard !left.isEmpty, !right.isEmpty else { break }
                let lm = left.reduce(0, +) / Double(left.count)
                let rm = right.reduce(0, +) / Double(right.count)
                split = (lm + rm) / 2
            }
            let left = lines.filter { $0.bbox.minX < split }
            let right = lines.filter { $0.bbox.minX >= split }
            // Each cluster must hold at least 25% of the lines to count as a
            // real column — otherwise we're just reading flush-right captions.
            let columnThreshold = Double(lines.count) * 0.25
            if Double(left.count) >= columnThreshold, Double(right.count) >= columnThreshold {
                let leftMaxX = left.map { $0.bbox.maxX }.max() ?? 0
                let rightMinX = right.map { $0.bbox.minX }.min() ?? 1
                // Real columns don't overlap horizontally.
                if leftMaxX <= rightMinX + 0.02 {
                    return sortTopDown(left) + sortTopDown(right)
                }
            }
        }

        return sortTopDown(lines)
    }

    private static func sortTopDown(_ lines: [OCRLine]) -> [OCRLine] {
        // Vision uses bottom-left origin, so higher y = higher on page.
        lines.sorted { lhs, rhs in
            if abs(lhs.bbox.midY - rhs.bbox.midY) < 0.006 {
                return lhs.bbox.minX < rhs.bbox.minX
            }
            return lhs.bbox.midY > rhs.bbox.midY
        }
    }

    // MARK: - Page stats

    private struct PageStats {
        let medianHeight: Double
        let medianGap: Double
        let bodyLeftEdge: Double
        let bodyRightEdge: Double
        let pageCenterX: Double

        init(lines: [OCRLine]) {
            let heights = lines.map { Double($0.bbox.height) }.sorted()
            medianHeight = heights[heights.count / 2]

            // Baseline gap between consecutive lines.
            var gaps: [Double] = []
            for i in 1..<lines.count {
                let gap = Double(lines[i - 1].bbox.minY - lines[i].bbox.maxY)
                if gap > 0 { gaps.append(gap) }
            }
            gaps.sort()
            medianGap = gaps.isEmpty ? medianHeight * 0.3 : gaps[gaps.count / 2]

            // Mode-style binning for margins — 5% bins keep it robust to
            // stray indented observations.
            let lefts = lines.map { Double($0.bbox.minX) }
            let rights = lines.map { Double($0.bbox.maxX) }
            bodyLeftEdge = modeBinned(lefts, binSize: 0.05)
            bodyRightEdge = modeBinned(rights, binSize: 0.05)
            pageCenterX = (bodyLeftEdge + bodyRightEdge) / 2
        }
    }

    private static func modeBinned(_ values: [Double], binSize: Double) -> Double {
        guard !values.isEmpty else { return 0 }
        var buckets: [Int: [Double]] = [:]
        for v in values {
            let bucket = Int(v / binSize)
            buckets[bucket, default: []].append(v)
        }
        let bestBucket = buckets.max(by: { $0.value.count < $1.value.count })!
        let bucketValues = bestBucket.value
        return bucketValues.reduce(0, +) / Double(bucketValues.count)
    }

    // MARK: - Header / footer stripping

    /// Drops lines at the very top or bottom of the page that look like
    /// running headers, page numbers, or folio marks — unless they look like
    /// a chapter title (larger than body text).
    private static func stripRunningHeadersAndFooters(_ lines: [OCRLine], stats: PageStats) -> [OCRLine] {
        lines.filter { line in
            let atTop = line.bbox.minY > 0.94
            let atBottom = line.bbox.maxY < 0.06
            guard atTop || atBottom else { return true }

            let heightRatio = Double(line.bbox.height) / stats.medianHeight
            // Large, centered line at the very top is a chapter opener — keep.
            if heightRatio > 1.2 { return true }

            let trimmed = line.text.trimmingCharacters(in: .whitespaces)
            // Bare numerals are always page numbers.
            if trimmed.range(of: "^\\d{1,4}$", options: .regularExpression) != nil {
                return false
            }
            let wordCount = trimmed.split(whereSeparator: { $0.isWhitespace }).count
            // Very short lines at the edges are near-certainly running heads.
            return wordCount > 5
        }
    }

    // MARK: - Assembly

    /// Walks the ordered lines and emits markdown, deciding at each boundary
    /// whether to soft-wrap (space), hard-break (paragraph), or promote to
    /// a heading / blockquote / list.
    private static func assembleMarkdown(_ lines: [OCRLine], stats: PageStats) -> String {
        guard !lines.isEmpty else { return "" }

        var out = ""
        var currentBlock = ""
        var currentKind: BlockKind = .body

        func flush() {
            let piece = currentBlock.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !piece.isEmpty else {
                currentBlock = ""
                return
            }
            switch currentKind {
            case .heading(let level):
                out += String(repeating: "#", count: level) + " " + piece + "\n\n"
            case .listItem(let ordered):
                out += (ordered ? "1. " : "- ") + piece + "\n"
            case .blockQuote:
                for para in piece.components(separatedBy: "\n\n") {
                    out += "> " + para.replacingOccurrences(of: "\n", with: "\n> ") + "\n\n"
                }
            case .body:
                out += piece + "\n\n"
            }
            currentBlock = ""
        }

        for (i, line) in lines.enumerated() {
            let classified = classify(line, stats: stats)

            // A classification change always flushes the previous block.
            if i == 0 {
                currentKind = classified.kind
                currentBlock = classified.text
                continue
            }

            let prev = lines[i - 1]
            let boundary = paragraphBoundary(prev: prev, curr: line, stats: stats)

            if classified.kind != currentKind || boundary == .hardBreak {
                flush()
                currentKind = classified.kind
                currentBlock = classified.text
            } else {
                // Soft-join inside the same block.
                let joined = softJoin(currentBlock, classified.text)
                currentBlock = joined
            }
        }
        flush()

        // Collapse any triple+ newlines left over from boundary quirks.
        while out.contains("\n\n\n") {
            out = out.replacingOccurrences(of: "\n\n\n", with: "\n\n")
        }
        return out.trimmingCharacters(in: .whitespacesAndNewlines) + "\n"
    }

    // MARK: - Block kinds & classification

    private enum BlockKind: Equatable {
        case body
        case heading(level: Int)
        case listItem(ordered: Bool)
        case blockQuote
    }

    private struct Classified {
        let kind: BlockKind
        let text: String
    }

    private static func classify(_ line: OCRLine, stats: PageStats) -> Classified {
        let text = line.text
        let heightRatio = Double(line.bbox.height) / stats.medianHeight
        let bodyWidth = max(stats.bodyRightEdge - stats.bodyLeftEdge, 0.0001)
        let indentRatio = (Double(line.bbox.minX) - stats.bodyLeftEdge) / bodyWidth
        let widthRatio = Double(line.bbox.width) / stats.medianHeight
        let centeredDeviation = abs(Double(line.bbox.midX) - stats.pageCenterX) / bodyWidth

        // List detection — check first so the bullet character doesn't end up
        // in the emitted body.
        if let stripped = stripListMarker(text) {
            return Classified(kind: .listItem(ordered: stripped.ordered), text: stripped.remainder)
        }

        // Heading: requires at least two corroborating signals so we don't
        // misclassify a random short line as a title.
        var headingSignals = 0
        if heightRatio > 1.25 { headingSignals += 1 }
        if centeredDeviation < 0.08, widthRatio > 1.5 { headingSignals += 1 }
        let wordCount = text.split(whereSeparator: { $0.isWhitespace }).count
        let terminator = text.last.map { ".!?".contains($0) } ?? false
        if wordCount < 8, !terminator { headingSignals += 1 }
        if text == text.uppercased(), wordCount > 0, text.contains(where: { $0.isLetter }) {
            headingSignals += 1
        }
        // Drop-cap paranoia: single-glyph observations are almost never headings.
        let isDropCap = widthRatio < 1.5 && text.count <= 2
        if headingSignals >= 2, !isDropCap {
            let level = heightRatio > 1.55 ? 1 : 2
            return Classified(kind: .heading(level: level), text: text)
        }

        // Blockquote — sustained left+right pulled in from body edges.
        let pulledLeft = indentRatio > 0.06
        let pulledRight = (stats.bodyRightEdge - Double(line.bbox.maxX)) / bodyWidth > 0.06
        if pulledLeft, pulledRight, heightRatio < 1.2 {
            return Classified(kind: .blockQuote, text: text)
        }

        return Classified(kind: .body, text: text)
    }

    private static func stripListMarker(_ text: String) -> (ordered: Bool, remainder: String)? {
        let trimmed = text.trimmingCharacters(in: .whitespaces)
        let bulletSet: Set<Character> = ["•", "·", "●", "○", "▪", "◦", "–", "—"]
        if let first = trimmed.first, bulletSet.contains(first) {
            let remainder = trimmed.dropFirst().trimmingCharacters(in: .whitespaces)
            if remainder.count > 2 { return (false, remainder) }
        }
        if let match = trimmed.range(of: "^\\d{1,2}[.)]\\s+", options: .regularExpression) {
            let remainder = String(trimmed[match.upperBound...])
            if !remainder.isEmpty { return (true, remainder) }
        }
        if trimmed.hasPrefix("- "), trimmed.count > 2 {
            return (false, String(trimmed.dropFirst(2)))
        }
        return nil
    }

    // MARK: - Paragraph boundary decisions

    private enum Boundary { case softWrap, hardBreak }

    /// Decides whether the break between `prev` and `curr` is a visual
    /// line-wrap (join with a space) or a real paragraph break (emit `\n\n`).
    private static func paragraphBoundary(prev: OCRLine, curr: OCRLine, stats: PageStats) -> Boundary {
        let gap = Double(prev.bbox.minY - curr.bbox.maxY)
        let gapRatio = gap / max(stats.medianHeight, 0.0001)
        let bodyWidth = max(stats.bodyRightEdge - stats.bodyLeftEdge, 0.0001)
        let indentRatio = (Double(curr.bbox.minX) - stats.bodyLeftEdge) / bodyWidth
        let prevShortRatio = (stats.bodyRightEdge - Double(prev.bbox.maxX)) / bodyWidth
        let prevEndsTerminal = prev.text.last.map { ".!?\"'".contains($0) } ?? false

        // Big vertical gap → paragraph.
        if gapRatio > 0.6 { return .hardBreak }

        // First-line indent on the next line → paragraph.
        if indentRatio > 0.04, gapRatio > 0.15 { return .hardBreak }

        // Previous line ends short and with terminal punctuation → paragraph.
        if prevShortRatio > 0.12, prevEndsTerminal, gapRatio > 0.2 { return .hardBreak }

        return .softWrap
    }

    /// Joins two fragments, handling end-of-line hyphenation. If the previous
    /// fragment ends in a soft hyphen inside a lowercase word, the hyphen is
    /// dropped and the tokens are fused; otherwise they're joined with a space.
    private static func softJoin(_ left: String, _ right: String) -> String {
        guard !left.isEmpty else { return right }
        guard !right.isEmpty else { return left }

        if left.hasSuffix("-") || left.hasSuffix("\u{2010}") || left.hasSuffix("\u{2011}") {
            let withoutHyphen = String(left.dropLast())
            let leftTail = withoutHyphen.last
            let rightHead = right.first
            let leftLower = leftTail.map { $0.isLowercase } ?? false
            let rightLower = rightHead.map { $0.isLowercase } ?? false
            // Only fuse when both halves are lowercase — keeps "Anglo-Saxon"
            // and dialog em-dashes intact.
            if leftLower, rightLower {
                return withoutHyphen + right
            }
        }

        return left + " " + right
    }
}
