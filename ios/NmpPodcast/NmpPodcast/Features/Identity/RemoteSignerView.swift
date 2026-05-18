import SwiftUI

// MARK: - RemoteSignerView
//
// Per identity-05-synthesis §4.7. Promotes `Nip46ConnectCard` to a primary
// push surface — the card paints itself in `.primary` mode (no outer chrome,
// no header glyph, no footnote). This view supplies the title, the prose
// intro, and the trailing footnote about keys never touching the device.

struct RemoteSignerView: View {

    @Environment(UserIdentityStore.self) private var identity
    @State private var bunkerInput = ""
    @State private var isConnecting = false
    @State private var showNostrConnect = false

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: AppTheme.Spacing.lg) {
                preface

                scanToConnectRow

                Divider()

                Nip46ConnectCard(
                    bunkerInput: $bunkerInput,
                    isConnectingRemote: $isConnecting,
                    connect: { await connect() },
                    disconnect: { await identity.disconnectRemoteSigner() },
                    presentation: .primary
                )

                footnote
            }
            .padding(AppTheme.Spacing.lg)
        }
        .navigationTitle("Remote signer")
        .navigationBarTitleDisplayMode(.inline)
        .background(Color(.systemBackground))
        .navigationDestination(isPresented: $showNostrConnect) {
            NostrConnectView()
        }
    }

    // MARK: - Sections

    private var scanToConnectRow: some View {
        Button {
            showNostrConnect = true
        } label: {
            HStack(spacing: AppTheme.Spacing.sm) {
                Image(systemName: "qrcode.viewfinder")
                    .font(.system(size: 22))
                    .foregroundStyle(Color.accentColor)
                VStack(alignment: .leading, spacing: 2) {
                    Text("Scan to connect")
                        .font(AppTheme.Typography.headline)
                        .foregroundStyle(.primary)
                    Text("Generate a QR code your signer app can scan")
                        .font(AppTheme.Typography.caption)
                        .foregroundStyle(.secondary)
                }
                Spacer()
                Image(systemName: "chevron.forward")
                    .font(.system(size: 13, weight: .semibold))
                    .foregroundStyle(.tertiary)
            }
            .padding(AppTheme.Spacing.md)
            .background(Color(.secondarySystemBackground), in: RoundedRectangle(cornerRadius: AppTheme.Corner.md))
        }
        .buttonStyle(.plain)
    }

    private var preface: some View {
        VStack(alignment: .leading, spacing: AppTheme.Spacing.sm) {
            Text("Some people prefer to keep their key in a separate signing app — like Amber or nsec.app — and let other apps ask permission to post. Podcastr supports this.")
                .font(AppTheme.Typography.body)
                .foregroundStyle(.primary)

            Text("Open your signer app, find \u{201C}connect a new app\u{201D} (it might say \u{201C}bunker\u{201D}), and paste the link here.")
                .font(AppTheme.Typography.body)
                .foregroundStyle(.secondary)
        }
    }

    private var footnote: some View {
        Text("Your private key never touches this device — every signature happens inside your signer app.")
            .font(AppTheme.Typography.caption)
            .foregroundStyle(.tertiary)
    }

    // MARK: - Connect

    private func connect() async {
        let trimmed = bunkerInput.trimmed
        guard !trimmed.isEmpty else { return }
        isConnecting = true
        await identity.connectRemoteSigner(uri: trimmed)
        isConnecting = false
        if identity.isRemoteSigner {
            bunkerInput = ""
            Haptics.success()
        } else {
            Haptics.error()
        }
    }
}
