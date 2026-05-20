import SwiftUI

struct NotificationsView: View {
    @EnvironmentObject private var model: KernelModel
    @Environment(\.dismiss) private var dismiss
    @State private var copiedHandle: String?

    var body: some View {
        ScrollView {
            VStack(spacing: ChirpSpace.l) {
                summary
                if model.publishOutbox.isEmpty {
                    emptyState
                } else {
                    LazyVStack(spacing: ChirpSpace.m) {
                        ForEach(model.publishOutbox) { item in
                            OutboxEventCard(
                                item: item,
                                copied: copiedHandle == item.handle,
                                retry: { model.retryPublish(handle: item.handle) },
                                cancel: { model.cancelPublish(handle: item.handle) },
                                copy: { copyEventID(item.eventId, handle: item.handle) }
                            )
                        }
                    }
                    .accessibilityIdentifier("publish-outbox-list")
                }
            }
            .padding(.horizontal, ChirpSpace.l)
            .padding(.vertical, ChirpSpace.l)
        }
        .chirpScreenBackground()
        .navigationTitle("Outbox")
        .navigationBarTitleDisplayMode(.large)
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                Button("Done") { dismiss() }
            }
        }
    }

    private var summary: some View {
        HStack(alignment: .center, spacing: ChirpSpace.m) {
            ZStack {
                Circle()
                    .fill(.tint.opacity(0.14))
                    .frame(width: 48, height: 48)
                Image(systemName: "paperplane.fill")
                    .font(.system(size: 20, weight: .semibold))
                    .foregroundStyle(.tint)
            }

            VStack(alignment: .leading, spacing: 3) {
                Text(summaryTitle)
                    .font(.title3.weight(.semibold))
                Text(summarySubtitle)
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
            }

            Spacer(minLength: 0)
        }
        .padding(ChirpSpace.l)
        .frame(maxWidth: .infinity, alignment: .leading)
        .chirpGlass(cornerRadius: ChirpSpace.radius)
    }

    private var emptyState: some View {
        VStack(spacing: ChirpSpace.m) {
            Image(systemName: "checkmark.seal")
                .font(.system(size: 40, weight: .light))
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
        .padding(ChirpSpace.xl)
        .frame(maxWidth: .infinity)
        .chirpGlass(cornerRadius: ChirpSpace.radius)
    }

    private var summaryTitle: String {
        let count = model.publishOutbox.count
        return count == 0 ? "Nothing waiting" : "\(count) pending publish\(count == 1 ? "" : "es")"
    }

    private var summarySubtitle: String {
        guard !model.publishOutbox.isEmpty else {
            return "Your local outbox is clear."
        }
        let sending = model.publishOutbox.filter { $0.status == "sending" }.count
        let retrying = model.publishOutbox.filter { $0.status == "retrying" }.count
        if retrying > 0 {
            return "\(retrying) waiting to retry, \(sending) currently sending."
        }
        return sending > 0 ? "\(sending) currently sending." : "Waiting for relay connections."
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

private struct OutboxEventCard: View {
    let item: PublishOutboxItem
    let copied: Bool
    let retry: () -> Void
    let cancel: () -> Void
    let copy: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: ChirpSpace.m) {
            header
            Text(item.preview)
                .font(.callout)
                .foregroundStyle(.primary)
                .fixedSize(horizontal: false, vertical: true)

            VStack(spacing: ChirpSpace.s) {
                ForEach(item.relays) { relay in
                    OutboxRelayRow(relay: relay)
                }
            }

            HStack(spacing: ChirpSpace.s) {
                Button(action: retry) {
                    Label("Retry", systemImage: "arrow.clockwise")
                }
                .buttonStyle(.bordered)
                .disabled(!canRetry)
                .accessibilityIdentifier("publish-outbox-retry")

                Button(role: .destructive, action: cancel) {
                    Label("Cancel", systemImage: "xmark")
                }
                .buttonStyle(.bordered)
                .accessibilityIdentifier("publish-outbox-cancel")

                Spacer(minLength: 0)

                Button(action: copy) {
                    Image(systemName: copied ? "checkmark.circle.fill" : "doc.on.doc")
                        .foregroundStyle(copied ? Color.green : Color.accentColor)
                }
                .buttonStyle(.borderless)
                .accessibilityLabel(copied ? "Copied event ID" : "Copy event ID")
            }
            .font(.callout.weight(.semibold))
        }
        .padding(ChirpSpace.l)
        .frame(maxWidth: .infinity, alignment: .leading)
        .chirpGlass(cornerRadius: ChirpSpace.radius, interactive: true)
        .accessibilityIdentifier("publish-outbox-card")
    }

    private var header: some View {
        HStack(alignment: .top, spacing: ChirpSpace.m) {
            ZStack {
                RoundedRectangle(cornerRadius: 10, style: .continuous)
                    .fill(iconColor.opacity(0.14))
                    .frame(width: 38, height: 38)
                Image(systemName: iconName)
                    .font(.system(size: 17, weight: .semibold))
                    .foregroundStyle(iconColor)
            }

            VStack(alignment: .leading, spacing: 3) {
                HStack(spacing: ChirpSpace.s) {
                    Text(item.title)
                        .font(.headline)
                    Text("kind \(item.kind)")
                        .font(.caption.weight(.medium))
                        .foregroundStyle(.secondary)
                }
                Text("\(item.targetRelays) relay\(item.targetRelays == 1 ? "" : "s") · \(item.createdAtDisplay)")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }

            Spacer(minLength: 0)
            OutboxStatusBadge(status: item.status)
        }
    }

    private var iconName: String {
        switch item.kind {
        case 0: return "person.crop.circle"
        case 1: return "text.bubble"
        case 3: return "person.2"
        case 7: return "heart"
        case 10002: return "antenna.radiowaves.left.and.right"
        default: return "doc.text"
        }
    }

    private var iconColor: Color {
        switch item.status {
        case "retrying": return .orange
        case "failed": return .red
        default: return .accentColor
        }
    }

    private var canRetry: Bool {
        item.status != "sending"
    }
}

