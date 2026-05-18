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

// MARK: - BodyWalker

/// Walks a `swift-markdown` `Document` and emits an `NSAttributedString`.
/// Mutates as it goes; call `render(_:)` once per document.
private struct BodyWalker {
    let accent: UIColor
    let tint: UIColor
    let ink: UIColor
    let muted: UIColor
    let bodyPointSize: CGFloat
    let definitionsById: [String: FootnotePreprocessor.Definition]
    let nostrDecoder: (@Sendable (String) -> NostrEntityRef?)?
    let profileNames: [String: String]

    var footnoteAnchors: [Int: NSRange] = [:]

    // Cached fonts — `UIFontMetrics` scaling is handled at the text-view level.
    private var serif: UIFont { UIFont(descriptor: UIFontDescriptor.preferredFontDescriptor(withTextStyle: .body).withDesign(.serif) ?? UIFontDescriptor.preferredFontDescriptor(withTextStyle: .body), size: bodyPointSize) }
    private var serifItalic: UIFont {
        let d = UIFontDescriptor.preferredFontDescriptor(withTextStyle: .body)
            .withDesign(.serif)?
            .withSymbolicTraits(.traitItalic)
            ?? UIFontDescriptor.preferredFontDescriptor(withTextStyle: .body)
        return UIFont(descriptor: d, size: bodyPointSize)
    }
    private var serifBold: UIFont {
        let d = UIFontDescriptor.preferredFontDescriptor(withTextStyle: .body)
            .withDesign(.serif)?
            .withSymbolicTraits(.traitBold)
            ?? UIFontDescriptor.preferredFontDescriptor(withTextStyle: .body)
        return UIFont(descriptor: d, size: bodyPointSize)
    }
    private var mono: UIFont { UIFont.monospacedSystemFont(ofSize: bodyPointSize - 2, weight: .regular) }

    mutating func render(_ document: Document) -> [MarkdownRenderer.BodySegment] {
        var segments: [MarkdownRenderer.BodySegment] = []
        var currentText = NSMutableAttributedString()

        func flush() {
            if currentText.length > 0 {
                segments.append(.text(currentText))
                currentText = NSMutableAttributedString()
            }
        }

        for child in document.children {
            if let para = child as? Paragraph, let (url, alt) = imageOnlyParagraph(para) {
                flush()
                segments.append(.image(url: url, alt: alt))
            } else if let para = child as? Paragraph, let ref = nostrOnlyParagraph(para) {
                flush()
                segments.append(.nostrEntity(ref))
            } else {
                currentText.append(renderBlock(child))
            }
        }
        flush()
        return segments
    }

    private func nostrOnlyParagraph(_ para: Paragraph) -> NostrEntityRef? {
        guard let decoder = nostrDecoder else { return nil }
        let children = Array(para.inlineChildren)
        guard children.count == 1, let textNode = children.first as? Markdown.Text else { return nil }
        let raw = textNode.string.trimmingCharacters(in: .whitespacesAndNewlines)
        guard raw.lowercased().hasPrefix("nostr:") else { return nil }
        let body = raw.dropFirst("nostr:".count)
        let bech32 = String(body.prefix(while: {
            guard let sc = $0.unicodeScalars.first, $0.unicodeScalars.count == 1 else { return false }
            let v = sc.value
            return (0x30...0x39).contains(v) || (0x61...0x7A).contains(v)
        }))
        let lower = bech32.lowercased()
        guard lower.hasPrefix("npub1") || lower.hasPrefix("nprofile1")
            || lower.hasPrefix("note1") || lower.hasPrefix("nevent1") || lower.hasPrefix("naddr1")
        else { return nil }
        return decoder(bech32)
    }

    private func imageOnlyParagraph(_ para: Paragraph) -> (URL, String)? {
        let children = Array(para.inlineChildren)
        guard children.count == 1,
              let img = children.first as? Image,
              let src = img.source,
              let url = URL(string: src) else { return nil }
        return (url, img.plainText)
    }

    // MARK: - Block

