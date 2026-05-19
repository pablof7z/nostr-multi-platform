import AVFoundation
import SwiftUI
import UIKit

/// Full-screen barcode scanner for book ISBNs. Presented as a sheet from the
/// BookPicker. Owns a single `AVCaptureSession`, draws corner brackets + a
/// live overlay around detected barcodes, and fires `onResult` once with the
/// normalized 13-digit ISBN the moment a valid Bookland EAN-13 decodes.
///
/// Design decisions anchored in the scanner UX brief:
/// - Static reticle (no pulse — pulsing reticles are a webview giveaway).
/// - Torch always visible bottom-left; bookstore lighting is real.
/// - Manual-entry pill bottom-right; also the VoiceOver entry point.
/// - Instant dismiss on valid scan — the BookPicker owns the "arrived" moment.
struct BookScannerView: View {
    /// Fires with the normalized 13-digit ISBN when a barcode scans cleanly,
    /// or with `nil` when the user dismisses without scanning.
    var onResult: (String?) -> Void

    @Environment(\.dismiss) private var dismiss
    @State private var model = BookScannerModel()
    @State private var showManualEntry = false
    @State private var tipVisible = false
    @State private var tipTask: Task<Void, Never>?

    var body: some View {
        ZStack {
            Color.black.ignoresSafeArea()

            switch model.permission {
            case .unknown, .requesting:
                ProgressView().tint(.white)
            case .denied:
                permissionDeniedOverlay
            case .granted:
                scannerContent
            }
        }
        .sheet(isPresented: $showManualEntry) {
            ManualISBNEntryView { isbn in
                showManualEntry = false
                if let isbn { hand(off: isbn) }
            }
            .presentationDetents([.medium])
        }
        .task {
            if UIAccessibility.isVoiceOverRunning {
                // VoiceOver users get routed straight to manual entry — an
                // aim-and-hold camera interaction isn't usable for them.
                showManualEntry = true
                return
            }
            await model.start { payload in
                guard let isbn = ISBNValidator.validate(payload) else {
                    model.flashNotABook()
                    return
                }
                hand(off: isbn)
            }
        }
        .onDisappear {
            tipTask?.cancel()
            model.stop()
        }
    }

    private func hand(off isbn: String) {
        let gen = UINotificationFeedbackGenerator()
        gen.notificationOccurred(.success)
        model.lock()
        model.stop()
        onResult(isbn)
        dismiss()
    }

    // MARK: - Scanner content

    @ViewBuilder
    private var scannerContent: some View {
        ZStack {
            CameraPreviewLayer(session: model.session, onTap: { point in
                model.focus(at: point)
            })
            .ignoresSafeArea()

            ScannerReticleView(seeing: !model.detectedBoxes.isEmpty, locked: model.locked)
                .allowsHitTesting(false)

            VStack {
                topBar
                Spacer()
                if tipVisible {
                    tip
                        .transition(.move(edge: .bottom).combined(with: .opacity))
                }
                bottomBar
            }
            .padding(20)

            if model.notABookFlash {
                notABookToast
                    .transition(.move(edge: .top).combined(with: .opacity))
            }
        }
        .onChange(of: model.visibleButUndecodedSeconds) { _, seconds in
            withAnimation(.easeInOut(duration: 0.2)) {
                tipVisible = seconds >= 3
            }
        }
    }

    private var topBar: some View {
        HStack {
            Button {
                onResult(nil)
                dismiss()
            } label: {
                Image(systemName: "xmark")
                    .font(.body.weight(.semibold))
                    .foregroundStyle(.white)
                    .frame(width: 40, height: 40)
                    .background(.ultraThinMaterial, in: Circle())
            }
            Spacer()
            Text("Scan a book")
                .font(.footnote.weight(.semibold))
                .foregroundStyle(.white.opacity(0.85))
                .padding(.horizontal, 12)
                .padding(.vertical, 6)
                .background(.ultraThinMaterial, in: Capsule())
            Spacer()
            // Balance-spacer matching the dismiss button.
            Color.clear.frame(width: 40, height: 40)
        }
    }

