import SwiftUI
import UIKit

/// A `UITextView`-backed inline paragraph that adds the three article-reading
/// capabilities SwiftUI `Text` cannot express:
///
///   1. **Selection → highlight.** A custom "Highlight" edit-menu action over
///      the user's selection reports `(quote, context)` back to the app via
///      `onSelect`, where `context` is the full paragraph the selection sits
///      inside. Standard Copy/Look-Up/etc. stay available.
///   2. **Range overlays.** Decorations and footnote markers arrive pre-painted
///      in the `attributed` string (background colour + a custom-scheme link),
///      so tapping a decorated range fires `onLink`.
///   3. **Footnote navigation.** Footnote markers are likewise link ranges;
///      `onLink` routes the tap to the scroll-to-footnote handler.
///
/// Only paragraph/heading inline runs route through this; block-level content
/// (media, embeds, code, lists) stays in native SwiftUI. The representable is
/// self-sizing: it disables its own scrolling and reports an intrinsic height
/// to SwiftUI's layout.
struct NostrSelectableText: UIViewRepresentable {
    let attributed: NSAttributedString
    /// `(quote, context)` — the selected substring and the full paragraph text.
    let onSelect: (String, String) -> Void
    /// Tap on a decoration / footnote link range; carries the link URL.
    let onLink: (URL) -> Void

    func makeUIView(context: Context) -> SelfSizingTextView {
        let view = SelfSizingTextView()
        view.isEditable = false
        view.isScrollEnabled = false
        view.isSelectable = true
        view.backgroundColor = .clear
        view.textContainerInset = .zero
        view.textContainer.lineFragmentPadding = 0
        view.adjustsFontForContentSizeCategory = true
        view.delegate = context.coordinator
        view.setContentCompressionResistancePriority(.required, for: .vertical)
        view.setContentHuggingPriority(.required, for: .vertical)
        context.coordinator.attach(view)
        return view
    }

    func updateUIView(_ view: SelfSizingTextView, context: Context) {
        context.coordinator.parent = self
        if view.attributedText != attributed {
            view.attributedText = attributed
            view.invalidateIntrinsicContentSize()
        }
    }

    func makeCoordinator() -> Coordinator { Coordinator(self) }

    final class Coordinator: NSObject, UITextViewDelegate {
        var parent: NostrSelectableText
        private weak var view: UITextView?

        init(_ parent: NostrSelectableText) { self.parent = parent }

        func attach(_ view: UITextView) { self.view = view }

        // Route decoration / footnote link taps (custom-scheme URLs) to the app
        // instead of letting UIKit try to open them. iOS 17+ text-item API.
        func textView(
            _ textView: UITextView,
            primaryActionFor textItem: UITextItem,
            defaultAction: UIAction
        ) -> UIAction? {
            if case .link(let url) = textItem.content {
                let parent = self.parent
                return UIAction { _ in parent.onLink(url) }
            }
            return defaultAction
        }

        // Inject a "Highlight" action into the selection edit menu (iOS 16+).
        func textView(
            _ textView: UITextView,
            editMenuForTextIn range: NSRange,
            suggestedActions: [UIMenuElement]
        ) -> UIMenu? {
            guard range.length > 0,
                  let text = textView.text,
                  let swiftRange = Range(range, in: text)
            else {
                return UIMenu(children: suggestedActions)
            }
            let quote = String(text[swiftRange])
            let context = text
            let highlight = UIAction(
                title: "Highlight",
                image: UIImage(systemName: "highlighter")
            ) { [weak self] _ in
                self?.parent.onSelect(quote, context)
            }
            return UIMenu(children: [highlight] + suggestedActions)
        }
    }
}

/// `UITextView` subclass that reports its laid-out height as its intrinsic
/// content size so it composes inside a SwiftUI `VStack` without an explicit
/// frame. Width comes from the SwiftUI parent; height is derived from it.
final class SelfSizingTextView: UITextView {
    override var intrinsicContentSize: CGSize {
        let width = bounds.width > 0 ? bounds.width : UIView.layoutFittingExpandedSize.width
        let fitting = sizeThatFits(CGSize(width: width, height: .greatestFiniteMagnitude))
        return CGSize(width: UIView.noIntrinsicMetric, height: ceil(fitting.height))
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        if bounds.width != lastWidth {
            lastWidth = bounds.width
            invalidateIntrinsicContentSize()
        }
    }

    private var lastWidth: CGFloat = 0
}
