import SwiftUI

// ─────────────────────────────────────────────────────────────────────────
// InvitesView — full list of pending MLS group invites.
//
// Pushed as a NavigationLink destination from GroupsView when there are
// pending welcomes. PendingInviteRow handles Accept / Decline — all
// accept/decline logic is delegated to MarmotStore (Rust-side).
// ─────────────────────────────────────────────────────────────────────────

struct InvitesView: View {
    @EnvironmentObject private var model: KernelModel

    var body: some View {
        List {
            ForEach(model.marmot.pendingWelcomes) { welcome in
                PendingInviteRow(welcome: welcome)
                    .environmentObject(model)
            }
        }
        .scrollContentBackground(.hidden)
        .chirpScreenBackground()
        .navigationTitle("Invites")
        .navigationBarTitleDisplayMode(.large)
    }
}

// ── Pending invite row ────────────────────────────────────────────────────

struct PendingInviteRow: View {
    let welcome: MarmotPendingWelcome
    @EnvironmentObject private var model: KernelModel

    @State private var busy = false

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 8) {
                Image(systemName: "envelope.badge.fill")
                    .foregroundStyle(.tint)
                Text(welcome.groupName.isEmpty ? "Group invite" : welcome.groupName)
                    .font(.headline)
                    .foregroundStyle(.primary)
            }
            Text("From \(shortNpub(welcome.inviterNpub))")
                .font(.caption)
                .foregroundStyle(.secondary)

            HStack(spacing: 12) {
                Button {
                    busy = true
                    _ = model.marmot.acceptWelcome(welcomeIDHex: welcome.idHex)
                    busy = false
                } label: {
                    Text("Accept")
                        .font(.callout.weight(.semibold))
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 8)
                }
                .buttonStyle(.borderedProminent)
                .accessibilityIdentifier("marmot-accept-invite-\(welcome.idHex)")

                Button {
                    busy = true
                    _ = model.marmot.declineWelcome(welcomeIDHex: welcome.idHex)
                    busy = false
                } label: {
                    Text("Decline")
                        .font(.callout.weight(.semibold))
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 8)
                }
                .buttonStyle(.bordered)
            }
            .disabled(busy)
            .opacity(busy ? 0.5 : 1.0)
        }
        .padding(.vertical, 4)
    }

    private func shortNpub(_ npub: String) -> String {
        guard npub.count >= 16 else { return npub }
        return "\(npub.prefix(10))…\(npub.suffix(6))"
    }
}
