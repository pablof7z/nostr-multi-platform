import Foundation
import UIKit
import Markdown

struct BodyWalker {
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