    private var bottomBar: some View {
        HStack(spacing: 12) {
            Button {
                model.toggleTorch()
            } label: {
                Image(systemName: model.torchOn ? "bolt.fill" : "bolt.slash.fill")
                    .font(.body.weight(.semibold))
                    .foregroundStyle(.white)
                    .frame(width: 44, height: 44)
                    .background(.ultraThinMaterial, in: Circle())
            }
            .accessibilityLabel(model.torchOn ? "Turn off flashlight" : "Turn on flashlight")

            Spacer()

            Button {
                showManualEntry = true
            } label: {
                Label("Enter ISBN", systemImage: "keyboard")
                    .font(.callout.weight(.semibold))
                    .foregroundStyle(.white)
                    .padding(.horizontal, 14)
                    .padding(.vertical, 10)
                    .background(.ultraThinMaterial, in: Capsule())
            }
        }
    }

    private var tip: some View {
        Label("Hold steady, or move closer", systemImage: "hand.raised")
            .font(.footnote.weight(.medium))
            .foregroundStyle(.white)
            .padding(.horizontal, 14)
            .padding(.vertical, 10)
            .background(.ultraThinMaterial, in: Capsule())
    }

    private var notABookToast: some View {
        VStack {
            HStack(spacing: 8) {
                Image(systemName: "book.closed.circle")
                Text("That's not a book barcode")
            }
            .font(.footnote.weight(.medium))
            .foregroundStyle(.white)
            .padding(.horizontal, 14)
            .padding(.vertical, 10)
            .background(.ultraThinMaterial, in: Capsule())
            Spacer()
        }
        .padding(.top, 60)
    }

    // MARK: - Permission-denied

    private var permissionDeniedOverlay: some View {
        VStack(spacing: 16) {
            Image(systemName: "book.closed")
                .font(.system(size: 44, weight: .light))
                .foregroundStyle(.white.opacity(0.85))
            Text("Scan a book's barcode")
                .font(.title3.weight(.semibold))
                .foregroundStyle(.white)
            Text("Enable camera access to aim at the back cover and look up the book instantly.")
                .font(.footnote)
                .foregroundStyle(.white.opacity(0.7))
                .multilineTextAlignment(.center)
                .padding(.horizontal, 32)
            Button {
                if let url = URL(string: UIApplication.openSettingsURLString) {
                    UIApplication.shared.open(url)
                }
            } label: {
                Text("Open Settings")
                    .font(.body.weight(.semibold))
                    .foregroundStyle(.black)
                    .padding(.horizontal, 24)
                    .padding(.vertical, 12)
                    .background(Color.white, in: Capsule())
            }
            Button {
                showManualEntry = true
            } label: {
                Text("Enter ISBN instead")
                    .font(.footnote.weight(.medium))
                    .foregroundStyle(.white.opacity(0.8))
            }
            Button {
                onResult(nil)
                dismiss()
            } label: {
                Text("Cancel")
                    .font(.footnote)
                    .foregroundStyle(.white.opacity(0.55))
            }
            .padding(.top, 12)
        }
        .padding()
    }
}

// MARK: - Camera preview

/// Thin UIViewRepresentable wrapping `AVCaptureVideoPreviewLayer`. Forwards
/// taps so the coordinator can issue a focus-POI to the capture device.
struct CameraPreviewLayer: UIViewRepresentable {
    let session: AVCaptureSession
    let onTap: (CGPoint) -> Void

    func makeUIView(context: Context) -> PreviewUIView {
        let view = PreviewUIView()
        view.previewLayer.session = session
        view.previewLayer.videoGravity = .resizeAspectFill
        view.onTap = onTap
        return view
    }

    func updateUIView(_ uiView: PreviewUIView, context: Context) {
        uiView.previewLayer.session = session
    }

    final class PreviewUIView: UIView {
        var onTap: ((CGPoint) -> Void)?

        override class var layerClass: AnyClass { AVCaptureVideoPreviewLayer.self }
        var previewLayer: AVCaptureVideoPreviewLayer { layer as! AVCaptureVideoPreviewLayer }

