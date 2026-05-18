import SwiftUI

// MARK: - AdvancedView
//
// Per identity-05-synthesis §4.5. Lead paragraph in body / .secondary, hairline
// divider separates sign-in options from account-management options. Sign-in
// options are listed first; "Start a new account" is last and destructive.

struct AdvancedView: View {

    @Environment(UserIdentityStore.self) private var identity
    @Environment(\.dismiss) private var dismiss
    @State private var startNewConfirm = false

    var body: some View {
        Form {
            Section {
                Text(introCopy)
                    .font(AppTheme.Typography.body)
                    .foregroundStyle(.secondary)
                    .listRowBackground(Color.clear)
            }
            Section {
                NavigationLink {
                    UseMyOwnKeyView(onImportComplete: { dismiss() })
                } label: {
                    advancedRow(
                        title: "Use my own key",
                        subtitle: "Already have an account from another app?",
                        systemImage: "key"
                    )
                }
                NavigationLink {
                    RemoteSignerView()
                } label: {
                    advancedRow(
                        title: "Sign in with a remote signer",
                        subtitle: "Keep your key in a separate signing app.",
                        systemImage: "link.icloud"
                    )
                }
            }
            Section {
                NavigationLink {
                    AccountDetailsView()
                } label: {
                    advancedRow(
                        title: "Account details",
                        subtitle: "Full account ID, public key formats",
                        systemImage: "doc.text.magnifyingglass"
                    )
                }
                Button(role: .destructive) {
                    startNewConfirm = true
                } label: {
                    advancedRow(
                        title: "Start a new account",
                        subtitle: "Replaces the account on this device",
                        systemImage: "arrow.triangle.2.circlepath",
                        destructive: true
                    )
                }
            }
        }
        .navigationTitle("Advanced")
        .navigationBarTitleDisplayMode(.inline)
        .alert("Start a new account?", isPresented: $startNewConfirm) {
            Button("Cancel", role: .cancel) {}
            Button("Start new", role: .destructive) {
                Task { await startNewAccount() }
            }
        } message: {
            Text(startNewMessage)
        }
    }

    // MARK: - Row

    private func advancedRow(
        title: String,
        subtitle: String,
        systemImage: String,
        destructive: Bool = false
    ) -> some View {
        HStack(spacing: AppTheme.Spacing.md) {
            Image(systemName: systemImage)
                .font(AppTheme.Typography.body)
                .foregroundStyle(destructive ? .red : .secondary)
                .frame(width: 24)
            VStack(alignment: .leading, spacing: 2) {
                Text(title)
                    .font(AppTheme.Typography.body)
                    .foregroundStyle(destructive ? .red : .primary)
                Text(subtitle)
                    .font(AppTheme.Typography.caption)
                    .foregroundStyle(.secondary)
            }
        }
    }

    // MARK: - Copy

    private var introCopy: String {
        """
        Most people will never need anything on this page. \
        It's here for people coming from other apps that use the same kind of account.
        """
    }

    /// Body adapts when the user has connected a bunker (per §4.9 — disconnecting
    /// doesn't lose the key, so the trailing line is dropped).
    private var startNewMessage: String {
        let base = """
        This will replace your current account on this device. Anything you've already \
        posted (notes, memories, feedback, clips) stays online but you won't be able \
        to edit it from here anymore.
        """
        if identity.isRemoteSigner {
            return base
        }
        return base + "\n\nIf you have your key saved elsewhere, you can sign back in later under Advanced."
    }

    // MARK: - Actions

    private func startNewAccount() async {
        // Per §4.9: clear → start (silently regenerates a new local key).
        identity.clearIdentity()
        identity.start()
        Haptics.medium()
    }
}
