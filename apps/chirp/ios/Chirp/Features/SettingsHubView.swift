import SwiftUI

struct SettingsHubView: View {
    @EnvironmentObject private var model: KernelModel

    private var appVersion: String {
        Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "1.0"
    }

    var body: some View {
        Form {
            Section("Account") {
                NavigationLink(destination: AccountsView()) {
                    Label("Accounts", systemImage: "person.2.fill")
                }
            }

            // ── Relays ────────────────────────────────────────────────────
            // Native row to match every other section: a `Label` with the
            // SF Symbol in the system tint plus a trailing status value, the
            // way Apple's own Settings surfaces a detail string.
            Section("Relays") {
                NavigationLink(destination: RelaySettingsView()) {
                    HStack {
                        Label("Relays", systemImage: "antenna.radiowaves.left.and.right")
                        Spacer()
                        // Projection-provided status subtitle
                        // (`projections.settings_hub.relays_subtitle`).
                        Text(model.settingsHub.relaysSubtitle)
                            .font(.subheadline)
                            .foregroundStyle(.secondary)
                            .lineLimit(1)
                    }
                }
            }

            Section("Encrypted Groups (Marmot)") {
                MarmotKeyPackageRow()
                    .environmentObject(model)
            }

            #if DEBUG
            Section("Developer") {
                NavigationLink(destination: DiagnosticsView()) {
                    Label("Diagnostics", systemImage: "waveform.path.ecg")
                }
            }
            #endif

            Section("About") {
                Label {
                    Text("Chirp")
                } icon: {
                    Image(systemName: "bird.fill")
                }

                HStack {
                    Text("Version")
                    Spacer()
                    Text(appVersion)
                        .foregroundStyle(.secondary)
                }
            }
        }
        // Tighter inter-section rhythm — the default grouped spacing reads as
        // inflated for a short settings list.
        .listSectionSpacing(.compact)
        .navigationTitle("Settings")
    }
}

// ── Marmot key-package status row ─────────────────────────────────────────
//
// Surfaces the local MLS key-package state and a publish / rotate action
// calling the `publish_key_package` dispatch op. Key-package visibility lives
// in Settings, not a top-level screen, per the milestone scope.
//
// aim.md §2: Rust sends raw data only. Subtitle and button label are derived
// here in the shell from the raw fields (isRegistered, published, ageSecs,
// stale) rather than being pre-formatted by the backend.

private struct MarmotKeyPackageRow: View {
    @EnvironmentObject private var model: KernelModel

    private var kp: MarmotKeyPackage { model.marmot.snapshot.keyPackage }

    private var subtitle: String {
        guard kp.isRegistered else { return "Sign in with an nsec to enable" }
        guard kp.published else { return "Not published" }
        var parts = ["Published"]
        if let secs = kp.ageSecs {
            parts.append(bucketAge(secs))
        }
        if kp.stale { parts.append("needs rotation") }
        return parts.joined(separator: " · ")
    }

    private var actionLabel: String {
        kp.published ? "Rotate key package" : "Publish key package"
    }

    private func bucketAge(_ secs: UInt64) -> String {
        if secs < 60 { return "\(secs)s old" }
        if secs < 3_600 { return "\(secs / 60)m old" }
        if secs < 86_400 { return "\(secs / 3_600)h old" }
        return "\(secs / 86_400)d old"
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                Text("Key package")
                Spacer()
                if kp.stale {
                    Text("Stale")
                        .foregroundStyle(ChirpColor.zap)
                        .font(.caption)
                }
            }
            Text(subtitle)
                .font(.caption)
                .foregroundStyle(.secondary)
                .accessibilityIdentifier("marmot-key-package-status")

            // Dispatch is fire-and-forget per aim.md §2 commandment #3; the
            // result comes back as a refreshed snapshot. No Swift-owned
            // `busy` flag (the prior `busy = true; …; busy = false` never
            // actually showed because the call returned synchronously — see
            // audit SH-5). publishKeyPackage() is fire-and-forget (dispatches
            // on DispatchQueue.global) so there is nothing to discard.
            Button {
                model.marmot.publishKeyPackage()
            } label: {
                Text(actionLabel)
            }
            .disabled(!kp.isRegistered)
            .accessibilityIdentifier("marmot-publish-key-package-button")
        }
    }
}
