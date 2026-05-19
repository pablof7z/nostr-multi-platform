import SwiftUI

// ─────────────────────────────────────────────────────────────────────────
// MarmotInviteSheet — invite one or more npubs into an existing MLS group.
//
// Presented from MarmotGroupChatView's overflow menu. Mirrors the
// create-group sheet's composer idiom (GlassCard + TextEditor +
// ChirpPrimaryButton). Calls `invite`; surfaces `key_package_unavailable`
// (`needs`) inline so the user knows which invitees haven't published a
// key package yet.
// ─────────────────────────────────────────────────────────────────────────

struct MarmotInviteSheet: View {
    let group: MarmotGroup

    @EnvironmentObject private var model: KernelModel
    @Environment(\.dismiss) private var dismiss

    @State private var inviteeText = ""
    @State private var errorMessage: String?
    @State private var busy = false

    private var invitees: [String] {
        inviteeText
            .split(whereSeparator: { $0 == "," || $0 == "\n" || $0 == " " })
            .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
            .filter { !$0.isEmpty }
    }

    var body: some View {
        NavigationStack {
            ZStack {
                Color(.systemBackground).ignoresSafeArea()
                ScrollView {
                    VStack(alignment: .leading, spacing: ChirpSpace.l) {
                        Text("Inviting to \(group.name.isEmpty ? "this group" : group.name)")
                            .font(ChirpFont.callout)
                            .foregroundStyle(ChirpColor.textSecondary)
                            .padding(.horizontal, ChirpSpace.l)

                        GlassCard {
                            VStack(alignment: .leading, spacing: ChirpSpace.xs) {
                                Text("Invitee npubs")
                                    .font(ChirpFont.caption)
                                    .foregroundStyle(ChirpColor.textTertiary)
                                TextEditor(text: $inviteeText)
                                    .font(ChirpFont.mono)
                                    .scrollContentBackground(.hidden)
                                    .frame(minHeight: 120)
                                    .textInputAutocapitalization(.never)
                                    .autocorrectionDisabled()
                                    .overlay(alignment: .topLeading) {
                                        if inviteeText.isEmpty {
                                            Text("npub1…, npub1… (comma or newline separated)")
                                                .font(ChirpFont.mono)
                                                .foregroundStyle(ChirpColor.textTertiary)
                                                .allowsHitTesting(false)
                                                .padding(.top, 8)
                                        }
                                    }
                            }
                        }
                        .padding(.horizontal, ChirpSpace.l)

                        if let errorMessage {
                            Text(errorMessage)
                                .font(ChirpFont.caption)
                                .foregroundStyle(ChirpColor.like)
                                .padding(.horizontal, ChirpSpace.l)
                        }

                        ChirpPrimaryButton(title: "Send invites",
                                           systemImage: "person.badge.plus") {
                            sendInvites()
                        }
                        .disabled(invitees.isEmpty || busy)
                        .opacity(invitees.isEmpty || busy ? 0.45 : 1.0)
                        .padding(.horizontal, ChirpSpace.l)
                        .padding(.bottom, ChirpSpace.xl)
                    }
                    .padding(.top, ChirpSpace.l)
                }
            }
            .navigationTitle("Invite")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button("Cancel") { dismiss() }
                        .font(ChirpFont.callout)
                        .foregroundStyle(ChirpColor.textSecondary)
                }
            }
        }
    }

    private func sendInvites() {
        busy = true
        errorMessage = nil
        let result = model.marmot.invite(groupIDHex: group.idHex, inviteeNpubs: invitees)
        busy = false
        if result.ok {
            dismiss()
        } else if let needs = result.needs, !needs.isEmpty {
            // Key packages are being fetched in the background (invite() triggered it).
            // Instruct the user to retry momentarily.
            errorMessage = "Fetching key packages for \(needs.map(shortNpub).joined(separator: ", "))… tap Send again in a moment."
        } else {
            errorMessage = result.error ?? "Could not send invites"
        }
    }

    private func shortNpub(_ npub: String) -> String {
        guard npub.count >= 16 else { return npub }
        return "\(npub.prefix(8))…\(npub.suffix(4))"
    }
}
