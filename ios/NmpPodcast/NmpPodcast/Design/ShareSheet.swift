import SwiftUI
import UIKit

// MARK: - ShareSheet

/// UIKit wrapper that presents a system share sheet via UIActivityViewController.
///
/// **Bug history.** Wrapping `UIActivityViewController` inside SwiftUI's
/// `.sheet { ShareSheet(...) }` modifier renders the activity controller
/// as a blank white sheet on iOS 16+ for **file-URL items** — the
/// activity controller wants its own presentation context (so it can
/// surface AirDrop / Files / etc.) and the SwiftUI sheet's modal scope
/// breaks that. For new file-share entry points, prefer SwiftUI's native
/// `ShareLink(item:, preview:)` (see `SubscriptionsListView` Export OPML)
/// or call `SystemShareSheet.present(items:)` to bypass the SwiftUI
/// modal stack entirely.
///
/// String-only items render fine in a SwiftUI sheet, so existing
/// `ShareSheet(items: ["..."])` call sites are kept for now.
struct ShareSheet: UIViewControllerRepresentable {
    let items: [Any]

    func makeUIViewController(context: Context) -> UIActivityViewController {
        UIActivityViewController(activityItems: items, applicationActivities: nil)
    }

    func updateUIViewController(_ uiViewController: UIActivityViewController, context: Context) {}
}

// MARK: - SystemShareSheet (imperative bypass)

/// Presents `UIActivityViewController` directly on the topmost VC of the
/// foreground key window — bypassing SwiftUI's `.sheet`/`.fullScreenCover`
/// modal stack. Use this from action handlers for file-URL items where
/// the SwiftUI-wrapped `ShareSheet` would render blank.
///
/// Returns silently when no foreground window is available.
@MainActor
enum SystemShareSheet {
    static func present(items: [Any]) {
        guard let scene = UIApplication.shared.connectedScenes
                .first(where: { $0.activationState == .foregroundActive }) as? UIWindowScene,
              let root = scene.keyWindow?.rootViewController
        else { return }
        var presenter = root
        while let presented = presenter.presentedViewController { presenter = presented }
        let activity = UIActivityViewController(activityItems: items, applicationActivities: nil)
        // iPad popover anchoring — without this the present call traps on
        // iPad. Anchors the popover on the centre of the presenter view as
        // a sensible default.
        if let popover = activity.popoverPresentationController {
            popover.sourceView = presenter.view
            popover.sourceRect = CGRect(
                x: presenter.view.bounds.midX,
                y: presenter.view.bounds.midY,
                width: 0,
                height: 0
            )
            popover.permittedArrowDirections = []
        }
        presenter.present(activity, animated: true)
    }
}

// MARK: - ShareButton

/// A button that presents a share sheet when tapped.
///
/// Example usage:
/// ```swift
/// ShareButton(items: [item.title, "Shared from Podcastr"]) {
///     Image(systemName: "square.and.arrow.up")
/// }
/// ```
struct ShareButton<Label: View>: View {
    let items: [Any]
    let label: () -> Label

    @State private var isPresented = false

    init(items: [Any], @ViewBuilder label: @escaping () -> Label) {
        self.items = items
        self.label = label
    }

    var body: some View {
        Button {
            isPresented = true
        } label: {
            label()
        }
        .sheet(isPresented: $isPresented) {
            ShareSheet(items: items)
        }
    }
}
