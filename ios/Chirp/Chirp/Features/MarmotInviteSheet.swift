import SwiftUI

// ─────────────────────────────────────────────────────────────────────────
// MarmotInviteSheet — invite one or more npubs into an existing MLS group.
//
// Sheet stays OPEN after dispatch until the kernel reports a terminal verdict:
//
//   1. Dispatch fires → `pendingCid` stashed.
//   2. While parked in `snapshot.pendingOps`, render Rust's `displayLabel`.
//   3. `recentTerminal` returning `.accepted` → dismiss.
//   4. `.failed(reason)` → show reason verbatim, keep sheet (retry/cancel).
//
// Thin-shell rule: ALL copy comes from Rust. Swift only gates dismissal.
// ─────────────────────────────────────────────────────────────────────────

struct MarmotInviteSheet: View {
    let group: MarmotGroup

    @EnvironmentObject private var model: KernelModel
    @Environment(\.dismiss) private var dismiss

    @State private var inviteeText = ""

    // ── Async feedback state ──────────────────────────────────────────────
    @State private var pendingCid: String?
    @State private var syncError: String?

    private var hasInviteeText: Bool {
        !inviteeText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    private var pendingOpRow: MarmotPendingOp? {
        guard let cid = pendingCid else { return nil }
        return model.marmot.snapshot.pendingOps.first { $0.correlationId == cid }
    }

    private var isWaiting: Bool { pendingCid != nil }

    var body: some View {
        NavigationStack {
            Form {
                inputSection
                feedbackSection
                actionSection
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
            .onChange(of: model.actionLifecycle) { resolveTerminal() }
        }
    }

    // ── Sections ──────────────────────────────────────────────────────────

    private var inputSection: some View {
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
                    .disabled(isWaiting)
                    .overlay(alignment: .topLeading) {
                        if inviteeText.isEmpty {
                            Text("npub1\u{2026}, npub1\u{2026} (comma or newline separated)")
                                .font(.body.monospaced())
                                .foregroundStyle(.secondary)
                                .allowsHitTesting(false)
                                .padding(.top, 8)
                        }
                    }
            }
        }
    }

    @ViewBuilder
    private var feedbackSection: some View {
        if let row = pendingOpRow {
            Section {
                HStack(spacing: ChirpSpace.s) {
                    ProgressView().controlSize(.small)
                    Text(row.displayLabel)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
        }
        if let err = syncError {
            Section {
                Text(err)
                    .font(.caption)
                    .foregroundStyle(ChirpColor.danger)
            }
        }
    }

    @ViewBuilder
    private var actionSection: some View {
        Section {
            if isWaiting, pendingOpRow == nil {
                HStack(spacing: ChirpSpace.s) {
                    ProgressView().controlSize(.small)
                    Text("Sending invites\u{2026}")
                        .font(.callout)
                        .foregroundStyle(.secondary)
                }
            } else if !isWaiting {
                Button { sendInvites() } label: {
                    Label("Send invites", systemImage: "person.badge.plus")
                }
                .disabled(!hasInviteeText)
            }
        }
    }

    // ── Helpers ───────────────────────────────────────────────────────────

    private func sendInvites() {
        syncError = nil
        pendingCid = nil
        Task {
            let result = await model.marmot.invite(
                groupIDHex: group.idHex,
                inviteeText: inviteeText)
            if result.ok, let cid = result.correlationId {
                pendingCid = cid
                resolveTerminal()
            } else {
                syncError = result.error ?? "Could not send invites"
            }
        }
    }

    private func resolveTerminal() {
        guard let cid = pendingCid,
              let entry = model.recentTerminal(correlationId: cid) else { return }
        pendingCid = nil
        switch entry.stage {
        case .accepted:
            dismiss()
        case .failed:
            // #1735: prefer the localized reason_code, falling back to the
            // English prose `reason` the wire carries.
            let reason = entry.stage.localizedReason ?? ""
            syncError = reason.isEmpty ? "Invite failed" : reason
        default:
            break
        }
    }
}