    private mutating func renderBlock(_ markup: Markup) -> NSAttributedString {
        switch markup {
        case let heading as Heading:
            return renderHeading(heading)
        case let paragraph as Paragraph:
            let inner = renderInlines(paragraph.inlineChildren)
            let s = NSMutableAttributedString(attributedString: inner)
            s.addAttribute(.paragraphStyle, value: paragraphStyle(), range: NSRange(location: 0, length: s.length))
            s.append(NSAttributedString(string: "\n\n", attributes: [.font: serif]))
            return s
        case let list as UnorderedList:
            return renderList(list, ordered: false)
        case let list as OrderedList:
            return renderList(list, ordered: true)
        case let quote as BlockQuote:
            return renderBlockQuote(quote)
        case let code as CodeBlock:
            return renderCodeBlock(code)
        case is ThematicBreak:
            return NSAttributedString(
                string: "\n———\n\n",
                attributes: [
                    .font: serif,
                    .foregroundColor: muted,
                    .paragraphStyle: centeredParagraphStyle()
                ]
            )
        case let html as HTMLBlock:
            // Render raw HTML as code-block-ish monospaced — we don't parse
            // arbitrary HTML inline. Rare in NIP-23 content.
            return NSAttributedString(
                string: html.rawHTML + "\n\n",
                attributes: [.font: mono, .foregroundColor: muted]
            )
        default:
            // Unknown block: fall through to rendering its children inline.
            let out = NSMutableAttributedString()
            for child in markup.children {
                out.append(renderBlock(child))
            }
            return out
        }
    }

    private mutating func renderHeading(_ heading: Heading) -> NSAttributedString {
        let base = UIFontDescriptor.preferredFontDescriptor(withTextStyle: .body)
            .withDesign(.serif) ?? UIFontDescriptor.preferredFontDescriptor(withTextStyle: .body)
        let pointSize: CGFloat
        switch heading.level {
        case 1: pointSize = bodyPointSize + 14
        case 2: pointSize = bodyPointSize + 10
        case 3: pointSize = bodyPointSize + 6
        case 4: pointSize = bodyPointSize + 3
        default: pointSize = bodyPointSize + 1
        }
        let bold = base.withSymbolicTraits(.traitBold) ?? base
        let font = UIFont(descriptor: bold, size: pointSize)

        let para = NSMutableParagraphStyle()
        para.paragraphSpacing = 10
        para.paragraphSpacingBefore = 18
        para.lineHeightMultiple = 1.1

        let inner = renderInlines(heading.inlineChildren)
        let out = NSMutableAttributedString(attributedString: inner)
        out.addAttributes(
            [
                .font: font,
                .foregroundColor: ink,
                .paragraphStyle: para
            ],
            range: NSRange(location: 0, length: out.length)
        )
        out.append(NSAttributedString(string: "\n\n", attributes: [.font: font]))
        return out
    }

    private mutating func renderList(_ list: Markup, ordered: Bool) -> NSAttributedString {
        let out = NSMutableAttributedString()
        var idx = 1
        for child in list.children {
            guard let item = child as? ListItem else { continue }
            let bullet: String = ordered ? "\(idx). " : "•  "
            idx += 1

            let itemBuf = NSMutableAttributedString(
                string: bullet,
                attributes: [.font: serifBold, .foregroundColor: accent]
            )
            for sub in item.children {
                // Inside a list item, paragraphs render as inline lines so
                // the bullet stays on the same visual row.
                if let para = sub as? Paragraph {
                    let inner = renderInlines(para.inlineChildren)
                    itemBuf.append(inner)
                } else {
                    itemBuf.append(renderBlock(sub))
                }
            }
            itemBuf.append(NSAttributedString(string: "\n"))
            // Apply list-friendly paragraph style (indent).
            let p = NSMutableParagraphStyle()
            p.headIndent = 24
            p.firstLineHeadIndent = 0
            p.paragraphSpacing = 6
            p.lineHeightMultiple = 1.35
            itemBuf.addAttribute(.paragraphStyle, value: p, range: NSRange(location: 0, length: itemBuf.length))
            out.append(itemBuf)
        }
        out.append(NSAttributedString(string: "\n", attributes: [.font: serif]))
        return out
    }

    private mutating func renderBlockQuote(_ quote: BlockQuote) -> NSAttributedString {
        let inner = NSMutableAttributedString()
        for child in quote.children {
            inner.append(renderBlock(child))
        }
        let p = NSMutableParagraphStyle()
        p.headIndent = 18
        p.firstLineHeadIndent = 18
        p.paragraphSpacingBefore = 8
        p.paragraphSpacing = 10
        p.lineHeightMultiple = 1.4
        inner.addAttributes(
            [
                .foregroundColor: muted,
                .paragraphStyle: p,
                .font: serifItalic
            ],
            range: NSRange(location: 0, length: inner.length)
        )
        return inner
    }