private struct OutboxRelayRow: View {
    let relay: PublishOutboxRelay

    var body: some View {
        HStack(spacing: ChirpSpace.s) {
            Circle()
                .fill(statusColor)
                .frame(width: 8, height: 8)
            Text(relay.relayUrl)
                .font(.caption.monospaced())
                .foregroundStyle(.primary)
                .lineLimit(1)
                .truncationMode(.middle)
            Spacer(minLength: 0)
            if relay.attempt > 0 {
                Text("try \(relay.attempt)")
                    .font(.caption2.weight(.medium))
                    .foregroundStyle(.secondary)
            }
            Text(relay.status.capitalized)
                .font(.caption2.weight(.semibold))
                .foregroundStyle(statusColor)
        }
        .accessibilityElement(children: .combine)
    }

    private var statusColor: Color {
        switch relay.status {
        case "sending", "ok": return .green
        case "retrying", "pending": return .orange
        case "failed": return .red
        default: return .secondary
        }
    }
}

private struct OutboxStatusBadge: View {
    let status: String

    var body: some View {
        Text(label)
            .font(.caption2.weight(.bold))
            .foregroundStyle(color)
            .padding(.horizontal, 9)
            .padding(.vertical, 5)
            .background(color.opacity(0.12), in: Capsule())
    }

    private var label: String {
        switch status {
        case "sending": return "Sending"
        case "retrying": return "Retrying"
        case "pending": return "Pending"
        case "failed": return "Failed"
        default: return "Queued"
        }
    }

    private var color: Color {
        switch status {
        case "sending": return .green
        case "retrying", "pending": return .orange
        case "failed": return .red
        default: return .secondary
        }
    }
}
