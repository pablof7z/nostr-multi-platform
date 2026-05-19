import SwiftUI
import os.log

// MARK: - DataExportView
//
// Settings → Data → Export. Generates a JSON document of the live `AppState`
// (items, notes, friends, agent memories, agent activity, non-secret settings)
// and surfaces it through a system share sheet so the user can save it to
// Files, AirDrop it, or send it through any share extension.
//
// Inspired by cut-tracker's `ExportCSVSheet` (sheet shape + share) and
// win-the-day-app's `FullBackupManager` (versioned JSON envelope).
//
// Secrets are never exported — see `DataExport.redactedState(from:)`.

struct DataExportView: View {
    private static let logger = Logger.app("DataExportView")
    /// Cached time formatter — `DateFormatter` is expensive to allocate and thread-safe for reads after setup.
    private static let exportTimeFormatter: DateFormatter = {
        let f = DateFormatter()
        f.dateStyle = .none
        f.timeStyle = .medium
        return f
    }()

    @Environment(AppStateStore.self) private var store

    @State private var fileURL: URL?
    @State private var fileSize: Int?
    @State private var errorMessage: String?
    @State private var generatedAt: Date?

    var body: some View {
        ZStack {
            Color(.systemGroupedBackground)
                .ignoresSafeArea()

            List {
                summarySection
                actionSection
                aboutSection
            }
            .listStyle(.insetGrouped)
            .scrollContentBackground(.hidden)
        }
        .navigationTitle("Export Data")
        .navigationBarTitleDisplayMode(.inline)
    }

    // MARK: - Sections

    private var summarySection: some View {
        Section("Contents") {
            statRow(icon: "antenna.radiowaves.left.and.right", tint: .pink, label: "Subscriptions", count: stats.subscriptions)
            statRow(icon: "headphones", tint: .blue, label: "Episodes", count: stats.episodes)
            statRow(icon: "note.text", tint: .indigo, label: "Notes", count: stats.notes)
            statRow(icon: "person.2.fill", tint: .green, label: "Friends", count: stats.friends)
            statRow(icon: "brain.head.profile", tint: .orange, label: "Memories", count: stats.memories)
            statRow(icon: "clock.arrow.circlepath", tint: .gray, label: "Agent activity", count: stats.agentActivity)
        }
    }

    @ViewBuilder
    private var actionSection: some View {
        if let errorMessage {
            errorActionSection(message: errorMessage)
        } else {
            exportActionSection
        }
    }

    private func errorActionSection(message: String) -> some View {
        Section {
            Text(message)
                .foregroundStyle(AppTheme.Tint.error)
            Button("Try again") { generate() }
        }
    }

    private var exportActionSection: some View {
        Section {
            Button {
                generate()
            } label: {
                SettingsRow(
                    icon: "square.and.arrow.up",
                    tint: .indigo,
                    title: "Export & Share",
                    subtitle: "Generates a JSON file and opens the share sheet"
                )
            }
            .foregroundStyle(.primary)
        } footer: {
            Text(actionFooterText)
        }
    }

    private var aboutSection: some View {
        Section("About") {
            SettingsRow(
                icon: "doc.text",
                tint: .gray,
                title: "Format",
                value: "JSON"
            )
            SettingsRow(
                icon: "number",
                tint: .gray,
                title: "Schema",
                value: "v\(DataExport.currentSchemaVersion)"
            )
        }
    }

    // MARK: - Subviews

    private func statRow(icon: String, tint: Color, label: String, count: Int) -> some View {
        SettingsRow(
            icon: icon,
            tint: tint,
            title: label,
            value: "\(count)"
        )
    }

    // MARK: - Derived

    private var stats: DataExport.Stats {
        DataExport.stats(for: store.state)
    }

    private var actionFooterText: String {
        let records = stats.totalRecords
        let base = "\(records) record\(records == 1 ? "" : "s")"
        if let size = fileSize, let generatedAt {
            return "\(base) · \(formatBytes(size)) · Last exported \(Self.exportTimeFormatter.string(from: generatedAt))"
        }
        return "\(base) · Bundles subscriptions, episodes, notes, friends, agent memories, and agent activity. API keys and the Nostr private key are never included."
    }

    // MARK: - Actions

    private func generate() {
        do {
            let now = Date()
            let exportState = store.state
            let url = try DataExport.writeExport(of: exportState, now: now)
            let attrs: [FileAttributeKey: Any]?
            do {
                attrs = try FileManager.default.attributesOfItem(atPath: url.path)
            } catch {
                Self.logger.warning("DataExportView: could not read file attributes for \(url.lastPathComponent, privacy: .public): \(error, privacy: .public)")
                attrs = nil
            }
            fileURL = url
            fileSize = (attrs?[.size] as? NSNumber)?.intValue
            generatedAt = now
            errorMessage = nil
            Haptics.success()
            // Imperative present rather than `.sheet { ShareSheet(items:) }`
            // — the SwiftUI sheet wrapper renders blank for file-URL items
            // on iOS 16+ (`UIActivityViewController` needs its own
            // presentation context, not a SwiftUI modal scope).
            SystemShareSheet.present(items: [url])
        } catch {
            errorMessage = "Could not generate export: \(error.localizedDescription)"
            fileURL = nil
            fileSize = nil
            Haptics.error()
        }
    }

    private func formatBytes(_ n: Int) -> String {
        let kb = 1_024
        let mb = 1_048_576
        if n >= mb { return String(format: "%.1f MB", Double(n) / Double(mb)) }
        if n >= kb { return String(format: "%.1f KB", Double(n) / Double(kb)) }
        return "\(n) B"
    }
}
