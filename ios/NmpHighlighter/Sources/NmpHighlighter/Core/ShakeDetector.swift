import SwiftUI
import UIKit

/// SwiftUI shake-gesture hook. Attach `.onShake { ... }` to any view; the
/// closure fires when iOS reports a `.motionShake` motion event. Backed by a
/// `UIViewControllerRepresentable` whose hosted controller becomes first
/// responder so `motionEnded(_:with:)` is delivered. Motion events bubble up
/// the responder chain, so this works even when sheets / text fields are
/// foregrounded — including when a fresh shake should re-open a sheet that's
/// already up (the parent caller debounces).
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

private final class ShakeDetectorViewController: UIViewController {
    var onShake: (() -> Void)?

    override var canBecomeFirstResponder: Bool { true }

    override func viewDidAppear(_ animated: Bool) {
        super.viewDidAppear(animated)
        becomeFirstResponder()
    }

    override func motionEnded(_ motion: UIEvent.EventSubtype, with event: UIEvent?) {
        if motion == .motionShake {
            onShake?()
        }
        super.motionEnded(motion, with: event)
    }
}
