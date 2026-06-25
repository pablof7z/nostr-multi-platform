import SwiftUI

// MARK: - Decoration model (range overlays)

/// Opaque identity for a body decoration (typically a NIP-84 highlight event
/// id). The renderer carries it through `onDecorationTap` so the app can open
/// the underlying highlight without the renderer knowing what it is.
public struct NostrContentDecorationId: Hashable, Sendable {
    public let raw: String
    public init(_ raw: String) { self.raw = raw }
}

/// A highlight overlay to render on the article body. The renderer locates
/// `quote` as a substring of the reconstructed plain-text body and paints the
/// matching range with `color`. Expressing the range as the *quoted text*
/// (rather than a byte offset) matches the NIP-84 data model — a kind:9802
/// highlight stores the quoted text — so no positional metadata is needed from
/// the content tree.
///
/// `quote` is matched case-sensitively against the body's visible text. If the
/// same quote appears more than once, every occurrence is decorated; apps that
/// need single-occurrence disambiguation should pass a longer surrounding span.
public struct NostrContentDecoration: Identifiable, Equatable, Sendable {
    public let id: NostrContentDecorationId
    public let quote: String
    public let color: Color

    public init(id: NostrContentDecorationId, quote: String, color: Color) {
        self.id = id
        self.quote = quote
        self.color = color
    }

    public init(id: String, quote: String, color: Color) {
        self.init(id: NostrContentDecorationId(id), quote: quote, color: color)
    }
}

// MARK: - Footnotes

/// A footnote parsed out of the body text. NIP-23 article bodies carry footnote
/// markers as Commonmark `[^label]` references plus a matching `[^label]: …`
/// definition; the Rust content tree models these as plain text (no dedicated
/// footnote node), so the renderer recovers them at render time from the text
/// runs. `marker` is the display marker (1-based ordinal), `label` the source
/// label, and `body` the definition text.
public struct NostrContentFootnote: Identifiable, Equatable, Sendable {
    public let id: String
    public let label: String
    public let marker: Int
    public let body: String

    public init(id: String, label: String, marker: Int, body: String) {
        self.id = id
        self.label = label
        self.marker = marker
        self.body = body
    }
}

// MARK: - Plain-text reconstruction

/// Reconstruct the visible plain text of one inline run subtree, in render
/// order. Used both for the `onTextSelected` context payload and for locating
/// decoration / footnote substrings. Mirrors the visible glyphs the renderer
/// emits for the same nodes (mentions render as `@label`, hashtags as `#tag`,
/// etc.) so a selection range maps back onto the source text predictably.
public func nostrContentPlainText(
    _ tree: ContentTreeWire,
    children: [UInt32],
    mentionLabel: (NostrWireUri) -> String
) -> String {
    var out = ""
    func walk(_ index: UInt32) {
        if index == nostrContentNewlineSentinel {
            out += "\n"
            return
        }
        guard let node = tree.node(at: index) else { return }
        switch node {
        case .text(let value): out += value
        case .mention(let uri): out += "@\(mentionLabel(uri))"
        case .eventRef(let uri): out += uri.primaryId
        case .hashtag(let tag): out += "#\(tag)"
        case .url(let value): out += value
        case .emoji(let shortcode, _): out += ":\(shortcode):"
        case .inlineCode(let value): out += value
        case .emphasis(let kids), .strong(let kids):
            kids.forEach(walk)
        case .link(let kids, _):
            kids.forEach(walk)
        case .paragraph(let kids), .heading(_, let kids), .blockQuote(let kids):
            kids.forEach(walk)
        case .image(let alt, _, _):
            out += alt.isEmpty ? "[image]" : "[\(alt)]"
        case .softBreak: out += " "
        case .hardBreak: out += "\n"
        case .invoice, .list, .codeBlock, .rule, .media, .placeholder:
            break
        }
    }
    children.forEach(walk)
    return out
}

// MARK: - Footnote extraction

/// Extract footnote definitions (`[^label]: body`) from the document's text
/// nodes, numbering them in first-reference order. Returns the footnotes keyed
/// for lookup plus an ordinal map from source label → display marker.
public func nostrContentFootnotes(_ tree: ContentTreeWire) -> [NostrContentFootnote] {
    // 1. Collect every label referenced via `[^label]` across all text nodes,
    //    in document order, to assign stable 1-based markers.
    var order: [String] = []
    var seen = Set<String>()
    var definitions: [String: String] = [:]

    for node in tree.nodes {
        guard case .text(let value) = node else { continue }
        for label in footnoteReferences(in: value) where !seen.contains(label) {
            seen.insert(label)
            order.append(label)
        }
        for (label, body) in footnoteDefinitions(in: value) {
            definitions[label] = body
        }
    }

    // A label only becomes a real footnote if it has a definition.
    var result: [NostrContentFootnote] = []
    var marker = 0
    for label in order {
        guard let body = definitions[label] else { continue }
        marker += 1
        result.append(
            NostrContentFootnote(id: label, label: label, marker: marker, body: body)
        )
    }
    return result
}

/// Inline `[^label]` references in a string, excluding definition lines
/// (`[^label]:`), in order.
func footnoteReferences(in value: String) -> [String] {
    var labels: [String] = []
    let scalars = Array(value)
    var i = 0
    while i < scalars.count {
        if scalars[i] == "[", i + 1 < scalars.count, scalars[i + 1] == "^" {
            var j = i + 2
            var label = ""
            while j < scalars.count, scalars[j] != "]" {
                label.append(scalars[j])
                j += 1
            }
            // Require a closing `]`, a non-empty label, and that this is a
            // reference (not a `[^x]:` definition).
            if j < scalars.count, scalars[j] == "]", !label.isEmpty {
                let isDefinition = j + 1 < scalars.count && scalars[j + 1] == ":"
                if !isDefinition { labels.append(label) }
                i = j + 1
                continue
            }
        }
        i += 1
    }
    return labels
}

/// `[^label]: body` definitions in a string.
func footnoteDefinitions(in value: String) -> [(String, String)] {
    var defs: [(String, String)] = []
    for line in value.split(separator: "\n", omittingEmptySubsequences: false) {
        let trimmed = line.trimmingCharacters(in: .whitespaces)
        guard trimmed.hasPrefix("[^"),
              let close = trimmed.range(of: "]:")
        else { continue }
        let label = String(trimmed[trimmed.index(trimmed.startIndex, offsetBy: 2)..<close.lowerBound])
        let body = String(trimmed[close.upperBound...]).trimmingCharacters(in: .whitespaces)
        if !label.isEmpty { defs.append((label, body)) }
    }
    return defs
}
