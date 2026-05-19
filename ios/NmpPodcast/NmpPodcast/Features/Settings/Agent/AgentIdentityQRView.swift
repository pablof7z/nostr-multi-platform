import SwiftUI
import UIKit
import CoreImage
import CoreImage.CIFilterBuiltins

// MARK: - QRCodeView

struct QRCodeView: View {
    let content: String

    /// Shared `CIContext` — creating one per render allocates GPU resources unnecessarily.
    private static let ciContext = CIContext()

    /// Cache the rendered image for this content string.  The QR code for a given
    /// npub is deterministic and never changes, so we generate it at most once per
    /// view lifetime (i.e. once per sheet presentation).
    @State private var cachedImage: UIImage?

    var body: some View {
        Group {
            if let image = cachedImage {
                Image(uiImage: image)
                    .interpolation(.none)
                    .resizable()
                    .scaledToFit()
            }
        }
        .task(id: content) {
            cachedImage = Self.generateQR(content)
        }
    }

    private static func generateQR(_ string: String) -> UIImage? {
        guard let data = string.data(using: .utf8),
              let filter = CIFilter(name: "CIQRCodeGenerator") else { return nil }
        filter.setValue(data, forKey: "inputMessage")
        filter.setValue("M", forKey: "inputCorrectionLevel")
        guard let output = filter.outputImage else { return nil }
        let scaled = output.transformed(by: CGAffineTransform(scaleX: 8, y: 8))
        guard let cgImage = ciContext.createCGImage(scaled, from: scaled.extent) else { return nil }
        return UIImage(cgImage: cgImage)
    }
}

// MARK: - AgentIdentityQRView

struct AgentIdentityQRView: View {

    private enum Layout {
        static let actionButtonHeight: CGFloat = 48
        static let qrInset: CGFloat = 20
        static let dismissIconSize: CGFloat = 14
        static let copiedIconSize: CGFloat = 36
        static let dismissButtonSize: CGFloat = 30
        static let qrImageSize: CGFloat = 260
        static let qrCardSize: CGFloat = 300
        static let actionRowSpacing: CGFloat = 12
        static let headerSpacing: CGFloat = 6
    }

    let npub: String
    let name: String

    @Environment(\.dismiss) private var dismiss
    @State private var copied = false

    var body: some View {
        ZStack {
            // Dimmed blurred background — tapping anywhere dismisses
            Color.black.opacity(0.6)
                .background(.ultraThinMaterial)
                .ignoresSafeArea()
                .onTapGesture { dismiss() }
                .accessibilityLabel("Dismiss")
                .accessibilityAddTraits(.isButton)

            VStack(spacing: AppTheme.Spacing.lg) {
                dismissRow
                headerText
                qrCard
                    .appShadow(AppTheme.Shadow.lifted)
                    .onTapGesture { copyNpub() }
                    .accessibilityLabel("QR code")
                    // Hints describe the effect — VoiceOver already
                    // narrates "double-tap to activate" via the button
                    // trait, so the hint shouldn't repeat the gesture.
                    .accessibilityHint("Copies your npub to the clipboard")
                    .accessibilityAddTraits(.isButton)
                npubCaption
                actionRow
            }
        }
        .statusBarHidden(true)
        .animation(AppTheme.Animation.spring, value: copied)
    }

    // MARK: - Subviews

    private var dismissRow: some View {
        HStack {
            Spacer()
            Button {
                dismiss()
            } label: {
                Image(systemName: "xmark")
                    .font(.system(size: Layout.dismissIconSize, weight: .semibold))
                    .foregroundStyle(.secondary)
                    .frame(width: Layout.dismissButtonSize, height: Layout.dismissButtonSize)
            }
            .accessibilityLabel("Close")
            .glassEffect(.regular.interactive(), in: .circle)
        }
        .padding(.horizontal, AppTheme.Spacing.lg)
    }

