import SwiftUI
import VisionKit

/// Wraps `VNDocumentCameraViewController` — Apple's built-in document scanner.
/// It handles real-time page detection, perspective flattening, and
/// auto-capture when the document is held steady. Exactly the Evernote
/// scan-to-flatten experience.
struct CameraView: UIViewControllerRepresentable {
    enum Result {
        case captured(UIImage)
        case cancelled
    }

    let onResult: @MainActor (Result) -> Void

    func makeUIViewController(context: Context) -> VNDocumentCameraViewController {
        let vc = VNDocumentCameraViewController()
        vc.delegate = context.coordinator
        return vc
    }

    func updateUIViewController(_ uiViewController: VNDocumentCameraViewController, context: Context) {}

    func makeCoordinator() -> Coordinator { Coordinator(onResult: onResult) }

    final class Coordinator: NSObject, VNDocumentCameraViewControllerDelegate {
        let onResult: @MainActor (CameraView.Result) -> Void

        init(onResult: @escaping @MainActor (CameraView.Result) -> Void) {
            self.onResult = onResult
        }

        func documentCameraViewController(
            _ controller: VNDocumentCameraViewController,
            didFinishWith scan: VNDocumentCameraScan
        ) {
            let callback = onResult
            guard scan.pageCount > 0 else {
                Task { @MainActor in callback(.cancelled) }
                return
            }
            let image = scan.imageOfPage(at: 0)
            Task { @MainActor in callback(.captured(image)) }
        }

        func documentCameraViewControllerDidCancel(_ controller: VNDocumentCameraViewController) {
            let callback = onResult
            Task { @MainActor in callback(.cancelled) }
        }

        func documentCameraViewController(
            _ controller: VNDocumentCameraViewController,
            didFailWithError error: Error
        ) {
            let callback = onResult
            Task { @MainActor in callback(.cancelled) }
        }
    }
}
