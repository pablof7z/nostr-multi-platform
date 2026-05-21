import SwiftUI

/// Publish-outbox screen. Thin shell: every string the user sees and the
/// retry-button enabled flag come pre-formatted from Rust under
/// `projections["outbox_summary"]` and `projections["publish_outbox"]` —
/// doctrine §6 anti-pattern #1 / RMP bible commandment #4. The per-row UI
/// lives in `NotificationsView+OutboxRow.swift` (color/SF-Symbol selection
/// is presentation only).
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
                            cancel: { model.cancelPublish(handle: item.handle) },
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
                .foregroundStyle(.tint)
                .frame(width: 30)

            VStack(alignment: .leading, spacing: 4) {
                Text(model.outboxSummary.title)
                    .font(.headline)
                Text(model.outboxSummary.subtitle)
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
                .foregroundStyle(.green)
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
