import SwiftUI
import UIKit

/// `UITextView` wrapper that handles:
/// - Attributed-string body with native text selection.
/// - A custom Edit Menu injecting `Highlight` + `Highlight with note` actions
///   when the user makes a selection.
/// - Tap hit-testing on existing highlight runs (dispatches to
///   `onHighlightTap`).
/// - Tap hit-testing on footnote superscript runs + corresponding back-link
///   in the footnote block.
///
/// `attributedText` is the full body — the reader composes body + separator +
/// footnotes into a single attributed string before handing it here so every
/// anchor lives in one `NSAttributedString`.
struct ArticleBodyView: UIViewRepresentable {
    let attributedText: NSAttributedString
    let footnoteAnchors: [Int: NSRange]
    let footnoteBackAnchors: [Int: NSRange]
    let highlightsById: [String: HighlightRecord]
    let paperColor: UIColor

    /// User selected text and tapped **Highlight** (without note). The view
    /// hands back the selected text + the surrounding paragraph as context.
    var onPublishHighlight: (_ quote: String, _ context: String) -> Void

    /// User selected text and tapped **Note**. The view shows a sheet; when
    /// the sheet confirms, the app calls `onPublishHighlight` with the note.
    var onRequestNote: (_ quote: String, _ context: String) -> Void

    /// User tapped an existing highlight run.
    var onHighlightTap: (_ highlight: HighlightRecord) -> Void

    /// User tapped a footnote superscript — scroll the body to the definition.
    var onFootnoteTap: (_ number: Int) -> Void

    /// User tapped the `↩` back-arrow in a footnote definition — scroll back
    /// to the inline reference.
    var onFootnoteBackTap: (_ number: Int) -> Void

    /// User tapped an inline image link (images that appear inside a mixed
    /// text paragraph; standalone images render as SwiftUI views).
    var onImageTap: (_ url: URL) -> Void

    /// User tapped a `nostr:npub1…` / `nostr:nprofile1…` mention rendered as
    /// a tappable `@name` run. The argument is the hex pubkey.
    var onProfileTap: (_ pubkey: String) -> Void

    func makeUIView(context: Context) -> UITextView {
        // TextKit 2 (`usingTextLayoutManager: true`) has a known bug where it
        // under-reports intrinsic content height for very long attributed strings
        // when `isScrollEnabled = false`, causing the parent ScrollView to clip
        // the article body before the end. TextKit 1 computes the correct height.
        let tv = ReaderTextView(usingTextLayoutManager: false)
        tv.coordinator = context.coordinator
        tv.isEditable = false
        tv.isSelectable = true
        // Outer SwiftUI ScrollView owns the gesture — the text view itself
        // stays non-scrolling and sizes to its intrinsic content.
        tv.isScrollEnabled = false
        tv.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        tv.setContentHuggingPriority(.defaultHigh, for: .vertical)
        tv.backgroundColor = paperColor
        tv.textContainer.lineFragmentPadding = 0
        tv.textContainerInset = UIEdgeInsets(top: 12, left: 20, bottom: 40, right: 20)
        tv.attributedText = attributedText
        tv.adjustsFontForContentSizeCategory = true
        tv.dataDetectorTypes = []
        // Empty dictionary keeps the per-run `.foregroundColor` / underline
        // attributes we set in `MarkdownRenderer` — UITextView would
        // otherwise paint all links in its default blue.
        tv.linkTextAttributes = [:]
        tv.delegate = context.coordinator
        tv.tintColor = UIColor(Color.highlighterAccent)

        // Tap recognizer for custom hit-testing (highlights + footnotes).
        let tap = UITapGestureRecognizer(
            target: context.coordinator,
            action: #selector(Coordinator.handleTap(_:))
        )
        tap.cancelsTouchesInView = false
        tap.delegate = context.coordinator
        tv.addGestureRecognizer(tap)

        context.coordinator.textView = tv

        return tv
    }

    func updateUIView(_ uiView: UITextView, context: Context) {
        if uiView.attributedText != attributedText {
            uiView.attributedText = attributedText
        }
        uiView.backgroundColor = paperColor
        context.coordinator.parent = self
    }

    func makeCoordinator() -> Coordinator {
        Coordinator(parent: self)
    }

    // MARK: - Coordinator

    @MainActor
    final class Coordinator: NSObject, UITextViewDelegate, UIGestureRecognizerDelegate {
        var parent: ArticleBodyView
        weak var textView: UITextView?

        init(parent: ArticleBodyView) {
            self.parent = parent
        }

        // The attributed text embeds its own link attributes with URLs like
        // `highlighter://footnote/<n>` so UITextView's selection/long-press
        // menu disables the default link handling; we route taps ourselves.
        func textView(_ textView: UITextView, primaryActionFor textItem: UITextItem, defaultAction: UIAction) -> UIAction? {
            if case let .link(url) = textItem.content {
                if url.scheme == "highlighter" {
                    return UIAction { [weak self] _ in
                        self?.handleCustomURL(url)
                    }
                }
            }
            return defaultAction
        }

        // MARK: Edit Menu (selection action sheet)

