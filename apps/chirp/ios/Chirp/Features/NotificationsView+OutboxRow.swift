import SwiftUI

// Per-row UI for `NotificationsView`. Lifted into a sibling file so the
// parent screen stays focused on the summary + section composition.
//
// ADR-0032 / aim.md §2 #4: `title`, `preview`, `statusLabel`, `systemImage`
// are no longer carried on the wire projection. This file owns the shell-side
// presentation helpers (`kindTitle`, `iconName`, `preview`, `statusLabel`).
// The kernel still owns policy: `canRetry`, raw `status` token, `attempt`
// count. Presentation chooses colors (aim.md §2 #4: no kind-number switches
// drive _policy_ — only icon/label presentation here).
//
// ADR-0032 / V-115: `targetSummary` and `createdAtDisplay` were removed from
// the Rust projection. The target relay count (`targetRelays`) and raw unix
// seconds (`createdAt`) are provided instead; this view computes the display
// string locally.

// MARK: - Shell-side presentation helpers

extension PublishOutboxItem {
    /// Human-readable kind label. Shell-side equivalent of the removed
    /// `publish_event_title()` Rust function.
    var kindTitle: String {
        switch kind {
        case 0: return "Profile"
        case 1: return "Note"
        case 6: return "Repost"
        case 7: return "Reaction"
        case 9735: return "Zap"
        case 30023: return "Article"
        default: return "Event"
        }
    }

    /// SF Symbol name for the event kind. Shell-side equivalent of the removed
    /// `publish_event_system_image()` Rust function.
    var iconName: String {
        switch kind {
        case 0: return "person.crop.circle"
        case 1: return "text.bubble"
        case 6: return "arrow.2.squarepath"
        case 7: return "heart"
        case 9735: return "bolt"
        case 30023: return "doc.text"
        default: return "doc.text"
        }
    }

    /// Preview string derived from raw content. Shell-side equivalent of the
    /// removed `publish_event_preview()` Rust function.
    var previewText: String {
        content.isEmpty ? "(no content)" : content
    }

    /// Human-readable status label. Shell-side equivalent of the removed
    /// `publish_outbox_status_label()` Rust function.
    var statusLabel: String {
        switch status {
        case "sending": return "Sending"
        case "retrying": return "Retrying"
        case "queued": return "Queued"
        case "failed": return "Failed"
        default: return status.capitalized
        }
    }
}

extension PublishOutboxRelay {
    /// Human-readable status label for this relay row. Shell-side equivalent
    /// of the removed `publish_outbox_relay_status_label()` Rust function.
    var statusLabel: String {
        switch status {
        case "sending": return "Sending"
        case "ok": return "OK"
        case "retrying": return "Retrying"
        case "pending": return "Pending"
        case "failed": return "Failed"
        default: return status.capitalized
        }
    }

    /// "try N" badge text — empty when `attempt == 0`. Shell-side equivalent
    /// of the removed `publish_outbox_attempt_label()` Rust function.
    var attemptLabel: String {
        attempt == 0 ? "" : "try \(attempt)"
    }

    /// Human-readable relay-reason label derived from the raw `relayReason`
    /// token. Parameterised tokens are parsed; unknown tokens pass through.
    var relayReasonDisplay: String {
        let token = relayReason
        if token.isEmpty { return "" }
        let discoveryPrefix = "discovery_indexer:"
        if token.hasPrefix(discoveryPrefix) {
            let kind = token.dropFirst(discoveryPrefix.count)
            return "Discovery indexer (kind \(kind))"
        }
        let inboxPrefix = "recipient_inbox:"
        if token.hasPrefix(inboxPrefix) {
            let pubkey = token.dropFirst(inboxPrefix.count)
            return "Inbox relay for \(pubkey)"
        }
        switch token {
        case "nip65_write": return "NIP-65 write relay"
        case "local_config": return "App relay (local config)"
        case "explicit": return "Explicit relay"
        default: return token
        }
    }
}

// MARK: - Views

struct OutboxEventRow: View {
    let item: PublishOutboxItem
    let copied: Bool
    let retry: () -> Void
    let cancel: () -> Void
    let copy: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(alignment: .top, spacing: 12) {
                Image(systemName: item.iconName)
                    .font(.system(size: 17, weight: .semibold))
                    .foregroundStyle(iconColor)
                    .frame(width: 22)

                VStack(alignment: .leading, spacing: 4) {
                    Text(item.kindTitle)
                        .font(.headline)
                    // ADR-0032 / V-115: compute locally from raw relay count + unix seconds.
                    Text(targetSummary)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }

                Spacer(minLength: 0)
                OutboxStatusBadge(label: item.statusLabel, status: item.status)
            }

            Text(item.previewText)
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
                .foregroundStyle(copied ? ChirpColor.success : ChirpColor.accent)
                .accessibilityLabel(copied ? "Copied event ID" : "Copy event ID")
            }
            .font(.callout.weight(.semibold))
        }
        .padding(.vertical, 4)
        .accessibilityIdentifier("publish-outbox-card")
    }

    /// "N relay(s) · <relative time>" — formatted from raw projection fields.
    private var targetSummary: String {
        let relayCount = Int(item.targetRelays)
        let relayWord = relayCount == 1 ? "relay" : "relays"
        let timeString = item.createdAt.relativeTimeFromUnixSeconds
        return "\(relayCount) \(relayWord) · \(timeString)"
    }

    private var iconColor: Color {
        switch item.status {
        case "retrying": return ChirpColor.warning
        case "failed": return ChirpColor.danger
        default: return ChirpColor.accent
        }
    }
}

struct OutboxRelayRow: View {
    let relay: PublishOutboxRelay

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
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
            if !relay.relayReasonDisplay.isEmpty {
                Text(relay.relayReasonDisplay)
                    .font(.caption2)
                    .foregroundStyle(.secondary)
                    .padding(.leading, 16)
            }
        }
        .accessibilityElement(children: .combine)
    }

    private var statusColor: Color {
        switch relay.status {
        case "sending", "ok": return ChirpColor.success
        case "retrying", "pending": return ChirpColor.warning
        case "failed": return ChirpColor.danger
        default: return ChirpColor.textSecondary
        }
    }
}

struct OutboxStatusBadge: View {
    /// Shell-computed status label.
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
        case "sending": return ChirpColor.success
        case "retrying", "pending": return ChirpColor.warning
        case "failed": return ChirpColor.danger
        default: return ChirpColor.textSecondary
        }
    }
}
