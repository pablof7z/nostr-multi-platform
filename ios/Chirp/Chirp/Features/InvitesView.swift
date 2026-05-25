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

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 8) {
                Image(systemName: "envelope.badge.fill")
                    .foregroundStyle(ChirpColor.accent)
                Text(welcome.displayName)
                    .font(.headline)
                    .foregroundStyle(.primary)
            }
            // ADR-0032: shell-side abbreviation of the inviter's hex pubkey.
            Text("From \(welcome.inviterNpub.shortHex)")
                .font(.caption)
                .foregroundStyle(.secondary)

            HStack(spacing: 12) {
                Button {
                    model.marmot.acceptWelcome(welcomeIDHex: welcome.idHex)
                } label: {
                    Text("Accept")
                        .font(.callout.weight(.semibold))
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 8)
                }
                .buttonStyle(.borderedProminent)
                .accessibilityIdentifier("marmot-accept-invite-\(welcome.idHex)")

                Button {
                    model.marmot.declineWelcome(welcomeIDHex: welcome.idHex)
                } label: {
                    Text("Decline")
                        .font(.callout.weight(.semibold))
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 8)
                }
                .buttonStyle(.bordered)
            }
        }
        .padding(.vertical, 4)
    }
}