        // `UITextView` owns its own `UIEditMenuInteraction` internally; the
        // delegate hook below is the supported way to add actions to the
        // selection menu in iOS 16+.
        func textView(
            _ textView: UITextView,
            editMenuForTextIn range: NSRange,
            suggestedActions: [UIMenuElement]
        ) -> UIMenu? {
            guard range.length > 0 else {
                return UIMenu(children: suggestedActions)
            }

            let highlightAction = UIAction(
                title: "Highlight",
                image: UIImage(systemName: "highlighter")
            ) { [weak self] _ in
                guard let self, let tv = self.textView else { return }
                let (quote, context) = self.selectionText(tv)
                guard !quote.isEmpty else { return }
                self.parent.onPublishHighlight(quote, context)
                tv.selectedRange = NSRange(location: 0, length: 0)
            }

            let noteAction = UIAction(
                title: "Highlight with note",
                image: UIImage(systemName: "square.and.pencil")
            ) { [weak self] _ in
                guard let self, let tv = self.textView else { return }
                let (quote, context) = self.selectionText(tv)
                guard !quote.isEmpty else { return }
                self.parent.onRequestNote(quote, context)
                tv.selectedRange = NSRange(location: 0, length: 0)
            }

            let customMenu = UIMenu(options: .displayInline, children: [highlightAction, noteAction])
            return UIMenu(children: [customMenu] + suggestedActions)
        }

        private func selectionText(_ tv: UITextView) -> (quote: String, context: String) {
            let range = tv.selectedRange
            guard range.length > 0 else { return ("", "") }
            guard let textRange = Range(range, in: tv.text) else { return ("", "") }
            let quote = String(tv.text[textRange]).trimmingCharacters(in: .whitespacesAndNewlines)

            // Context: the paragraph the selection starts in. Find the
            // paragraph bounds by scanning for double-newlines on either side.
            let full = tv.text as NSString
            var start = range.location
            var end = range.location + range.length
            while start > 0 {
                let prior = full.substring(with: NSRange(location: start - 1, length: 1))
                if prior == "\n" {
                    // Stop one step before a double-newline paragraph break.
                    if start >= 2, full.substring(with: NSRange(location: start - 2, length: 1)) == "\n" {
                        break
                    }
                }
                start -= 1
            }
            while end < full.length {
                if end + 1 < full.length,
                   full.substring(with: NSRange(location: end, length: 1)) == "\n",
                   full.substring(with: NSRange(location: end + 1, length: 1)) == "\n" {
                    break
                }
                end += 1
            }
            let paragraphRange = NSRange(location: start, length: max(0, end - start))
            let paragraph = full.substring(with: paragraphRange)
                .trimmingCharacters(in: .whitespacesAndNewlines)
            let context = paragraph == quote ? "" : paragraph
            return (quote, context)
        }

        // MARK: Tap hit-testing

        @objc func handleTap(_ gr: UITapGestureRecognizer) {
            guard let tv = textView, tv.selectedRange.length == 0 else { return }
            let point = gr.location(in: tv)
            guard let pos = tv.closestPosition(to: point) else { return }

            // Convert to an index and walk attributes there.
            let offset = tv.offset(from: tv.beginningOfDocument, to: pos)
            let length = tv.attributedText.length
            guard offset >= 0, offset < length else { return }

            let attrs = tv.attributedText.attributes(at: offset, effectiveRange: nil)

            if let id = attrs[MarkdownRenderer.highlightAttribute] as? String,
               let record = parent.highlightsById[id] {
                parent.onHighlightTap(record)
                return
            }
            if let number = attrs[MarkdownRenderer.footnoteReferenceAttribute] as? Int {
                parent.onFootnoteTap(number)
                return
            }
            if let number = attrs[MarkdownRenderer.footnoteBackAttribute] as? Int {
                parent.onFootnoteBackTap(number)
                return
            }
            // Fall through: let `UITextView` process its normal link handling.
        }

        // Let the tap coexist with selection/long-press gestures rather than
        // swallowing them.
        func gestureRecognizer(
            _ gestureRecognizer: UIGestureRecognizer,
            shouldRecognizeSimultaneouslyWith other: UIGestureRecognizer
        ) -> Bool {
            true
        }

        // MARK: Custom URL routing

        private func handleCustomURL(_ url: URL) {
            guard url.scheme == "highlighter" else { return }
            let host = url.host ?? ""
            let n = Int(url.lastPathComponent) ?? 0
            switch host {
            case "footnote":
                parent.onFootnoteTap(n)
            case "footnote-back":
                parent.onFootnoteBackTap(n)
            case "image":
                let encoded = url.absoluteString.dropFirst("highlighter://image/".count)
                let decoded = String(encoded).removingPercentEncoding ?? String(encoded)
                if let imageURL = URL(string: decoded) {
                    parent.onImageTap(imageURL)
                }
            case "profile":
                let pubkey = url.lastPathComponent
                if !pubkey.isEmpty { parent.onProfileTap(pubkey) }
            default:
                break
            }
        }
    }
}

/// `UITextView` subclass that exposes enough API for the coordinator to ask
/// the edit menu to present itself at a specific rect. Today this is just a
/// forwarding stub; future refinements (pinch-to-zoom typography, etc.) can
/// live here without touching the SwiftUI wrapper.
private final class ReaderTextView: UITextView {
    weak var coordinator: ArticleBodyView.Coordinator?
}
