import SwiftUI

// OWNER: Phase-2 Agent C (Settings hub). Replace whole file.

struct SettingsHubView: View {
    @EnvironmentObject private var model: KernelModel

    // Relay add fields
    @State private var newRelayURL = ""
    @State private var newRelayRole = "both"  // kernel accepts "read" | "write" | "both"
    @State private var showRoadmap = false

    private let relayRoles = ["both", "read", "write"]

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
                // Existing relay rows
                if model.relayEditRows.isEmpty {
                    HStack {
                        Image(systemName: "antenna.radiowaves.left.and.right")
                            .foregroundStyle(ChirpColor.textTertiary)
                        Text("No relays configured")
                            .font(ChirpFont.callout)
                            .foregroundStyle(ChirpColor.textTertiary)
                    }
                    .padding(.vertical, ChirpSpace.xs)
                    .listRowBackground(Color.clear)
                    .listRowSeparator(.hidden)
                } else {
                    ForEach(model.relayEditRows) { relay in
                        RelayRow(relay: relay)
                            .swipeActions(edge: .trailing, allowsFullSwipe: true) {
                                Button(role: .destructive) {
                                    model.removeRelay(url: relay.url)
                                } label: {
                                    Label("Remove", systemImage: "trash")
                                }
                            }
                            .listRowBackground(Color.clear)
                            .listRowSeparator(.hidden)
                    }
                }

                // Add relay row
                addRelayRow

            } header: {
                ChirpSectionHeader(title: "Relays")
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

    private var activeAccountSubtitle: String {
        guard let activeID = model.activeAccount,
              let account = model.accounts.first(where: { $0.id == activeID })
        else { return "No active account" }
        return account.displayName.isEmpty ? shortNpub(account.npub) : account.displayName
    }

    // ── Add relay row ─────────────────────────────────────────────────────

    private var addRelayRow: some View {
        VStack(alignment: .leading, spacing: ChirpSpace.s) {
            HStack(spacing: ChirpSpace.s) {
                Image(systemName: "plus.circle.fill")
                    .foregroundStyle(ChirpColor.accent)
                    .font(.system(size: 16))

                TextField("wss://relay.example.com", text: $newRelayURL)
                    .font(ChirpFont.mono)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .keyboardType(.URL)
            }

            HStack(spacing: ChirpSpace.m) {
                // Role picker
                Picker("Role", selection: $newRelayRole) {
                    ForEach(relayRoles, id: \.self) { role in
                        Text(role.capitalized).tag(role)
                    }
                }
                .pickerStyle(.segmented)
                .frame(maxWidth: 200)

                Spacer()

                // Add button
                Button {
                    let url = newRelayURL.trimmingCharacters(in: .whitespacesAndNewlines)
                    guard !url.isEmpty else { return }
                    model.addRelay(url: url, role: newRelayRole)
                    newRelayURL = ""
                    newRelayRole = "both"
                } label: {
                    Text("Add")
                        .font(ChirpFont.headline)
                        .foregroundStyle(.white)
                        .padding(.horizontal, ChirpSpace.m)
                        .padding(.vertical, ChirpSpace.s)
                        .background(
                            newRelayURL.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
                                ? ChirpColor.accent.opacity(0.4)
                                : ChirpColor.accent,
                            in: Capsule()
                        )
                }
                .buttonStyle(.plain)
                .disabled(newRelayURL.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }
        }
        .padding(.vertical, ChirpSpace.xs)
        .listRowBackground(Color.clear)
        .listRowSeparator(.hidden)
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

// ── Relay row ─────────────────────────────────────────────────────────────

private struct RelayRow: View {
    let relay: RelayEditRow

    var body: some View {
        HStack(spacing: ChirpSpace.m) {
            Image(systemName: "antenna.radiowaves.left.and.right")
                .foregroundStyle(roleColor)
                .font(.system(size: 14, weight: .medium))
                .frame(width: 20)

            VStack(alignment: .leading, spacing: 2) {
                Text(relay.url)
                    .font(ChirpFont.mono)
                    .foregroundStyle(ChirpColor.textPrimary)
                    .lineLimit(1)
            }

            Spacer()

            // Role badge
            Text(relay.role.capitalized)
                .font(.system(.caption2, design: .rounded).weight(.semibold))
                .foregroundStyle(roleColor)
                .padding(.horizontal, ChirpSpace.s)
                .padding(.vertical, 3)
                .background(roleColor.opacity(0.12), in: Capsule())
        }
        .padding(.vertical, ChirpSpace.xs)
    }

    private var roleColor: Color {
        switch relay.role {
        case "read": return Color.blue
        case "write": return ChirpColor.positive
        default: return ChirpColor.accent  // "both"
        }
    }
}
