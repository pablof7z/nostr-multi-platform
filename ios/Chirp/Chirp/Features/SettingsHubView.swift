import SwiftUI

struct SettingsHubView: View {
    @EnvironmentObject private var model: KernelModel

    @State private var newRelayURL = ""
    @State private var newRelayRole = "both"
    @State private var showRoadmap = false

    private let relayRoles = ["both", "read", "write"]

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

            Section("Relays") {
                if model.relayEditRows.isEmpty {
                    Text("No relays configured")
                        .foregroundStyle(.secondary)
                } else {
                    ForEach(model.relayEditRows) { relay in
                        HStack {
                            Text(relay.url)
                                .lineLimit(1)
                            Spacer()
                            Text(relay.role.capitalized)
                                .foregroundStyle(.secondary)
                                .font(.caption)
                        }
                        .swipeActions(edge: .trailing, allowsFullSwipe: true) {
                            Button(role: .destructive) {
                                model.removeRelay(url: relay.url)
                            } label: {
                                Label("Remove", systemImage: "trash")
                            }
                        }
                    }
                }

                TextField("wss://relay.example.com", text: $newRelayURL)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .keyboardType(.URL)

                Picker("Role", selection: $newRelayRole) {
                    ForEach(relayRoles, id: \.self) { role in
                        Text(role.capitalized).tag(role)
                    }
                }
                .pickerStyle(.segmented)

                Button {
                    let url = newRelayURL.trimmingCharacters(in: .whitespacesAndNewlines)
                    guard !url.isEmpty else { return }
                    model.addRelay(url: url, role: newRelayRole)
                    newRelayURL = ""
                    newRelayRole = "both"
                } label: {
                    Label("Add Relay", systemImage: "plus")
                }
                .disabled(newRelayURL.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
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
        .navigationTitle("Settings")
    }
}

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
                        .foregroundStyle(.orange)
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
