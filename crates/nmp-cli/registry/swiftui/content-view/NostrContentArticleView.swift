import SwiftUI
import UIKit

/// Article-mode rendering for `NostrContentView`: the selection / decoration /
/// footnote path. Kept in its own extension so the core renderer stays focused
/// on the default `Text`-concatenation surface. Article mode activates when the
/// host opts into selection, supplies decorations, or the body carries
/// footnotes — see `articleMode`.
extension NostrContentView {
    /// Footnotes recovered from the body text (`[^label]` + `[^label]:`). The
    /// Rust content tree has no footnote node, so these are parsed from the
    /// shipped text at render time.
    var footnotes: [NostrContentFootnote] { nostrContentFootnotes(tree) }

    /// Article mode turns on the `UITextView`-backed inline path (selection /
    /// decoration overlays / tappable footnote markers). Off, the renderer uses
    /// pure SwiftUI `Text` concatenation, unchanged.
    var articleMode: Bool {
        selectionEnabled || !decorations.isEmpty || !footnotes.isEmpty
    }

    func footnoteAnchorId(_ label: String) -> String { "nmp-footnote-\(label)" }

    /// True when an inline group is *only* footnote definitions (`[^label]: …`)
    /// for labels that resolved to a real footnote. Such groups are suppressed
    /// from the body loop because `footnoteSection` renders them — rendering
    /// them in both places would duplicate every definition and split the
    /// scroll-to anchor. Groups that mix a definition with other prose are kept
    /// (we only drop pure-definition paragraphs); an empty group is not a
    /// definition.
    func isFootnoteDefinitionGroup(_ group: NostrContentGroup) -> Bool {
        guard case .inline(_, let children) = group else { return false }
        let text = nostrContentPlainText(tree, children: children, mentionLabel: mentionLabel)
            .trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { return false }
        let knownLabels = Set(footnotes.map(\.label))
        // Every non-empty line must be a definition for a known footnote label.
        let lines = text.split(separator: "\n", omittingEmptySubsequences: true)
        guard !lines.isEmpty else { return false }
        for line in lines {
            let defs = footnoteDefinitions(in: String(line))
            guard defs.count == 1, let (label, _) = defs.first, knownLabels.contains(label) else {
                return false
            }
        }
        return true
    }

    /// Route a tapped link from `NostrSelectableText`: decoration → app
    /// callback, footnote → scroll, anything else (real `http(s)` links) →
    /// `onLinkTap`.
    func handleArticleLink(_ url: URL, proxy: ScrollViewProxy?) {
        switch NostrContentLink.decode(url) {
        case .decoration(let id):
            renderer.callbacks.onDecorationTap(id)
        case .footnote(let label):
            withAnimation { proxy?.scrollTo(footnoteAnchorId(label), anchor: .top) }
        case .none:
            if let scheme = url.scheme, scheme == "http" || scheme == "https" {
                renderer.callbacks.onLinkTap(url)
            }
        }
    }

    /// Article-mode inline run: rendered through `NostrSelectableText` (a
    /// `UITextView`) so selection, decoration overlays, and footnote markers
    /// all work. The selection menu reports `(quote, context)` and decoration /
    /// footnote link taps route through `handleArticleLink`.
    @ViewBuilder
    func articleInlineGroup(level: NostrContentInlineLevel, children: [UInt32], proxy: ScrollViewProxy?) -> some View {
        let baseFont = articleBaseFont(for: level)
        let attributed = articleAttributed(
            children,
            baseFont: baseFont,
            decorations: decorations,
            footnotes: footnotes
        )
        NostrSelectableText(
            attributed: attributed,
            onSelect: { quote, context in
                renderer.callbacks.onTextSelected(quote, context)
            },
            onLink: { url in handleArticleLink(url, proxy: proxy) }
        )
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private func articleBaseFont(for level: NostrContentInlineLevel) -> UIFont {
        switch level {
        case .paragraph: return UIFont.preferredFont(forTextStyle: .body)
        case .heading(let lvl): return headingUIFont(for: lvl)
        }
    }

    private func headingUIFont(for level: UInt8) -> UIFont {
        let style: UIFont.TextStyle
        switch level {
        case 1: style = .largeTitle
        case 2: style = .title1
        case 3: style = .title2
        case 4: style = .title3
        case 5: style = .headline
        default: style = .subheadline
        }
        let base = UIFont.preferredFont(forTextStyle: style)
        guard let d = base.fontDescriptor.withSymbolicTraits(.traitBold) else { return base }
        return UIFont(descriptor: d, size: base.pointSize)
    }

    /// Footnote definitions rendered at the foot of the article, each with a
    /// stable scroll anchor so a `[^n]` marker tap can jump to it via the
    /// `ScrollViewReader` in `body`.
    @ViewBuilder
    func footnoteSection(proxy: ScrollViewProxy) -> some View {
        if !footnotes.isEmpty {
            VStack(alignment: .leading, spacing: 8) {
                Rectangle()
                    .fill(renderer.quoteBorderColor)
                    .frame(height: 1)
                    .padding(.vertical, 4)
                ForEach(footnotes) { footnote in
                    HStack(alignment: .firstTextBaseline, spacing: 8) {
                        Text("\(footnote.marker).")
                            .font(.caption.weight(.semibold))
                            .foregroundStyle(renderer.linkColor)
                        Text(footnote.body)
                            .font(.callout)
                            .foregroundStyle(renderer.secondaryTextColor)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                    .id(footnoteAnchorId(footnote.label))
                }
            }
            .padding(.top, 4)
        }
    }
}
