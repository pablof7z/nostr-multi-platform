import SwiftUI
import UIKit

/// Custom URL schemes the article-mode renderer uses to make decoration and
/// footnote ranges tappable inside `NostrSelectableText`. The view layer maps a
/// tapped link back to the originating decoration id / footnote label.
public enum NostrContentLink {
    public static let decorationScheme = "nmp-decoration"
    public static let footnoteScheme = "nmp-footnote"

    public static func decoration(_ id: NostrContentDecorationId) -> URL? {
        guard let encoded = id.raw.addingPercentEncoding(withAllowedCharacters: .urlHostAllowed)
        else { return nil }
        return URL(string: "\(decorationScheme)://\(encoded)")
    }

    public static func footnote(_ label: String) -> URL? {
        guard let encoded = label.addingPercentEncoding(withAllowedCharacters: .urlHostAllowed)
        else { return nil }
        return URL(string: "\(footnoteScheme)://\(encoded)")
    }

    /// Decode a tapped link back into either a decoration id or a footnote
    /// label. Returns `nil` for any other URL (those route to `onLinkTap`).
    public static func decode(_ url: URL) -> Decoded? {
        guard let host = url.host?.removingPercentEncoding else { return nil }
        switch url.scheme {
        case decorationScheme: return .decoration(NostrContentDecorationId(host))
        case footnoteScheme: return .footnote(host)
        default: return nil
        }
    }

    public enum Decoded {
        case decoration(NostrContentDecorationId)
        case footnote(String)
    }
}

/// `NSAttributedString` inline builder for *article mode* (selection /
/// decorations / footnotes). SwiftUI `Text` concatenation can't paint a
/// background over an arbitrary range or expose selection, so article surfaces
/// render their inline runs through a `UITextView` (`NostrSelectableText`),
/// which needs an `NSAttributedString`. The non-article fast path keeps using
/// `inlineText` (concatenated `Text`).
extension NostrContentView {
    func articleAttributed(
        _ children: [UInt32],
        baseFont: UIFont,
        decorations: [NostrContentDecoration],
        footnotes: [NostrContentFootnote]
    ) -> NSAttributedString {
        let out = NSMutableAttributedString()
        for child in children {
            out.append(articleNode(child, baseFont: baseFont))
        }
        // Default text colour for any run that didn't set its own.
        out.enumerateAttribute(.foregroundColor, in: NSRange(location: 0, length: out.length)) { value, range, _ in
            if value == nil {
                out.addAttribute(.foregroundColor, value: uiColor(renderer.textColor), range: range)
            }
        }
        applyFootnoteMarkers(to: out, footnotes: footnotes)
        applyDecorations(to: out, decorations: decorations)
        return out
    }

    private func uiColor(_ color: Color) -> UIColor { UIColor(color) }

    private func articleNode(_ index: UInt32, baseFont: UIFont) -> NSAttributedString {
        if index == nostrContentNewlineSentinel { return NSAttributedString(string: "\n") }
        guard let node = tree.node(at: index) else { return NSAttributedString() }
        func styled(_ string: String, _ attrs: [NSAttributedString.Key: Any] = [:]) -> NSAttributedString {
            var merged: [NSAttributedString.Key: Any] = [.font: baseFont]
            merged.merge(attrs) { _, new in new }
            return NSAttributedString(string: string, attributes: merged)
        }
        switch node {
        case .text(let value):
            return styled(value)
        case .mention(let uri):
            return styled("@\(mentionLabel(uri))", [
                .foregroundColor: uiColor(renderer.mentionColor),
                .font: bold(baseFont),
            ])
        case .eventRef(let uri):
            return styled(uri.primaryId, [.foregroundColor: uiColor(renderer.linkColor)])
        case .hashtag(let tag):
            return styled("#\(tag)", [
                .foregroundColor: uiColor(renderer.hashtagColor),
                .font: bold(baseFont),
            ])
        case .url(let value):
            return styled(value, [.foregroundColor: uiColor(renderer.linkColor)])
        case .emoji(let shortcode, _):
            return styled(":\(shortcode):")
        case .inlineCode(let value):
            return styled(value, [.font: UIFont.monospacedSystemFont(ofSize: baseFont.pointSize, weight: .regular)])
        case .emphasis(let kids):
            return reduce(kids, baseFont: italic(baseFont))
        case .strong(let kids):
            return reduce(kids, baseFont: bold(baseFont))
        case .link(let kids, let href):
            let inner = reduce(kids, baseFont: baseFont)
            if let href, !href.isEmpty, let url = URL(string: href) {
                let m = NSMutableAttributedString(attributedString: inner)
                let full = NSRange(location: 0, length: m.length)
                m.addAttributes([
                    .foregroundColor: uiColor(renderer.linkColor),
                    .underlineStyle: NSUnderlineStyle.single.rawValue,
                    .link: url,
                ], range: full)
                return m
            }
            return inner
        case .paragraph(let kids), .heading(_, let kids), .blockQuote(let kids):
            return reduce(kids, baseFont: baseFont)
        case .image(let alt, _, _):
            return styled(alt.isEmpty ? "[image]" : "[\(alt)]", [
                .foregroundColor: uiColor(renderer.placeholderColor),
            ])
        case .softBreak:
            return styled(" ")
        case .hardBreak:
            return styled("\n")
        case .invoice, .list, .codeBlock, .rule, .media, .placeholder:
            return NSAttributedString()
        }
    }

