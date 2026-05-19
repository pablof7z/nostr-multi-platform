import SwiftUI
import UIKit

struct WhatsNewSheet: View {

    let entries: [WhatsNewEntry]
    @Environment(\.dismiss) private var dismiss

    /// Mirrors `WhatsNewService.lastSeenAtKey` so dismissal advances the
    /// marker via the same UserDefaults key the service reads on next launch.
    @AppStorage("whatsNew.lastSeenAt") private var lastSeenAtString: String = ""

    var body: some View {
        NavigationStack {
            content
                .navigationTitle("What's new")
                .navigationBarTitleDisplayMode(.large)
        }
        .presentationDetents([.medium, .large])
        .presentationDragIndicator(.visible)
        .onDisappear {
            // Swipe-dismiss path: write marker so entries don't re-surface.
            // The "Got it" button writes the same value before dismiss(), so
            // this is idempotent in that case.
            if let newest = entries.first {
                lastSeenAtString = Self.iso8601.string(from: newest.shippedAt)
            }
        }
    }

    // MARK: - Content

    private var content: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                subtitle
                ForEach(entries) { entry in
                    entrySection(entry)
                }
                gotItButton
                    .padding(.top, 8)
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 16)
        }
    }

    private var subtitle: some View {
        Text("SINCE YOU LAST OPENED HIGHLIGHTER")
            .font(.caption2.weight(.semibold))
            .tracking(0.5)
            .foregroundStyle(.secondary)
    }

    @ViewBuilder
    private func entrySection(_ entry: WhatsNewEntry) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(Self.dateline(for: entry.shippedAt))
                .font(.caption2.weight(.semibold))
                .tracking(0.5)
                .foregroundStyle(.secondary)
            VStack(alignment: .leading, spacing: 8) {
                ForEach(Array(entry.lines.enumerated()), id: \.offset) { _, line in
                    lineRow(line)
                }
            }
        }
    }

    private func lineRow(_ line: String) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Image(systemName: "sparkle")
                .font(.body)
                .foregroundStyle(.tint)
                .accessibilityHidden(true)
            Text(line)
                .font(.body)
                .fixedSize(horizontal: false, vertical: true)
        }
    }

    private var gotItButton: some View {
        HStack {
            Spacer()
            Button("Got it") {
                if let newest = entries.first {
                    lastSeenAtString = Self.iso8601.string(from: newest.shippedAt)
                }
                UINotificationFeedbackGenerator().notificationOccurred(.success)
                dismiss()
            }
            .buttonStyle(.glassProminent)
            Spacer()
        }
    }

    private static let iso8601: ISO8601DateFormatter = {
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime]
        return f
    }()

    // MARK: - Formatting

    /// "MAY 14 · 20:03" — uppercase short month, day no zero-pad, middle dot, 24h time.
    private static func dateline(for date: Date) -> String {
        let cal = Calendar.current
        let comps = cal.dateComponents([.month, .day, .hour, .minute], from: date)
        let monthSymbols = cal.shortMonthSymbols
        let monthIndex = (comps.month ?? 1) - 1
        let month = monthSymbols.indices.contains(monthIndex)
            ? monthSymbols[monthIndex].uppercased()
            : ""
        let day = comps.day ?? 0
        let hour = comps.hour ?? 0
        let minute = comps.minute ?? 0
        return String(format: "%@ %d \u{00B7} %02d:%02d", month, day, hour, minute)
    }
}
