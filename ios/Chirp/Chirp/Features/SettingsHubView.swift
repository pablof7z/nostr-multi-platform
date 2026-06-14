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
// Surfaces the local MLS key-package state (subtitle + action label from
// `nmp-marmot::projection`) and a publish / rotate action calling the
// `publish_key_package` dispatch op. Key-package visibility lives in Settings,
// not a top-level screen, per the milestone scope.

private struct MarmotKeyPackageRow: View {
    @EnvironmentObject private var model: KernelModel

    private var snapshot: MarmotSnapshot { model.marmot.snapshot }
    private var kp: MarmotKeyPackage { snapshot.keyPackage }

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
            Text(kp.subtitle)
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
                Text(kp.actionLabel)
            }
            .disabled(!snapshot.isRegistered)
            .accessibilityIdentifier("marmot-publish-key-package-button")
        }
    }
}