    private mutating func renderCodeBlock(_ code: CodeBlock) -> NSAttributedString {
        let p = NSMutableParagraphStyle()
        p.paragraphSpacing = 14
        p.paragraphSpacingBefore = 6
        p.lineHeightMultiple = 1.25
        return NSAttributedString(
            string: code.code + "\n",
            attributes: [
                .font: mono,
                .foregroundColor: ink,
                .paragraphStyle: p,
                .backgroundColor: muted.withAlphaComponent(0.08)
            ]
        )
    }

    // MARK: - Inline

    private mutating func renderInlines(_ inlines: LazyMapSequence<MarkupChildren, InlineMarkup>) -> NSAttributedString {
        let out = NSMutableAttributedString()
        for inline in inlines {
            out.append(renderInline(inline))
        }
        return out
    }

    private mutating func renderInline(_ inline: InlineMarkup) -> NSAttributedString {
        switch inline {
        case let text as Markdown.Text:
            return renderPlainText(text.string)
        case let emphasis as Emphasis:
            let inner = renderInlines(emphasis.inlineChildren)
            let out = NSMutableAttributedString(attributedString: inner)
            out.addAttribute(.font, value: serifItalic, range: NSRange(location: 0, length: out.length))
            return out
        case let strong as Strong:
            let inner = renderInlines(strong.inlineChildren)
            let out = NSMutableAttributedString(attributedString: inner)
            out.addAttribute(.font, value: serifBold, range: NSRange(location: 0, length: out.length))
            return out
        case let strike as Strikethrough:
            let inner = renderInlines(strike.inlineChildren)
            let out = NSMutableAttributedString(attributedString: inner)
            out.addAttribute(.strikethroughStyle, value: NSUnderlineStyle.single.rawValue, range: NSRange(location: 0, length: out.length))
            return out
        case let code as InlineCode:
            return NSAttributedString(
                string: code.code,
                attributes: [
                    .font: mono,
                    .backgroundColor: muted.withAlphaComponent(0.15),
                    .foregroundColor: ink
                ]
            )
        case let link as Link:
            let inner = renderInlines(link.inlineChildren)
            let out = NSMutableAttributedString(attributedString: inner)
            if let dest = link.destination, let url = URL(string: dest) {
                out.addAttributes(
                    [
                        .link: url,
                        .foregroundColor: accent,
                        .underlineStyle: NSUnderlineStyle.single.rawValue,
                        .underlineColor: accent.withAlphaComponent(0.4)
                    ],
                    range: NSRange(location: 0, length: out.length)
                )
            }
            return out
        case let image as Image:
            // Standalone image paragraphs are handled as BodySegment.image in
            // BodyWalker.render(); this branch only fires for images embedded
            // inside a mixed paragraph. Render as a tappable link so the
            // reader can open the full-screen viewer via highlighter://image/.
            let alt = image.plainText
            let dest = image.source ?? ""
            let label = alt.isEmpty ? "Image" : alt
            if !dest.isEmpty,
               let encoded = dest.addingPercentEncoding(withAllowedCharacters: .urlQueryAllowed),
               let linkURL = URL(string: "highlighter://image/\(encoded)") {
                return NSAttributedString(string: "[\(label)]", attributes: [
                    .font: serifItalic,
                    .foregroundColor: accent,
                    .underlineStyle: NSUnderlineStyle.single.rawValue,
                    .underlineColor: accent.withAlphaComponent(0.4),
                    .link: linkURL
                ])
            }
            return NSAttributedString(
                string: "[\(label)]",
                attributes: [.font: serifItalic, .foregroundColor: muted]
            )
        case is LineBreak:
            return NSAttributedString(string: "\n", attributes: [.font: serif])
        case is SoftBreak:
            return NSAttributedString(string: " ", attributes: [.font: serif])
        default:
            // Unknown inline: flatten via `plainText`.
            return renderPlainText(inline.plainText)
        }
    }

