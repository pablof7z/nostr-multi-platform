import CoreImage.CIFilterBuiltins
import Kingfisher
import SwiftUI

/// "Whoever shows up" half of the invite screen. A paper-styled card with
/// the room's cover, name, an invite URL, a Copy button, and an
/// expandable QR. Mirrors the existing room-card aesthetic — accent
/// gradient fallback, ink-on-paper QR rather than black-on-white.
///
/// The shareable URL is `https://highlighter.com/r/<id>/join/<code>` where
/// `<code>` is a single-use NIP-29 invite minted on appear (kind:9009).
/// relay29 consumes codes on first use, so each card render produces a
/// fresh code suitable for one new member. For batch sharing, admins use
/// the web `/r/<id>/invite` page.
struct RoomShareCard: View {
    let groupId: String
    let room: CommunitySummary?

    @Environment(HighlighterStore.self) private var appStore

    @State private var qrShown = false
    @State private var copied = false
    @State private var inviteCode: String?
    @State private var mintError: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            heroBackdrop

            VStack(alignment: .leading, spacing: 14) {
                if let room {
                    Text(room.name)
                        .font(.system(.title3, design: .default).weight(.semibold))
                        .foregroundStyle(Color.highlighterInkStrong)
                        .lineLimit(1)
                } else {
                    Text("New room")
                        .font(.system(.title3, design: .default).weight(.semibold))
                        .foregroundStyle(Color.highlighterInkStrong)
                }

                HStack(spacing: 8) {
                    Image(systemName: "link")
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(Color.highlighterInkMuted)
                    Text(linkLabel)
                        .font(.system(.caption, design: .monospaced))
                        .foregroundStyle(Color.highlighterInkMuted)
                        .lineLimit(1)
                        .truncationMode(.middle)
                }

                HStack(spacing: 10) {
                    Button(action: copy) {
                        Label(copied ? "Copied" : "Copy link",
                              systemImage: copied ? "checkmark" : "doc.on.doc")
                            .font(.subheadline.weight(.medium))
                            .foregroundStyle(Color.highlighterInkStrong)
                            .padding(.horizontal, 14)
                            .padding(.vertical, 9)
                            .background(
                                Capsule().fill(Color.highlighterTintPale)
                            )
                    }
                    .buttonStyle(.plain)
                    .disabled(shareURL == nil)
                    .opacity(shareURL == nil ? 0.5 : 1)

                    if let url = URL(string: shareURL ?? "") {
                        ShareLink(item: url) {
                            Label("Share", systemImage: "square.and.arrow.up")
                                .font(.subheadline.weight(.medium))
                                .foregroundStyle(Color.highlighterInkStrong)
                                .padding(.horizontal, 14)
                                .padding(.vertical, 9)
                                .background(
                                    Capsule().fill(Color.highlighterTintPale)
                                )
                        }
                    }

                    Spacer(minLength: 0)

                    Button {
                        withAnimation(.easeInOut(duration: 0.25)) { qrShown.toggle() }
                    } label: {
                        Image(systemName: qrShown ? "qrcode.viewfinder" : "qrcode")
                            .font(.title3)
                            .foregroundStyle(Color.highlighterAccent)
                            .padding(9)
                            .background(
                                Circle().fill(Color.highlighterTintPale)
                            )
                    }
                    .buttonStyle(.plain)
                    .disabled(shareURL == nil)
                    .opacity(shareURL == nil ? 0.4 : 1)
                    .accessibilityLabel(qrShown ? "Hide QR" : "Show QR")
                }

                if let mintError {
                    Text(mintError)
                        .font(.caption)
                        .foregroundStyle(Color.highlighterAccent)
                }

                if qrShown, let _ = shareURL {
                    qrView
                        .transition(.opacity.combined(with: .move(edge: .top)))
                }
            }
            .padding(18)
        }
        .background(Color.highlighterPaper)
        .overlay(
            RoundedRectangle(cornerRadius: 18)
                .stroke(Color.highlighterRule, lineWidth: 1)
        )
        .clipShape(RoundedRectangle(cornerRadius: 18))
        .task(id: groupId) {
            await mintInviteIfNeeded()
        }
    }

    private var shareURL: String? {
        guard let code = inviteCode else { return nil }
        return "https://highlighter.com/r/\(groupId)/join/\(code)"
    }

    private var linkLabel: String {
        if let url = shareURL { return url }
        if mintError != nil { return "Couldn't create invite link" }
        return "Creating invite link…"
    }

    private func mintInviteIfNeeded() async {
        guard inviteCode == nil else { return }
        do {
            let codes = try await appStore.safeCore.createRoomInviteCodes(
                groupId: groupId,
                count: 1
            )
            await MainActor.run {
                inviteCode = codes.first
                mintError = inviteCode == nil ? "No code returned." : nil
            }
        } catch {
            await MainActor.run {
                mintError = "Couldn't mint invite link. Add people directly below."
            }
        }
    }

    private func copy() {
        guard let url = shareURL else { return }
        UIPasteboard.general.string = url
        UISelectionFeedbackGenerator().selectionChanged()
        copied = true
        Task {
            try? await Task.sleep(for: .seconds(2))
            await MainActor.run { copied = false }
        }
    }

    @ViewBuilder
    private var heroBackdrop: some View {
        ZStack {
            if let url = URL(string: room?.picture ?? ""),
               !(room?.picture ?? "").isEmpty {
                KFImage(url)
                    .resizable()
                    .scaledToFill()
            } else {
                LinearGradient(
                    colors: [
                        Color.highlighterAccent.opacity(0.72),
                        Color.highlighterAccent.opacity(0.36),
                    ],
                    startPoint: .topLeading,
                    endPoint: .bottomTrailing
                )
            }
        }
        .frame(height: 110)
        .frame(maxWidth: .infinity)
        .clipped()
    }

    @ViewBuilder
    private var qrView: some View {
        if let url = shareURL, let image = QRCodeGenerator.image(for: url) {
            HStack {
                Spacer()
                VStack(spacing: 8) {
                    Image(uiImage: image)
                        .interpolation(.none)
                        .resizable()
                        .scaledToFit()
                        .frame(width: 200, height: 200)
                        .padding(12)
                        .background(Color.highlighterPaper)
                        .overlay(
                            RoundedRectangle(cornerRadius: 12)
                                .stroke(Color.highlighterRule, lineWidth: 1)
                        )
                    Text("Scan to join")
                        .font(.caption)
                        .foregroundStyle(Color.highlighterInkMuted)
                }
                Spacer()
            }
            .padding(.top, 6)
        }
    }
}

