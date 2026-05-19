import SwiftUI

// OWNER: Phase-2 Agent C (Settings hub). Replace whole file.

struct SettingsHubView: View {
    @EnvironmentObject private var model: KernelModel

    @State private var showRoadmap = false

    private var appVersion: String {
        Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "1.0"
    }

    var body: some View {
        List {
            // ── Account ───────────────────────────────────────────────────
            Section {
                NavigationLink(destination: AccountsView()) {
                    settingsRow(
                        icon: "person.2.fill",
                        iconColor: ChirpColor.accent,
                        title: "Accounts",
                        subtitle: activeAccountSubtitle
                    )
                }
                .listRowBackground(Color.clear)
                .listRowSeparator(.hidden)
            } header: {
                ChirpSectionHeader(title: "Account")
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

            // ── Encrypted Groups (Marmot) ─────────────────────────────────
            Section {
                MarmotKeyPackageRow()
                    .environmentObject(model)
                    .listRowBackground(Color.clear)
                    .listRowSeparator(.hidden)
            } header: {
                ChirpSectionHeader(title: "Encrypted Groups (Marmot)")
            }

            // ── Developer ─────────────────────────────────────────────────
            Section {
                NavigationLink(destination: DiagnosticsView()) {
                    settingsRow(
                        icon: "waveform.path.ecg",
                        iconColor: Color(red: 0.20, green: 0.78, blue: 0.55),
                        title: "Diagnostics",
                        subtitle: "Kernel rev \(model.rev) · \(model.snapshotCount) snapshots"
                    )
                }
                .listRowBackground(Color.clear)
                .listRowSeparator(.hidden)
            } header: {
                ChirpSectionHeader(title: "Developer")
            }

            // ── About ─────────────────────────────────────────────────────
            Section {
                // App info
                HStack {
                    Image(systemName: "bird.fill")
                        .font(.system(size: 22))
                        .foregroundStyle(ChirpColor.accent)
                        .frame(width: 32)

                    VStack(alignment: .leading, spacing: 2) {
                        Text("Chirp")
                            .font(ChirpFont.headline)
                            .foregroundStyle(ChirpColor.textPrimary)
                        Text("Version \(appVersion) · Nostr client")
                            .font(ChirpFont.caption)
                            .foregroundStyle(ChirpColor.textTertiary)
                    }
                }
                .padding(.vertical, ChirpSpace.xs)
                .listRowBackground(Color.clear)
                .listRowSeparator(.hidden)

                // Roadmap disclosure
                DisclosureGroup(
                    isExpanded: $showRoadmap,
                    content: {
                        VStack(alignment: .leading, spacing: ChirpSpace.m) {
                            roadmapItem(cx: "CX1", title: "DMs", description: "Direct messages via NIP-04 / NIP-17")
                            roadmapItem(cx: "CX2", title: "Wallet", description: "Lightning wallet integration")
                            roadmapItem(cx: "CX3", title: "Signer + Wallet auto-link", description: "Seamless identity ↔ payment binding")
                            roadmapItem(cx: "CX4", title: "Media + Lists", description: "Inline media, curated lists & communities")
                            roadmapItem(cx: "CX5", title: "Push", description: "Real-time push notifications")
                        }
                        .padding(.top, ChirpSpace.s)
                    },
                    label: {
                        Label("Roadmap", systemImage: "map")
                            .font(ChirpFont.callout)
                            .foregroundStyle(ChirpColor.textPrimary)
                    }
                )
                .tint(ChirpColor.accent)
                .listRowBackground(Color.clear)
                .listRowSeparator(.hidden)

            } header: {
                ChirpSectionHeader(title: "About")
            }
        }
        .listStyle(.plain)
        .background(Color(.systemBackground))
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
                    .foregroundStyle(iconColor)
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
                .foregroundStyle(.white)
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
        VStack(alignment: .leading, spacing: ChirpSpace.s) {
            HStack(spacing: ChirpSpace.m) {
                ZStack {
                    RoundedRectangle(cornerRadius: 8, style: .continuous)
                        .fill(statusColor.opacity(0.15))
                        .frame(width: 32, height: 32)
                    Image(systemName: "key.horizontal.fill")
                        .font(.system(size: 15, weight: .semibold))
                        .foregroundStyle(statusColor)
                }

                VStack(alignment: .leading, spacing: 2) {
                    Text("Key package")
                        .font(ChirpFont.callout.weight(.medium))
                        .foregroundStyle(ChirpColor.textPrimary)
                    Text(statusSubtitle)
                        .font(ChirpFont.caption)
                        .foregroundStyle(ChirpColor.textTertiary)
                        .lineLimit(1)
                }

                Spacer()

                if kp.stale {
                    Text("STALE")
                        .font(.system(.caption2, design: .rounded).weight(.bold))
                        .foregroundStyle(ChirpColor.zap)
                        .padding(.horizontal, ChirpSpace.s)
                        .padding(.vertical, 3)
                        .background(ChirpColor.zap.opacity(0.14), in: Capsule())
                }
            }

            Button {
                busy = true
                _ = model.marmot.publishKeyPackage()
                busy = false
            } label: {
                Text(kp.published ? "Rotate key package" : "Publish key package")
                    .font(ChirpFont.callout.weight(.semibold))
                    .foregroundStyle(.white)
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, ChirpSpace.s)
                    .background(
                        model.marmot.isRegistered && !busy
                            ? ChirpColor.accent
                            : ChirpColor.accent.opacity(0.4),
                        in: Capsule())
            }
            .buttonStyle(.plain)
            .disabled(!model.marmot.isRegistered || busy)
        }
        .padding(.vertical, ChirpSpace.xs)
    }

    private var statusColor: Color {
        if !model.marmot.isRegistered { return ChirpColor.textTertiary }
        if kp.stale { return ChirpColor.zap }
        return kp.published ? ChirpColor.positive : ChirpColor.textTertiary
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