    /// Scan plain text for `[^id]` footnote references and `nostr:` profile
    /// mentions, emitting styled runs for each. Everything else is plain serif.
    private mutating func renderPlainText(_ s: String) -> NSAttributedString {
        let hasFootnote = s.contains("[^")
        let hasNostr = nostrDecoder != nil && s.contains("nostr:")
        guard hasFootnote || hasNostr else {
            return NSAttributedString(string: s, attributes: [.font: serif, .foregroundColor: ink])
        }

        let out = NSMutableAttributedString()
        var i = s.startIndex

        while i < s.endIndex {
            // Find earliest next footnote or nostr: marker
            let fn = hasFootnote ? s.range(of: "[^", range: i..<s.endIndex) : nil
            let ns = hasNostr ? s.range(of: "nostr:", options: .caseInsensitive, range: i..<s.endIndex) : nil

            let next: Range<String.Index>?
            if let fn, let ns {
                next = fn.lowerBound <= ns.lowerBound ? fn : ns
            } else {
                next = fn ?? ns
            }

            guard let special = next else {
                appendPlain(String(s[i...]), to: out)
                break
            }

            // Text before the marker
            if special.lowerBound > i {
                appendPlain(String(s[i..<special.lowerBound]), to: out)
            }

            if special == fn {
                // Footnote reference [^id]
                let afterOpen = s[special.upperBound...]
                guard let closeRange = afterOpen.range(of: "]") else {
                    appendPlain(String(s[special.lowerBound...]), to: out)
                    return out
                }
                let id = String(afterOpen[afterOpen.startIndex..<closeRange.lowerBound])
                if let def = definitionsById[id] {
                    let marker = "[\(def.number)]"
                    let rangeStart = out.length
                    let superSize = max(10, bodyPointSize - 6)
                    out.append(NSAttributedString(string: marker, attributes: [
                        .font: UIFont.systemFont(ofSize: superSize, weight: .semibold),
                        .foregroundColor: accent,
                        .baselineOffset: bodyPointSize * 0.35,
                        MarkdownRenderer.footnoteReferenceAttribute: def.number,
                        .link: URL(string: "highlighter://footnote/\(def.number)")!
                    ]))
                    footnoteAnchors[def.number] = NSRange(location: rangeStart, length: marker.utf16.count)
                } else {
                    appendPlain("[^\(id)]", to: out)
                }
                i = closeRange.upperBound
            } else {
                // nostr: entity
                let bodyStart = special.upperBound
                var end = bodyStart
                while end < s.endIndex {
                    guard let sc = s[end].unicodeScalars.first, s[end].unicodeScalars.count == 1 else { break }
                    let v = sc.value
                    if (0x30...0x39).contains(v) || (0x61...0x7A).contains(v) { end = s.index(after: end) }
                    else { break }
                }
                let bech32 = String(s[bodyStart..<end])
                let lower = bech32.lowercased()
                let isKnown = lower.hasPrefix("npub1") || lower.hasPrefix("nprofile1")
                    || lower.hasPrefix("note1") || lower.hasPrefix("nevent1") || lower.hasPrefix("naddr1")

                if isKnown, let decoder = nostrDecoder, let ref = decoder(bech32) {
                    switch ref {
                    case .profile(let pk, _):
                        let label = profileNames[pk] ?? "@" + String(pk.prefix(8))
                        let atLabel = label.hasPrefix("@") ? label : "@\(label)"
                        out.append(NSAttributedString(string: atLabel, attributes: [
                            .font: serifBold,
                            .foregroundColor: accent,
                            .link: URL(string: "highlighter://profile/\(pk)")!
                        ]))
                    case .event, .address:
                        // Inline event refs are unlikely in body paragraphs;
                        // standalone ones become .nostrEntity segments above.
                        // Render a short dimmed chip so nothing vanishes.
                        let kind: String
                        if case .event(let id, _, _, _) = ref { kind = "note:\(id.prefix(8))…" }
                        else if case .address(_, _, let d, _) = ref { kind = d.isEmpty ? "article" : d }
                        else { kind = "…" }
                        out.append(NSAttributedString(string: "[\(kind)]", attributes: [
                            .font: mono,
                            .foregroundColor: muted
                        ]))
                    }
                } else if bech32.isEmpty {
                    appendPlain("nostr:", to: out)
                }
                // Unknown / undecodable entity: silently drop the raw URI
                i = end
            }
        }

        return out
    }

    private func appendPlain(_ s: String, to out: NSMutableAttributedString) {
        guard !s.isEmpty else { return }
        out.append(NSAttributedString(string: s, attributes: [.font: serif, .foregroundColor: ink]))
    }

    // MARK: - Paragraph styles

    private func paragraphStyle() -> NSParagraphStyle {
        let p = NSMutableParagraphStyle()
        p.paragraphSpacing = 4
        p.lineHeightMultiple = 1.45
        return p
    }

    private func centeredParagraphStyle() -> NSParagraphStyle {
        let p = NSMutableParagraphStyle()
        p.alignment = .center
        p.paragraphSpacing = 12
        p.paragraphSpacingBefore = 12
        return p
    }
}
