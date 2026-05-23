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
            Section {
                NavigationLink(destination: RelaySettingsView()) {
                    settingsRow(
                        icon: "antenna.radiowaves.left.and.right",
                        iconColor: ChirpColor.accent,
                        title: "Relays",
                        // §6/AP1: subtitle is pre-formatted in Rust
                        // (`projections.settings_hub.relays_subtitle`).
                        subtitle: model.settingsHub.relaysSubtitle
                    )
                }
                .listRowBackground(Color.clear)
                .listRowSeparator(.hidden)
            } header: {
                ChirpSectionHeader(title: "Relays")
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
        .scrollContentBackground(.hidden)
        .chirpScreenBackground()
        .navigationTitle("Settings")
    }

    // ── Helpers ───────────────────────────────────────────────────────────

    @ViewBuilder
    private func settingsRow(
        icon: String,
        iconColor: Color,
        title: String,
        subtitle: String
    ) -> some View {
        HStack(spacing: ChirpSpace.m) {
            ZStack {
                RoundedRectangle(cornerRadius: 8, style: .continuous)
                    .fill(iconColor.opacity(0.15))
                    .frame(width: 32, height: 32)
                Image(systemName: icon)
                    .font(.system(size: 15, weight: .semibold))
                    .foregroundStyle(.tint)
            }

            VStack(alignment: .leading, spacing: 2) {
                Text(title)
                    .font(ChirpFont.callout.weight(.medium))
                    .foregroundStyle(ChirpColor.textPrimary)
                Text(subtitle)
                    .font(ChirpFont.caption)
                    .foregroundStyle(ChirpColor.textTertiary)
                    .lineLimit(1)
            }
        }
        .padding(.vertical, ChirpSpace.xs)
    }
}

// ── Marmot key-package status row ─────────────────────────────────────────
//
// Surfaces the local MLS key-package state (subtitle + action label, both
// pre-formatted in `nmp-marmot::projection`) and a publish / rotate action
// calling the `publish_key_package` dispatch op. Key-package visibility lives
// in Settings, not a top-level screen, per the milestone scope.

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
