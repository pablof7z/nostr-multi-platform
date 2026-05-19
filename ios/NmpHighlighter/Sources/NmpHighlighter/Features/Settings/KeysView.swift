import SwiftUI

struct KeysView: View {
    @State private var nsec: String? = KeychainService.loadNsec()
    @State private var isRevealed = false
    @State private var copiedNsec = false
    @State private var copiedNpub = false
    @Environment(HighlighterStore.self) private var store

    var body: some View {
        List {
            nsecSection
            if store.currentUser != nil {
                npubSection
            }
            warningSection
        }
        .listStyle(.insetGrouped)
        .navigationTitle("Keys")
        .navigationBarTitleDisplayMode(.inline)
    }

    // MARK: - Sections

    @ViewBuilder
    private var nsecSection: some View {
        Section {
            if let nsec {
                VStack(alignment: .leading, spacing: 10) {
                    HStack {
                        Text(isRevealed ? nsec : maskedKey(nsec))
                            .font(.system(.footnote, design: .monospaced))
                            .foregroundStyle(isRevealed ? .primary : .secondary)
                            .lineLimit(isRevealed ? nil : 1)
                            .truncationMode(.middle)
                            .animation(.easeInOut(duration: 0.2), value: isRevealed)

                        Spacer(minLength: 8)

                        Button {
                            withAnimation(.easeInOut(duration: 0.2)) {
                                isRevealed.toggle()
                            }
                        } label: {
                            Image(systemName: isRevealed ? "eye.slash" : "eye")
                                .foregroundStyle(.secondary)
                                .frame(width: 28, height: 28)
                        }
                        .buttonStyle(.plain)
                    }

                    Button {
                        UIPasteboard.general.string = nsec
                        copiedNsec = true
                        Task {
                            try? await Task.sleep(for: .seconds(2))
                            copiedNsec = false
                        }
                    } label: {
                        Label(
                            copiedNsec ? "Copied!" : "Copy Secret Key",
                            systemImage: copiedNsec ? "checkmark" : "doc.on.doc"
                        )
                        .font(.subheadline)
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 6)
                    }
                    .buttonStyle(.glassProminent)
                    .disabled(copiedNsec)
                }
                .padding(.vertical, 6)
            } else {
                HStack {
                    Image(systemName: "exclamationmark.triangle.fill")
                        .foregroundStyle(.orange)
                    Text("Secret key not stored on this device.")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                }
                .padding(.vertical, 4)
            }
        } header: {
            Text("Secret Key (nsec)")
        }
    }

    @ViewBuilder
    private var npubSection: some View {
        if let user = store.currentUser {
            Section {
                VStack(alignment: .leading, spacing: 10) {
                    Text(user.npub)
                        .font(.system(.footnote, design: .monospaced))
                        .foregroundStyle(.secondary)
                        .lineLimit(3)
                        .textSelection(.enabled)

                    Button {
                        UIPasteboard.general.string = user.npub
                        copiedNpub = true
                        Task {
                            try? await Task.sleep(for: .seconds(2))
                            copiedNpub = false
                        }
                    } label: {
                        Label(
                            copiedNpub ? "Copied!" : "Copy Public Key",
                            systemImage: copiedNpub ? "checkmark" : "doc.on.doc"
                        )
                        .font(.subheadline)
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 6)
                    }
                    .buttonStyle(.glass)
                    .disabled(copiedNpub)
                }
                .padding(.vertical, 6)
            } header: {
                Text("Public Key (npub)")
            }
        }
    }

    private var warningSection: some View {
        Section {
            HStack(alignment: .top, spacing: 12) {
                Image(systemName: "exclamationmark.shield.fill")
                    .foregroundStyle(.orange)
                    .font(.title3)
                VStack(alignment: .leading, spacing: 4) {
                    Text("Keep your secret key private")
                        .font(.subheadline.weight(.semibold))
                    Text("Anyone with your nsec can post as you and access your encrypted messages. Store it securely and never paste it into untrusted apps or websites.")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                }
            }
            .padding(.vertical, 4)
        }
    }

    // MARK: - Helpers

    private func maskedKey(_ key: String) -> String {
        guard key.count > 10 else { return String(repeating: "•", count: key.count) }
        return "\(key.prefix(8))••••••••••••••••••••••••\(key.suffix(6))"
    }
}
