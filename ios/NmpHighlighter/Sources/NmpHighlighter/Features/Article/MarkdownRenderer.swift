import Foundation
import UIKit
import Markdown

/// Converts raw NIP-23 markdown into an `NSAttributedString` suitable for a
/// `UITextView`, plus a mapping of highlight-run ranges back to the
/// `HighlightRecord` they came from.
///
/// Footnote syntax (`[^id]` + `[^id]: …`) is pre-processed out of the body
/// before parsing; references are rendered as superscript, tappable runs with
/// a `highlighter://footnote/<id>` URL. Footnote definitions are rendered as
/// a separate attributed string the reader appends below the body.
///
/// Highlight overlay: any `HighlightRecord` whose `quote` matches a range of
/// flattened body text receives an `.highlighterHighlight` custom attribute
/// holding the event id. The reader uses this to resolve taps without a
/// separate hit-test pass.
enum MarkdownRenderer {
    enum BodySegment: @unchecked Sendable {
        case text(NSAttributedString)
        case image(url: URL, alt: String)
        /// A standalone `nostr:` entity paragraph — rendered as a SwiftUI card
        /// by the reader rather than inside the UITextView.
        case nostrEntity(NostrEntityRef)
    }

    struct Output: @unchecked Sendable {
        let segments: [BodySegment]
        let footnotes: NSAttributedString
        /// Keyed by highlight event id so the reader can resolve a tap back
        /// to the record.
        let highlightsById: [String: HighlightRecord]
        /// Footnote number → character range in `body`, so taps on "^[1]"
        /// back-references in footnotes can flash the inline mark.
        let footnoteAnchors: [Int: NSRange]
    }

    /// Marker attribute the UITextView uses to recognize tapped runs. Value
    /// is the highlight event id.
    static let highlightAttribute = NSAttributedString.Key("highlighterHighlight")
    /// Marker attribute for footnote reference targets. Value is the footnote
    /// number (Int) so we can scroll to `.footnote-<n>` after a tap.
    static let footnoteReferenceAttribute = NSAttributedString.Key("highlighterFootnoteRef")
    /// Marker attribute for footnote back-reference targets in the definition
    /// block. Value is the footnote number (Int) so tapping the back-arrow
    /// can flash the inline reference.
    static let footnoteBackAttribute = NSAttributedString.Key("highlighterFootnoteBack")

    /// Render a full article body. Pure function — safe to call off the main
    /// thread (`UIFont` / `NSParagraphStyle` are thread-safe for construction).
    static func render(
        content: String,
        highlights: [HighlightRecord],
        accent: UIColor,
        tint: UIColor,
        ink: UIColor,
        muted: UIColor,
        bodyPointSize: CGFloat = 18,
        nostrDecoder: (@Sendable (String) -> NostrEntityRef?)? = nil,
        profileNames: [String: String] = [:]
    ) -> Output {
        let preprocessed = FootnotePreprocessor.extract(content)
        let document = Document(parsing: preprocessed.cleanedMarkdown)

        var walker = BodyWalker(
            accent: accent,
            tint: tint,
            ink: ink,
            muted: muted,
            bodyPointSize: bodyPointSize,
            definitionsById: Dictionary(uniqueKeysWithValues: preprocessed.definitions.map { ($0.id, $0) }),
            nostrDecoder: nostrDecoder,
            profileNames: profileNames
        )
        let rawSegments = walker.render(document)

        // Overlay highlights on each text segment. A highlight is applied to
        // whichever segment contains the matching text run; unmatched
        // highlights are silently dropped.
        var highlightsById: [String: HighlightRecord] = [:]
        let segments: [BodySegment] = rawSegments.map { segment in
            guard case .text(let attrStr) = segment else { return segment }
            let mutable = attrStr.mutableCopy() as! NSMutableAttributedString
            for highlight in highlights {
                let quote = highlight.quote.trimmingCharacters(in: .whitespacesAndNewlines)
                guard !quote.isEmpty, quote.count >= 4 else { continue }
                let plain = mutable.string
                if let range = plain.range(of: quote) {
                    let nsRange = NSRange(range, in: plain)
                    mutable.addAttribute(highlightAttribute, value: highlight.eventId, range: nsRange)
                    mutable.addAttribute(.backgroundColor, value: tint.withAlphaComponent(0.35), range: nsRange)
                    highlightsById[highlight.eventId] = highlight
                }
            }
            return .text(mutable)
        }

        // Footnote definitions — rendered separately as a smaller attributed
        // string. The reader appends this below the body with a divider.
        let footnotes = renderFootnotes(
            preprocessed.definitions,
            accent: accent,
            ink: ink,
            muted: muted,
            bodyPointSize: bodyPointSize
        )

        return Output(
            segments: segments,
            footnotes: footnotes,
            highlightsById: highlightsById,
            footnoteAnchors: walker.footnoteAnchors
        )
    }

    // MARK: - Footnote block rendering

    private static func renderFootnotes(
        _ defs: [FootnotePreprocessor.Definition],
        accent: UIColor,
        ink: UIColor,
        muted: UIColor,
        bodyPointSize: CGFloat
    ) -> NSAttributedString {
        guard !defs.isEmpty else { return NSAttributedString() }

        let out = NSMutableAttributedString()
        let smallSize = max(14, bodyPointSize - 3)

        for def in defs {
            // Leading number + back-arrow.
            let numberPara = NSMutableParagraphStyle()
            numberPara.paragraphSpacing = 10
            numberPara.lineHeightMultiple = 1.3

            let header = NSMutableAttributedString(
                string: "\(def.number). ",
                attributes: [
                    .font: UIFont.systemFont(ofSize: smallSize, weight: .semibold),
                    .foregroundColor: ink,
                    .paragraphStyle: numberPara
                ]
            )
            out.append(header)

            // Body — parse the definition itself as markdown so nested
            // inlines (emphasis, links, code) render too. Reuse BodyWalker
            // with a smaller point size.
            var inner = BodyWalker(
                accent: accent,
                tint: .clear,
                ink: muted,
                muted: muted,
                bodyPointSize: smallSize,
                definitionsById: [:],
                nostrDecoder: nil,
                profileNames: [:]
            )
            let innerDoc = Document(parsing: def.markdown)
            let innerSegments = inner.render(innerDoc)
            let innerMerged = NSMutableAttributedString()
            for seg in innerSegments { if case .text(let t) = seg { innerMerged.append(t) } }
            let innerString = innerMerged
            // Strip the trailing newline BodyWalker appends after the last
            // block — we want one newline between definitions, not two.
            if innerString.string.hasSuffix("\n\n") {
                innerString.deleteCharacters(in: NSRange(location: innerString.length - 1, length: 1))
            }
            out.append(innerString)

            // Back-arrow — tappable.
            let back = NSAttributedString(
                string: " ↩",
                attributes: [
                    .font: UIFont.systemFont(ofSize: smallSize),
                    .foregroundColor: accent,
                    footnoteBackAttribute: def.number,
                    .link: URL(string: "highlighter://footnote-back/\(def.number)")!
                ]
            )
            out.append(back)
            out.append(NSAttributedString(string: "\n"))
        }

        return out
    }
}