        override init(frame: CGRect) {
            super.init(frame: frame)
            let tap = UITapGestureRecognizer(target: self, action: #selector(handleTap(_:)))
            addGestureRecognizer(tap)
        }

        required init?(coder: NSCoder) { fatalError("init(coder:) has not been implemented") }

        @objc private func handleTap(_ gr: UITapGestureRecognizer) {
            let point = gr.location(in: self)
            let device = previewLayer.captureDevicePointConverted(fromLayerPoint: point)
            onTap?(device)
        }
    }
}

// MARK: - Reticle + overlay

/// Corner-bracket reticle + any real-time detected-barcode overlay boxes.
/// The reticle flashes `locked` color the moment a valid ISBN decodes (just
/// before the scanner dismisses).
struct ScannerReticleView: View {
    /// Metadata output is reporting *something* — doesn't mean it's a book
    /// yet, but the bracket tint shifts to signal "I see you".
    let seeing: Bool
    /// A valid Bookland ISBN has decoded and the scanner is about to dismiss.
    let locked: Bool

    private let width: CGFloat = 280
    private let height: CGFloat = 160

    var body: some View {
        GeometryReader { proxy in
            let size = proxy.size
            let rect = CGRect(
                x: (size.width - width) / 2,
                y: (size.height - height) / 2,
                width: width,
                height: height
            )

            ZStack {
                // Dim scrim with a rounded cutout over the reticle.
                Color.black.opacity(0.45)
                    .mask {
                        Rectangle()
                            .overlay {
                                RoundedRectangle(cornerRadius: 12)
                                    .frame(width: width + 8, height: height + 8)
                                    .position(x: rect.midX, y: rect.midY)
                                    .blendMode(.destinationOut)
                            }
                            .compositingGroup()
                    }

                CornerBrackets(
                    rect: rect,
                    color: bracketColor,
                    fillFlash: locked
                )

                Text("Point at the back cover")
                    .font(.footnote)
                    .foregroundStyle(.white.opacity(0.75))
                    .position(x: rect.midX, y: rect.maxY + 24)
            }
        }
        .ignoresSafeArea()
        .animation(.easeInOut(duration: 0.18), value: seeing)
        .animation(.easeInOut(duration: 0.12), value: locked)
    }

    private var bracketColor: Color {
        if locked { return Color.highlighterAccent }
        if seeing { return Color.highlighterAccent.opacity(0.7) }
        return .white
    }

    private struct CornerBrackets: View {
        let rect: CGRect
        let color: Color
        let fillFlash: Bool

        private let legLength: CGFloat = 24
        private let stroke: CGFloat = 3

        var body: some View {
            ZStack {
                if fillFlash {
                    RoundedRectangle(cornerRadius: 12)
                        .fill(color.opacity(0.18))
                        .frame(width: rect.width, height: rect.height)
                        .position(x: rect.midX, y: rect.midY)
                }

                Path { path in
                    let tl = CGPoint(x: rect.minX, y: rect.minY)
                    let tr = CGPoint(x: rect.maxX, y: rect.minY)
                    let bl = CGPoint(x: rect.minX, y: rect.maxY)
                    let br = CGPoint(x: rect.maxX, y: rect.maxY)
                    for (c, horiz, vert) in [
                        (tl, CGVector(dx: 1, dy: 0), CGVector(dx: 0, dy: 1)),
                        (tr, CGVector(dx: -1, dy: 0), CGVector(dx: 0, dy: 1)),
                        (bl, CGVector(dx: 1, dy: 0), CGVector(dx: 0, dy: -1)),
                        (br, CGVector(dx: -1, dy: 0), CGVector(dx: 0, dy: -1))
                    ] {
                        path.move(to: c)
                        path.addLine(to: CGPoint(x: c.x + horiz.dx * legLength, y: c.y + horiz.dy * legLength))
                        path.move(to: c)
                        path.addLine(to: CGPoint(x: c.x + vert.dx * legLength, y: c.y + vert.dy * legLength))
                    }
                }
                .stroke(color, style: StrokeStyle(lineWidth: stroke, lineCap: .round))
            }
        }
    }
}
