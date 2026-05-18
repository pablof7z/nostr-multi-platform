import SwiftUI
import UIKit

// Shake-to-feedback: attach .onShake { } to any view (typically the root).
// The detector uses UIViewControllerRepresentable to hook into UIKit's
// motionEnded, since SwiftUI has no native shake gesture modifier.

extension View {
    func onShake(perform action: @escaping () -> Void) -> some View {
        background(ShakeDetectorRepresentable(action: action))
    }
}

private struct ShakeDetectorRepresentable: UIViewControllerRepresentable {
    let action: () -> Void

    func makeUIViewController(context: Context) -> ShakeDetectorViewController {
        let vc = ShakeDetectorViewController()
        vc.onShake = action
        return vc
    }

    func updateUIViewController(_ uiViewController: ShakeDetectorViewController, context: Context) {
        uiViewController.onShake = action
    }
}

final class ShakeDetectorViewController: UIViewController {
    var onShake: (() -> Void)?

    override var canBecomeFirstResponder: Bool { true }

    override func viewDidLoad() {
        super.viewDidLoad()
        view.backgroundColor = .clear
    }

    override func viewDidAppear(_ animated: Bool) {
        super.viewDidAppear(animated)
        activateFirstResponder()
    }

    override func motionEnded(_ motion: UIEvent.EventSubtype, with event: UIEvent?) {
        if motion == .motionShake {
            DispatchQueue.main.async { [onShake] in onShake?() }
        }
        super.motionEnded(motion, with: event)
    }

    private func activateFirstResponder() {
        guard view.window != nil, !isFirstResponder else { return }
        DispatchQueue.main.async { [weak self] in
            guard let self, self.view.window != nil, !self.isFirstResponder else { return }
            self.becomeFirstResponder()
        }
    }
}
