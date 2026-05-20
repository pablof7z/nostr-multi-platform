import SwiftUI

struct SettingsHubView: View {
    @EnvironmentObject private var model: KernelModel

    @State private var showRoadmap = false

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
                        subtitle: relaySubtitle
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

            Section("Developer") {
                NavigationLink(destination: DiagnosticsView()) {
                    Label("Diagnostics", systemImage: "waveform.path.ecg")
                }
            }

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

                DisclosureGroup("Roadmap", isExpanded: $showRoadmap) {
                    VStack(alignment: .leading, spacing: 12) {
                        Text("DMs — Direct messages via NIP-04 / NIP-17")
                        Text("Wallet — Lightning wallet integration")
                        Text("Signer + Wallet auto-link")
                        Text("Media + Lists")
                        Text("Push — Real-time push notifications")
                    }
                    .font(.footnote)
                    .foregroundStyle(.secondary)
                }
            }
        }
        .scrollContentBackground(.hidden)
        .chirpScreenBackground()
        .navigationTitle("Settings")
    }

    // ── Active account subtitle ───────────────────────────────────────────

    private var relaySubtitle: String {
        let count = model.relayEditRows.count
        return count == 0 ? "No relays configured" : "\(count) relay\(count == 1 ? "" : "s")"
    }

    private var activeAccountSubtitle: String {
        guard let activeID = model.activeAccount,
              let account = model.accounts.first(where: { $0.id == activeID })
        else { return "No active account" }
        return account.displayName.isEmpty ? shortNpub(account.npub) : account.displayName
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

    @ViewBuilder
    private func roadmapItem(cx: String, title: String, description: String) -> some View {
        HStack(alignment: .top, spacing: ChirpSpace.m) {
            Text(cx)
                .font(.system(.caption2, design: .rounded).weight(.bold))
                .foregroundStyle(.primary)
                .padding(.horizontal, 6)
                .padding(.vertical, 3)
                .background(ChirpColor.accent, in: Capsule())
                .fixedSize()

            VStack(alignment: .leading, spacing: 2) {
                Text(title)
                    .font(ChirpFont.callout.weight(.medium))
                    .foregroundStyle(ChirpColor.textPrimary)
                Text(description)
                    .font(ChirpFont.caption)
                    .foregroundStyle(ChirpColor.textSecondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
    }

    private func shortNpub(_ npub: String) -> String {
        guard npub.count >= 16 else { return npub }
        return "\(npub.prefix(10))…\(npub.suffix(6))"
    }
}

// ── Marmot key-package status row ─────────────────────────────────────────
//
// Surfaces the local MLS key-package state (published? · age · stale) and a
// publish / rotate action calling the `publish_key_package` dispatch op.
// Key-package visibility lives in Settings, not a top-level screen, per the
// milestone scope.

private struct MarmotKeyPackageRow: View {
    @EnvironmentObject private var model: KernelModel
    @State private var busy = false

    private var kp: MarmotKeyPackage { model.marmot.keyPackage }

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
            Text(statusSubtitle)
                .font(.caption)
                .foregroundStyle(.secondary)

            Button {
                busy = true
                _ = model.marmot.publishKeyPackage()
                busy = false
            } label: {
                Text(kp.published ? "Rotate key package" : "Publish key package")
            }
            .disabled(!model.marmot.isRegistered || busy)
        }
    }

    private var statusSubtitle: String {
        guard model.marmot.isRegistered else {
            return "Sign in with an nsec to enable"
        }
        guard kp.published else { return "Not published" }
        if let age = kp.ageSecs {
            return "Published · \(ageString(age))\(kp.stale ? " · needs rotation" : "")"
        }
        return "Published"
    }

    private func ageString(_ secs: UInt64) -> String {
        if secs < 60 { return "\(secs)s old" }
        if secs < 3600 { return "\(secs / 60)m old" }
        if secs < 86_400 { return "\(secs / 3600)h old" }
        return "\(secs / 86_400)d old"
    }
}
