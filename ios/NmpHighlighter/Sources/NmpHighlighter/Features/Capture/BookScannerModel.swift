import AVFoundation
import Observation
import UIKit

/// Owns the `AVCaptureSession` for the book scanner: camera permission, EAN
/// metadata detection, detected-box conversion, torch, and the single-fire
/// capture latch. Stop-on-first-hit is the only debounce we need; the session
/// stops running as soon as the scanner hands off.
@MainActor
@Observable
final class BookScannerModel: NSObject {
    enum Permission { case unknown, requesting, denied, granted }

    private(set) var permission: Permission = .unknown
    /// Real-time bounding boxes of detected barcodes, in the preview layer's
    /// coordinate space. Stays ~empty most of the time; used to draw a soft
    /// "I see you" rectangle before the decoder confirms the payload.
    private(set) var detectedBoxes: [CGRect] = []
    private(set) var torchOn = false
    private(set) var locked = false
    private(set) var notABookFlash = false
    /// Seconds that *some* barcode has been visible without decoding to a
    /// valid ISBN. Drives the "Hold steady" tip at ~3s.
    private(set) var visibleButUndecodedSeconds: Double = 0

    let session = AVCaptureSession()

    private let sessionQueue = DispatchQueue(label: "app.highlighter.scanner.session")
    private let metadataQueue = DispatchQueue(label: "app.highlighter.scanner.metadata")
    private var metadataOutput: AVCaptureMetadataOutput?
    private var previewLayerBounds: CGRect = .zero
    private var firstVisibleAt: Date?
    private var tipTimer: Timer?
    private var resultHandler: ((String) -> Void)?

    /// Returns once the camera session is started (or permission is resolved
    /// as denied). `onPayload` fires on the main actor with the raw EAN-13
    /// string for every detection — the caller validates and decides whether
    /// to accept or flash a "not a book" toast.
    func start(onPayload: @escaping @MainActor (String) -> Void) async {
        resultHandler = { payload in
            Task { @MainActor in onPayload(payload) }
        }

        switch AVCaptureDevice.authorizationStatus(for: .video) {
        case .authorized:
            permission = .granted
        case .notDetermined:
            permission = .requesting
            let granted = await AVCaptureDevice.requestAccess(for: .video)
            permission = granted ? .granted : .denied
        case .denied, .restricted:
            permission = .denied
        @unknown default:
            permission = .denied
        }

        guard permission == .granted else { return }

        await configureAndStart()
        startTipTimer()
    }

    func stop() {
        tipTimer?.invalidate()
        tipTimer = nil
        let session = self.session
        sessionQueue.async {
            if session.isRunning { session.stopRunning() }
        }
    }

    /// Latch so the view can stop receiving further callbacks while the
    /// capture session is still winding down.
    func lock() { locked = true }

    func toggleTorch() {
        guard let device = AVCaptureDevice.default(for: .video),
              device.hasTorch,
              device.isTorchAvailable else { return }
        do {
            try device.lockForConfiguration()
            let nextOn = !torchOn
            device.torchMode = nextOn ? .on : .off
            device.unlockForConfiguration()
            torchOn = nextOn
        } catch {
            // Torch can refuse for any number of reasons (low battery, etc).
            // Silently no-op — the scanner still works without it.
        }
    }

    func focus(at devicePoint: CGPoint) {
        guard let device = AVCaptureDevice.default(for: .video) else { return }
        sessionQueue.async {
            do {
                try device.lockForConfiguration()
                if device.isFocusPointOfInterestSupported {
                    device.focusPointOfInterest = devicePoint
                    device.focusMode = .continuousAutoFocus
                }
                if device.isExposurePointOfInterestSupported {
                    device.exposurePointOfInterest = devicePoint
                    device.exposureMode = .continuousAutoExposure
                }
                device.unlockForConfiguration()
            } catch {
                // Focus is a nicety; ignore lock failures.
            }
        }
    }

    /// Fires a brief "not a book" toast + warning haptic when the scanner
    /// rejected a payload because it wasn't a valid Bookland EAN-13. The
    /// haptic fires only on the leading edge of the flash — not once per
    /// metadata frame — so the device doesn't buzz continuously while the
    /// camera stares at a grocery barcode.
    func flashNotABook() {
        if !notABookFlash {
            UINotificationFeedbackGenerator().notificationOccurred(.warning)
        }
        notABookFlash = true
        Task { @MainActor in
            try? await Task.sleep(nanoseconds: 1_800_000_000)
            notABookFlash = false
        }
    }

    // MARK: - Session configuration

    private func configureAndStart() async {
        let session = self.session
        await withCheckedContinuation { (continuation: CheckedContinuation<Void, Never>) in
            sessionQueue.async {
                session.beginConfiguration()
                session.sessionPreset = .high

                // Camera input. Fails silently if the device has no back
                // camera (e.g. simulator) — the permission-denied overlay
                // isn't quite right but the user can still manually enter.
                if let device = AVCaptureDevice.default(.builtInWideAngleCamera, for: .video, position: .back),
                   let input = try? AVCaptureDeviceInput(device: device),
                   session.canAddInput(input) {
                    session.addInput(input)
                }

                let output = AVCaptureMetadataOutput()
                if session.canAddOutput(output) {
                    session.addOutput(output)
                    output.setMetadataObjectsDelegate(self, queue: self.metadataQueue)
                    let types: [AVMetadataObject.ObjectType] = [.ean13, .ean8]
                    output.metadataObjectTypes = types.filter {
                        output.availableMetadataObjectTypes.contains($0)
                    }
                }
                self.metadataOutput = output

                session.commitConfiguration()
                session.startRunning()
                continuation.resume()
            }
        }
    }

    // MARK: - Tip timer

    /// Accumulates seconds-since-first-barcode-seen. Resets whenever nothing
    /// has been visible for a while — the tip only shows when the user IS
    /// trying, just not succeeding.
    private func startTipTimer() {
        tipTimer = Timer.scheduledTimer(withTimeInterval: 0.25, repeats: true) { [weak self] _ in
            Task { @MainActor [weak self] in
                guard let self else { return }
                if self.detectedBoxes.isEmpty {
                    self.firstVisibleAt = nil
                    self.visibleButUndecodedSeconds = 0
                } else {
                    if self.firstVisibleAt == nil { self.firstVisibleAt = Date() }
                    if let since = self.firstVisibleAt {
                        self.visibleButUndecodedSeconds = Date().timeIntervalSince(since)
                    }
                }
            }
        }
    }
}

// MARK: - Metadata delegate

extension BookScannerModel: AVCaptureMetadataOutputObjectsDelegate {
    nonisolated func metadataOutput(
        _ output: AVCaptureMetadataOutput,
        didOutput metadataObjects: [AVMetadataObject],
        from connection: AVCaptureConnection
    ) {
        let codes: [(payload: String, bounds: CGRect)] = metadataObjects.compactMap { obj in
            guard let code = obj as? AVMetadataMachineReadableCodeObject,
                  let payload = code.stringValue else { return nil }
            return (payload, code.bounds)
        }

        Task { @MainActor in
            guard !self.locked else { return }
            self.detectedBoxes = codes.map(\.bounds)
            if let first = codes.first {
                // The raw payload goes to the view; it decides whether it's
                // a book (and calls `lock()` + stop) or a false positive
                // that should flash "not a book".
                self.resultHandler?(first.payload)
            }
        }
    }
}