private enum QRCodeGenerator {
    static func image(for string: String) -> UIImage? {
        let context = CIContext()
        let filter = CIFilter.qrCodeGenerator()
        filter.message = Data(string.utf8)
        filter.correctionLevel = "M"
        guard let output = filter.outputImage else { return nil }

        // Tint to ink-on-paper instead of black-on-white. We feed the QR
        // through `CIFalseColor` so the dark bits become highlighter ink and
        // the light bits become paper — matches the rest of the app and
        // doesn't look like every other generic share QR.
        let inkComponents = UIColor(Color.highlighterInkStrong).cgColor.components ?? [0, 0, 0, 1]
        let paperComponents = UIColor(Color.highlighterPaper).cgColor.components ?? [1, 1, 1, 1]
        let ink = CIColor(
            red: inkComponents[safe: 0] ?? 0,
            green: inkComponents[safe: 1] ?? 0,
            blue: inkComponents[safe: 2] ?? 0
        )
        let paper = CIColor(
            red: paperComponents[safe: 0] ?? 1,
            green: paperComponents[safe: 1] ?? 1,
            blue: paperComponents[safe: 2] ?? 1
        )
        let colored = output.applyingFilter("CIFalseColor", parameters: [
            "inputColor0": ink,
            "inputColor1": paper,
        ])

        let scale: CGFloat = 10
        let scaled = colored.transformed(by: CGAffineTransform(scaleX: scale, y: scale))
        guard let cg = context.createCGImage(scaled, from: scaled.extent) else { return nil }
        return UIImage(cgImage: cg)
    }
}

private extension Array where Element == CGFloat {
    subscript(safe index: Int) -> CGFloat? {
        indices.contains(index) ? self[index] : nil
    }
}
