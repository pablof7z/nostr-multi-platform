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
                // Best-effort auto-dismiss once the engine has settled the
                // publish on at least one relay (status flipped to `"ok"`).
                // T128: pre-T128 we dismissed on `accepted_locally`, which
                // raced the engine's terminal verdict — a publish that
                // ended up failing all relays would be dismissed before the
                // `failed` state ever rendered, hiding the retry CTA from
                // the user. We now wait for `ok` to dismiss, and stay
                // open on `failed` so Retry is reachable. `accepted_locally`
                // by itself no longer triggers dismiss (it's transient).
                guard didSubmit else { return }
                if queue.last?.status == "ok" {
                    DispatchQueue.main.asyncAfter(deadline: .now() + 0.8) {
                        dismiss()
                    }
                }
            }
        }
    }

    /// T128: render terminal status with per-relay detail.
    /// - `accepted_locally`: spinner + "Publishing…"
    /// - `ok` full: "Published to N relay(s)"
    /// - `ok` partial: "Published to N of M relays"
    /// - `failed`: error label + Retry button (re-publishes the same draft)
    @ViewBuilder
    private var publishStatus: some View {
        let latest = model.publishQueue.last
        switch latest?.status {
        case "ok":
            publishStatusOk(latest!)
        case "failed":
            publishStatusFailed(latest!)
        default:
            // `accepted_locally`, missing entry, or any older / forward-compat
            // status — render the in-flight indicator.
            HStack(spacing: 8) {
                ProgressView()
                Text("Publishing…")
            }
            .font(.caption)
            .foregroundStyle(.secondary)
        }
    }

    private func publishStatusOk(_ entry: PublishQueueEntry) -> some View {
        let accepted = entry.acceptedRelayCount
        let target = entry.targetRelays
        // Partial success: at least one relay refused. The kernel still
        // reports `"ok"` (publish landed somewhere) but the per-relay map
        // tells us not every relay accepted.
        let isPartial = accepted < target
        let label: String = {
            if target == 0 { return "Published" }
            if isPartial {
                return "Published to \(accepted) of \(target) relays"
            }
            return "Published to \(target) relay\(target == 1 ? "" : "s")"
        }()
        return HStack(spacing: 8) {
            Image(systemName: "checkmark.circle.fill")
                .foregroundStyle(isPartial ? .orange : .green)
            Text(label)
        }
        .font(.caption)
        .foregroundStyle(.secondary)
    }

    private func publishStatusFailed(_ entry: PublishQueueEntry) -> some View {
        // Every relay reached FailedAfterRetries. Surface the first reason
        // for context, but keep the row compact — the toast (D6) carries
        // any kernel-wide error already.
        let firstReason = entry.outcomes
            .first(where: { $0.status == "failed" })?
            .reason
        return HStack(spacing: 8) {
            Image(systemName: "exclamationmark.triangle.fill")
                .foregroundStyle(.red)
            VStack(alignment: .leading, spacing: 2) {
                Text("Publish failed on all \(entry.targetRelays) relay\(entry.targetRelays == 1 ? "" : "s")")
                if let firstReason, !firstReason.isEmpty {
                    Text(firstReason)
                        .font(.caption2)
                        .foregroundStyle(.tertiary)
                        .lineLimit(2)
                }
            }
            Spacer()
            Button("Retry") {
                let trimmed = draft.trimmingCharacters(in: .whitespacesAndNewlines)
                guard !trimmed.isEmpty else { return }
                model.publishNote(trimmed, replyToID: replyToID)
            }
            .buttonStyle(.bordered)
            .controlSize(.small)
        }
        .font(.caption)
        .foregroundStyle(.secondary)
    }

    private func short(_ id: String) -> String {
        id.count > 12 ? "\(id.prefix(8))…\(id.suffix(4))" : id
    }
}