    private func reduce(_ kids: [UInt32], baseFont: UIFont) -> NSAttributedString {
        let out = NSMutableAttributedString()
        for kid in kids { out.append(articleNode(kid, baseFont: baseFont)) }
        return out
    }

    private func applyDecorations(
        to out: NSMutableAttributedString,
        decorations: [NostrContentDecoration]
    ) {
        guard !decorations.isEmpty else { return }
        let plain = out.string as NSString
        for decoration in decorations where !decoration.quote.isEmpty {
            var searchRange = NSRange(location: 0, length: plain.length)
            while searchRange.location < plain.length {
                let found = plain.range(of: decoration.quote, options: [], range: searchRange)
                if found.location == NSNotFound { break }
                out.addAttribute(.backgroundColor, value: uiColor(decoration.color), range: found)
                if let link = NostrContentLink.decoration(decoration.id) {
                    out.addAttribute(.link, value: link, range: found)
                }
                let next = found.location + max(found.length, 1)
                searchRange = NSRange(location: next, length: plain.length - next)
            }
        }
    }

    /// Turn `[^label]` reference markers into superscript, tappable ordinal
    /// markers (`¹`, `²`, …). Only labels that resolved to a footnote are
    /// replaced; the rest stay as literal text.
    private func applyFootnoteMarkers(
        to out: NSMutableAttributedString,
        footnotes: [NostrContentFootnote]
    ) {
        guard !footnotes.isEmpty else { return }
        // `nostrContentFootnotes` already dedupes by label; `uniquingKeysWith`
        // keeps this crash-proof even if a duplicate label ever slips through.
        let byLabel = Dictionary(footnotes.map { ($0.label, $0) }, uniquingKeysWith: { first, _ in first })
        // Replace from the end so earlier ranges stay valid as we mutate.
        let plain = out.string as NSString
        var replacements: [(NSRange, NostrContentFootnote)] = []
        var search = NSRange(location: 0, length: plain.length)
        while search.location < plain.length {
            let open = plain.range(of: "[^", options: [], range: search)
            if open.location == NSNotFound { break }
            let afterOpen = open.location + open.length
            let closeSearch = NSRange(location: afterOpen, length: plain.length - afterOpen)
            let close = plain.range(of: "]", options: [], range: closeSearch)
            if close.location == NSNotFound { break }
            let labelRange = NSRange(location: afterOpen, length: close.location - afterOpen)
            let label = plain.substring(with: labelRange)
            let isDef = close.location + 1 < plain.length
                && plain.substring(with: NSRange(location: close.location + 1, length: 1)) == ":"
            if !isDef, let fn = byLabel[label] {
                let full = NSRange(location: open.location, length: close.location + 1 - open.location)
                replacements.append((full, fn))
            }
            search = NSRange(location: close.location + 1, length: plain.length - close.location - 1)
        }
        for (range, fn) in replacements.reversed() {
            let marker = NSMutableAttributedString(string: " \(fn.marker)")
            marker.addAttributes([
                .foregroundColor: uiColor(renderer.linkColor),
                .baselineOffset: 5,
            ], range: NSRange(location: 0, length: marker.length))
            if let link = NostrContentLink.footnote(fn.label) {
                marker.addAttribute(.link, value: link, range: NSRange(location: 0, length: marker.length))
            }
            out.replaceCharacters(in: range, with: marker)
        }
    }

    private func bold(_ font: UIFont) -> UIFont {
        guard let d = font.fontDescriptor.withSymbolicTraits(font.fontDescriptor.symbolicTraits.union(.traitBold)) else { return font }
        return UIFont(descriptor: d, size: font.pointSize)
    }

    private func italic(_ font: UIFont) -> UIFont {
        guard let d = font.fontDescriptor.withSymbolicTraits(font.fontDescriptor.symbolicTraits.union(.traitItalic)) else { return font }
        return UIFont(descriptor: d, size: font.pointSize)
    }
}