    private var headerText: some View {
        VStack(spacing: Layout.headerSpacing) {
            if !name.isEmpty {
                Text(name)
                    .font(AppTheme.Typography.title.weight(.bold))
                    .foregroundStyle(.primary)
            }
            Text("Scan to add as a contact")
                .font(AppTheme.Typography.subheadline)
                .foregroundStyle(.secondary)
        }
    }

    private var qrCard: some View {
        ZStack {
            Color.white
                .clipShape(RoundedRectangle(cornerRadius: AppTheme.Corner.xl))

            QRCodeView(content: npub)
                .frame(width: Layout.qrImageSize, height: Layout.qrImageSize)
                .padding(Layout.qrInset)

            if copied {
                copiedOverlay
                    .transition(.opacity.combined(with: .scale(scale: 0.92)))
            }
        }
        .frame(width: Layout.qrCardSize, height: Layout.qrCardSize)
    }

    private var copiedOverlay: some View {
        ZStack {
            RoundedRectangle(cornerRadius: AppTheme.Corner.xl)
                .fill(.black.opacity(0.55))
            VStack(spacing: AppTheme.Spacing.sm) {
                Image(systemName: "checkmark.circle.fill")
                    .font(.system(size: Layout.copiedIconSize))
                    .foregroundStyle(.white)
                Text("Copied")
                    .font(AppTheme.Typography.headline)
                    .foregroundStyle(.white)
            }
        }
    }

    private var npubCaption: some View {
        VStack(spacing: AppTheme.Spacing.xs) {
            Text(npub)
                .font(AppTheme.Typography.mono)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
                .lineLimit(2)
                .padding(.horizontal, AppTheme.Spacing.xl)

            Text("Tap QR to copy")
                .font(AppTheme.Typography.caption2)
                .foregroundStyle(.tertiary)
        }
    }

    private var actionRow: some View {
        HStack(spacing: Layout.actionRowSpacing) {
            Button {
                copyNpub()
            } label: {
                Label("Copy npub", systemImage: "doc.on.doc")
                    .frame(maxWidth: .infinity)
                    .frame(height: Layout.actionButtonHeight)
            }
            .buttonStyle(.glass)

            // Deep-link-aware share. When a `podcastr://friend/add` URL
            // is buildable, prefer sharing it (recipients on Podcastr tap
            // → AddFriendSheet pre-filled). Falls back to the bare npub
            // string when the URL builder returns nil. Either way wraps
            // a SharePreview so the share sheet header shows the
            // sender's name instead of the raw base32 npub.
            shareLink
                .buttonStyle(.glass)
        }
        .padding(.horizontal, AppTheme.Spacing.lg)
    }

    // MARK: - Share assembly

    /// Branches on whether we can build a `podcastr://friend/add` URL —
    /// SwiftUI's `ShareLink` requires a single Transferable item to
    /// attach a `SharePreview`, so the URL and string variants need
    /// distinct call shapes.
    @ViewBuilder
    private var shareLink: some View {
        let trimmedName = name.trimmingCharacters(in: .whitespacesAndNewlines)
        let preview = SharePreview(
            sharePreviewTitle,
            image: Image(systemName: "person.crop.circle.badge.plus")
        )
        if let url = DeepLinkHandler.friendInviteURL(
            npub: npub,
            name: trimmedName.isEmpty ? nil : trimmedName
        ) {
            ShareLink(item: url, preview: preview) {
                Label("Share", systemImage: "square.and.arrow.up")
                    .frame(maxWidth: .infinity)
                    .frame(height: Layout.actionButtonHeight)
            }
        } else {
            ShareLink(item: npub, preview: preview) {
                Label("Share", systemImage: "square.and.arrow.up")
                    .frame(maxWidth: .infinity)
                    .frame(height: Layout.actionButtonHeight)
            }
        }
    }

    private var sharePreviewTitle: String {
        let trimmedName = name.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmedName.isEmpty ? "Podcastr invite" : "\(trimmedName) on Podcastr"
    }

    // MARK: - Actions

    private func copyNpub() {
        copyToClipboard(npub, isCopied: $copied, haptic: { Haptics.success() })
    }
}
