import SwiftUI

// MARK: - OutboxSummary display helpers

// ADR-0032 / aim.md §2 #4: `title` and `subtitle` removed from the wire.
// The shell computes them from the raw counters.
extension OutboxSummary {
    /// Primary headline for the outbox summary section.
    var displayTitle: String {
        if total == 0 { return "Nothing waiting" }
        return total == 1 ? "1 pending publish" : "\(total) pending publishes"
    }

    /// Secondary subtitle describing the current breakdown.
    var displaySubtitle: String {
        if total == 0 { return "Your local outbox is clear." }
        var parts: [String] = []
        if sending > 0 { parts.append("\(sending) currently sending") }
        if retrying > 0 { parts.append("\(retrying) retrying") }
        if queued > 0 { parts.append("\(queued) queued") }
        if failed > 0 { parts.append("\(failed) failed") }
        return parts.joined(separator: ", ") + "."
    }
}

/// Publish-outbox screen. Thin shell: Rust owns publish status, retry policy,
/// and raw counters under `projections["outbox_summary"]` and
/// `projections["publish_outbox"]`. Display strings are computed here.
/// The per-row UI lives in `NotificationsView+OutboxRow.swift` (color/SF-Symbol
/// selection is presentation only).
struct NotificationsView: View {
    @EnvironmentObject private var model: KernelModel
    @Environment(\.dismiss) private var dismiss
    @State private var copiedHandle: String?

    var body: some View {
        List {
            Section { summarySection }

            if model.publishOutbox.isEmpty {
                Section { emptyStateSection }
            } else {
                Section("Pending publishes") {
                    ForEach(model.publishOutbox) { item in
                        OutboxEventRow(
                            item: item,
                            copied: copiedHandle == item.handle,
                            retry: { model.retryPublish(handle: item.handle) },
                            cancel: { model.cancelPublish(correlationID: item.handle) },
                            copy: { copyEventID(item.eventId, handle: item.handle) }
                        )
                    }
                }
            }
        }
        .listStyle(.insetGrouped)
        .scrollContentBackground(.hidden)
        .chirpScreenBackground()
        .navigationTitle("Outbox")
        .navigationBarTitleDisplayMode(.large)
        .accessibilityIdentifier("publish-outbox-list")
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                Button("Done") { dismiss() }
            }
        }
    }

    private var summarySection: some View {
        HStack(alignment: .center, spacing: 14) {
            Image(systemName: "paperplane.fill")
                .font(.system(size: 20, weight: .semibold))
                .foregroundStyle(ChirpColor.accent)
                .frame(width: 30)

            VStack(alignment: .leading, spacing: 4) {
                Text(model.outboxSummary.displayTitle)
                    .font(.headline)
                Text(model.outboxSummary.displaySubtitle)
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .padding(.vertical, 4)
    }

    private var emptyStateSection: some View {
        VStack(spacing: 12) {
            Image(systemName: "checkmark.seal")
                .font(.system(size: 34, weight: .light))
                .symbolRenderingMode(.hierarchical)
                .foregroundStyle(.secondary)
            Text("All published")
                .font(.headline)
            Text("No relay acknowledgements are outstanding.")
                .font(.callout)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
                .fixedSize(horizontal: false, vertical: true)
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 20)
    }

    private func copyEventID(_ eventID: String, handle: String) {
        UIPasteboard.general.string = eventID
        UIImpactFeedbackGenerator(style: .light).impactOccurred()
        withAnimation(.smooth(duration: 0.2)) { copiedHandle = handle }
        Task {
            try? await Task.sleep(for: .seconds(1.8))
            await MainActor.run {
                withAnimation(.smooth(duration: 0.25)) { copiedHandle = nil }
            }
        }
    }
}
