import UIKit
import SwiftUI
import UniformTypeIdentifiers

/// Entry point for the iOS share sheet. Pulls the first URL out of the
/// extension context and hands it to a SwiftUI picker. The extension never
/// talks to the Rust core or the Keychain — it writes to the App Group and
/// opens the main app, which does the real publish.
final class ShareViewController: UIViewController {
    override func viewDidLoad() {
        super.viewDidLoad()
        extractIncomingURL { [weak self] url in
            guard let self else { return }
            DispatchQueue.main.async {
                self.presentRoot(incomingURL: url)
            }
        }
    }

    private func presentRoot(incomingURL: URL?) {
        let root = ShareRootView(
            incomingURL: incomingURL,
            onSubmit: { [weak self] share in
                ShareQueue.enqueue(share)
                self?.openMainAppAndFinish()
            },
            onCancel: { [weak self] in
                self?.finish()
            }
        )
        let host = UIHostingController(rootView: root)
        addChild(host)
        host.view.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(host.view)
        NSLayoutConstraint.activate([
            host.view.topAnchor.constraint(equalTo: view.topAnchor),
            host.view.bottomAnchor.constraint(equalTo: view.bottomAnchor),
            host.view.leadingAnchor.constraint(equalTo: view.leadingAnchor),
            host.view.trailingAnchor.constraint(equalTo: view.trailingAnchor)
        ])
        host.didMove(toParent: self)
    }

    /// Walk the extension context for the first attached URL. We accept
    /// explicit `public.url` types and fall back to parsing a URL out of a
    /// plain `public.text` item (how Overcast shares on some OS versions).
    private func extractIncomingURL(completion: @escaping @Sendable (URL?) -> Void) {
        let items = (extensionContext?.inputItems as? [NSExtensionItem]) ?? []
        // `NSItemProvider` predates Swift concurrency and isn't marked
        // Sendable. Passing it across `loadItem`'s `@Sendable` completion
        // boundary is safe in practice (the runtime isolates providers per
        // call) but must be opted-in explicitly under Swift 6.
        nonisolated(unsafe) let providers = items.flatMap { $0.attachments ?? [] }

        @Sendable func tryURLProviders(_ providers: [NSItemProvider]) {
            guard let provider = providers.first(where: {
                $0.hasItemConformingToTypeIdentifier(UTType.url.identifier)
            }) else {
                tryTextProviders(providers)
                return
            }
            nonisolated(unsafe) let capturedProviders = providers
            provider.loadItem(forTypeIdentifier: UTType.url.identifier, options: nil) { item, _ in
                if let url = item as? URL {
                    completion(url)
                } else if let string = item as? String, let url = URL(string: string) {
                    completion(url)
                } else {
                    tryTextProviders(capturedProviders)
                }
            }
        }

        @Sendable func tryTextProviders(_ providers: [NSItemProvider]) {
            guard let provider = providers.first(where: {
                $0.hasItemConformingToTypeIdentifier(UTType.plainText.identifier)
            }) else {
                completion(nil)
                return
            }
            provider.loadItem(forTypeIdentifier: UTType.plainText.identifier, options: nil) { item, _ in
                if let string = item as? String,
                   let url = ShareViewController.firstURL(in: string) {
                    completion(url)
                } else {
                    completion(nil)
                }
            }
        }

        tryURLProviders(providers)
    }

    private nonisolated static func firstURL(in text: String) -> URL? {
        guard let detector = try? NSDataDetector(
            types: NSTextCheckingResult.CheckingType.link.rawValue
        ) else { return nil }
        let range = NSRange(text.startIndex..., in: text)
        return detector.firstMatch(in: text, options: [], range: range)?.url
    }

    private func openMainAppAndFinish() {
        if let url = ShareURLScheme.processShareURL {
            openURLViaResponderChain(url)
        }
        finish()
    }

    /// iOS doesn't expose `openURL(_:)` directly on a Share Extension; the
    /// documented workaround is to walk the responder chain looking for any
    /// responder that responds to the `openURL:` selector.
    private func openURLViaResponderChain(_ url: URL) {
        var responder: UIResponder? = self
        let selector = sel_registerName("openURL:")
        while let r = responder {
            if r.responds(to: selector) {
                _ = r.perform(selector, with: url)
                return
            }
            responder = r.next
        }
    }

    private func finish() {
        extensionContext?.completeRequest(returningItems: [], completionHandler: nil)
    }
}
