import SwiftUI

// Per-row UI for `NotificationsView`. Lifted into a sibling file so the
// parent screen stays focused on the summary + section composition. All
// display strings (`statusLabel`, `targetSummary`, `attemptLabel`) and the
// retry-enablement flag (`canRetry`) come pre-formatted from Rust
// (`projections["publish_outbox"]`); these structs only choose colors and
// icons — presentation, not policy (RMP bible commandment #4).

struct OutboxEventRow: View {
    let item: PublishOutboxItem
    let copied: Bool
    let retry: () -> Void
    let cancel: () -> Void
    let copy: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(alignment: .top, spacing: 12) {
                Image(systemName: iconName)
                    .font(.system(size: 17, weight: .semibold))
                    .foregroundStyle(iconColor)
                    .frame(width: 22)

                VStack(alignment: .leading, spacing: 4) {
                    HStack(spacing: 8) {
                        Text(item.title)
                            .font(.headline)
                        Text("kind \(item.kind)")
                            .font(.caption.weight(.medium))
                            .foregroundStyle(.secondary)
                    }
                    Text(item.targetSummary)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }

                Spacer(minLength: 0)
                OutboxStatusBadge(label: item.statusLabel, status: item.status)
            }

            Text(item.preview)
                .font(.callout)
                .foregroundStyle(.primary)
                .fixedSize(horizontal: false, vertical: true)

            VStack(alignment: .leading, spacing: 6) {
                ForEach(item.relays) { relay in
                    OutboxRelayRow(relay: relay)
                }
            }

            HStack(spacing: 8) {
                Button(action: retry) {
                    Label("Retry", systemImage: "arrow.clockwise")
                }
                .buttonStyle(.bordered)
                .disabled(!item.canRetry)
                .accessibilityIdentifier("publish-outbox-retry")

                Button(role: .destructive, action: cancel) {
                    Label("Cancel", systemImage: "xmark")
                }
                .buttonStyle(.bordered)
                .accessibilityIdentifier("publish-outbox-cancel")

                Spacer(minLength: 0)

                Button(action: copy) {
                    Label(copied ? "Copied" : "Copy ID", systemImage: copied ? "checkmark.circle.fill" : "doc.on.doc")
                        .labelStyle(.titleAndIcon)
                }
                .buttonStyle(.borderless)
                .foregroundStyle(copied ? Color.green : Color.accentColor)
                .accessibilityLabel(copied ? "Copied event ID" : "Copy event ID")
            }
            .font(.callout.weight(.semibold))
        }
        .padding(.vertical, 4)
        .accessibilityIdentifier("publish-outbox-card")
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
}

struct OutboxRelayRow: View {
    let relay: PublishOutboxRelay

    var body: some View {
        HStack(spacing: 8) {
            Circle()
                .fill(statusColor)
                .frame(width: 8, height: 8)
            Text(relay.relayUrl)
                .font(.caption.monospaced())
                .foregroundStyle(.primary)
                .lineLimit(1)
                .truncationMode(.middle)
            Spacer(minLength: 0)
            // `attemptLabel` is "" when attempt == 0 — no `if attempt > 0`.
            Text(relay.attemptLabel)
                .font(.caption2.weight(.medium))
                .foregroundStyle(.secondary)
            Text(relay.statusLabel)
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

struct OutboxStatusBadge: View {
    /// Pre-formatted label from `publish_outbox[].status_label`.
    let label: String
    /// Raw status key — color selection only.
    let status: String

    var body: some View {
        Text(label)
            .font(.caption2.weight(.bold))
            .foregroundStyle(color)
            .padding(.horizontal, 9)
            .padding(.vertical, 5)
            .background(color.opacity(0.12), in: Capsule())
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
