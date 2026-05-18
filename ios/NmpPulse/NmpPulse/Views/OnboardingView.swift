import SwiftUI

/// Screen 1 — first-launch entry. No data yet.
///
/// Dispatches `nmp_app_signin_nsec` / `nmp_app_signin_bunker` /
/// `nmp_app_create_new_account`. The kernel owns all identity state; this
/// screen only collects input and reads back `model.lastErrorToast` (D6) and
/// `model.hasActiveAccount` (auto-navigate is RootView's job).
struct OnboardingView: View {
    @EnvironmentObject private var model: KernelModel

    @State private var nsecField = ""
    @State private var bunkerField = ""
    @State private var showNsecSheet = false
    @State private var showBunkerSheet = false

    var body: some View {
        VStack(spacing: 28) {
            Spacer()

            VStack(spacing: 8) {
                Image(systemName: "waveform.path.ecg")
                    .font(.system(size: 54))
                    .foregroundStyle(.tint)
                Text("Pulse")
                    .font(.largeTitle).bold()
                Text("e2e validation client for the NMP kernel")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
            }

            Spacer()

            VStack(spacing: 14) {
                Button {
                    showNsecSheet = true
                } label: {
                    Label("Paste nsec", systemImage: "key.fill")
                        .frame(maxWidth: .infinity)
                }
                .buttonStyle(.borderedProminent)

                Button {
                    showBunkerSheet = true
                } label: {
                    Label("Connect bunker", systemImage: "link")
                        .frame(maxWidth: .infinity)
                }
                .buttonStyle(.bordered)

                Button {
                    model.createAccount()
                } label: {
                    Label("Create new account", systemImage: "plus.circle")
                        .frame(maxWidth: .infinity)
                }
                .buttonStyle(.bordered)
            }
            .padding(.horizontal, 32)

            if let toast = model.lastErrorToast {
                Text(toast)
                    .font(.caption)
                    .foregroundStyle(.red)
                    .multilineTextAlignment(.center)
                    .padding(.horizontal, 24)
                    .transition(.opacity)
            }

            Spacer()
        }
        .animation(.default, value: model.lastErrorToast)
        .sheet(isPresented: $showNsecSheet) {
            SecretEntrySheet(
                title: "Paste nsec",
                prompt: "nsec1… or 64-char hex secret key",
                field: $nsecField
            ) { value in
                model.signInNsec(value)
                showNsecSheet = false
            }
        }
        .sheet(isPresented: $showBunkerSheet) {
            SecretEntrySheet(
                title: "Connect bunker",
                prompt: "bunker://<pubkey>?relay=wss://…",
                field: $bunkerField
            ) { value in
                model.signInBunker(value)
                showBunkerSheet = false
            }
        }
    }
}

/// Reusable modal text-entry sheet for nsec / bunker URIs.
private struct SecretEntrySheet: View {
    let title: String
    let prompt: String
    @Binding var field: String
    let onSubmit: (String) -> Void

    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            VStack(spacing: 20) {
                Text(prompt)
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, alignment: .leading)

                TextField(prompt, text: $field, axis: .vertical)
                    .textFieldStyle(.roundedBorder)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .lineLimit(2...4)

                Button("Sign in") {
                    let trimmed = field.trimmingCharacters(in: .whitespacesAndNewlines)
                    guard !trimmed.isEmpty else { return }
                    onSubmit(trimmed)
                }
                .buttonStyle(.borderedProminent)
                .disabled(field.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)

                Spacer()
            }
            .padding(24)
            .navigationTitle(title)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                }
            }
        }
        .presentationDetents([.medium])
    }
}
