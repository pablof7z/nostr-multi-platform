import SwiftUI

/// Screen 4 — author a kind:1, optionally as a reply.
///
/// Dispatches `nmp_app_publish_note(content, reply_to_id_or_null)`. The
/// kernel resolves write-relays via `Nip65OutboxResolver` (D3) — there is no
/// relay picker. The publish-queue status (`model.publishQueue`) and any
/// failure toast (`model.lastErrorToast`, D6) are read straight off the
/// snapshot; no Swift-side state.
struct ComposeView: View {
    @EnvironmentObject private var model: KernelModel
    @Environment(\.dismiss) private var dismiss

    /// When non-nil, this compose is a reply to the given event id.
    let replyToID: String?

    @State private var draft = ""
    @State private var didSubmit = false

    init(replyToID: String? = nil) {
        self.replyToID = replyToID
    }

    var body: some View {
        NavigationStack {
            VStack(alignment: .leading, spacing: 16) {
                if let replyToID {
                    Label("Replying to \(short(replyToID))", systemImage: "arrowshape.turn.up.left")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }

                TextEditor(text: $draft)
                    .frame(minHeight: 160)
                    .overlay(alignment: .topLeading) {
                        if draft.isEmpty {
                            Text("What's happening?")
                                .foregroundStyle(.tertiary)
                                .padding(.top, 8)
                                .padding(.leading, 5)
                                .allowsHitTesting(false)
                        }
                    }
                    .overlay(
                        RoundedRectangle(cornerRadius: 8)
                            .stroke(.quaternary)
                    )

                if didSubmit, !model.publishQueue.isEmpty {
                    publishStatus
                }

                if let toast = model.lastErrorToast {
                    Text(toast)
                        .font(.caption)
                        .foregroundStyle(.red)
                }

                Spacer()
            }
            .padding(20)
            .navigationTitle(replyToID == nil ? "Compose" : "Reply")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Send") {
                        let trimmed = draft.trimmingCharacters(in: .whitespacesAndNewlines)
                        guard !trimmed.isEmpty else { return }
                        model.publishNote(trimmed, replyToID: replyToID)
                        didSubmit = true
                    }
                    .disabled(draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                }
            }
            .onChange(of: model.publishQueue) { _, queue in
                // Best-effort auto-dismiss once the kernel has accepted the
                // publish locally (D1: render now, refine in place).
                if didSubmit, queue.contains(where: { $0.status == "accepted_locally" }) {
                    DispatchQueue.main.asyncAfter(deadline: .now() + 0.8) {
                        dismiss()
                    }
                }
            }
        }
    }

    private var publishStatus: some View {
        let latest = model.publishQueue.last
        return HStack(spacing: 8) {
            if latest?.status == "accepted_locally" {
                Image(systemName: "checkmark.circle.fill").foregroundStyle(.green)
                Text("Sent to \(latest?.targetRelays ?? 0) relay(s)")
            } else {
                ProgressView()
                Text("Publishing…")
            }
        }
        .font(.caption)
        .foregroundStyle(.secondary)
    }

    private func short(_ id: String) -> String {
        id.count > 12 ? "\(id.prefix(8))…\(id.suffix(4))" : id
    }
}
