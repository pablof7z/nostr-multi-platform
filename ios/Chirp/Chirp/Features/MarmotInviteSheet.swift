import SwiftUI

// ─────────────────────────────────────────────────────────────────────────
// MarmotInviteSheet — invite one or more npubs into an existing MLS group.
//
// Presented from MarmotGroupChatView's overflow menu. Calls `invite`;
// surfaces `key_package_unavailable` (`needsDisplay`) inline so the user
// knows which invitees haven't published a key package yet. Rust supplies
// the abbreviated form — Swift only joins the strings.
// ─────────────────────────────────────────────────────────────────────────

struct MarmotInviteSheet: View {
    let group: MarmotGroup

    @EnvironmentObject private var model: KernelModel
    @Environment(\.dismiss) private var dismiss

    @State private var inviteeText = ""
    @State private var errorMessage: String?
    @State private var busy = false

    /// `true` when the user has typed at least one non-whitespace
    /// character. Tokenisation + validation happen Rust-side on dispatch.
    private var hasInviteeText: Bool {
        !inviteeText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    Text("Inviting to \(group.displayName)")
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
                    .disabled(!hasInviteeText || busy)
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
        Task {
            let result = await model.marmot.invite(groupIDHex: group.idHex, inviteeText: inviteeText)
            busy = false
            if result.ok {
                dismiss()
            } else if let needsDisplay = result.needsDisplay, !needsDisplay.isEmpty {
                errorMessage = "Waiting for key packages from \(needsDisplay.joined(separator: ", "))."
            } else {
                errorMessage = result.error ?? "Could not send invites"
            }
        }
    }
}
