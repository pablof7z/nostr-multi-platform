import SwiftUI

// ─────────────────────────────────────────────────────────────────────────
// MarmotInviteSheet — invite one or more npubs into an existing MLS group.
//
// Presented from MarmotGroupChatView's overflow menu. Mirrors the
// create-group sheet's composer idiom (TextEditor). Calls `invite`;
// surfaces `key_package_unavailable` (`needs`) inline so the user knows
// which invitees haven't published a key package yet.
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
            Form {
                Section {
                    Text("Inviting to \(group.name.isEmpty ? "this group" : group.name)")
                        .font(.callout)
                        .foregroundStyle(.secondary)

                    VStack(alignment: .leading, spacing: 8) {
                        Text("Invitee npubs")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                        TextEditor(text: $inviteeText)
                            .font(.body.monospaced())
                            .frame(minHeight: 120)
                            .textInputAutocapitalization(.never)
                            .autocorrectionDisabled()
                            .overlay(alignment: .topLeading) {
                                if inviteeText.isEmpty {
                                    Text("npub1…, npub1… (comma or newline separated)")
                                        .font(.body.monospaced())
                                        .foregroundStyle(.secondary)
                                        .allowsHitTesting(false)
                                        .padding(.top, 8)
                                }
                            }
                    }
                }

                if let errorMessage {
                    Section {
                        Text(errorMessage)
                            .font(.caption)
                            .foregroundStyle(.red)
                    }
                }

                Section {
                    Button {
                        sendInvites()
                    } label: {
                        Label("Send invites", systemImage: "person.badge.plus")
                    }
                    .disabled(invitees.isEmpty || busy)
                }
            }
            .scrollContentBackground(.hidden)
            .chirpScreenBackground()
            .navigationTitle("Invite")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button("Cancel") { dismiss() }
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
