import SwiftUI

/// Newest-first feed of kind:1 notes fed by the raw event observer
/// (`nmp_app_register_raw_event_observer` with `kinds_json = "[1]"`).
struct TimelineView: View {
    let bridge: NotesBridge
    var body: some View {
        NavigationStack {
            List(bridge.notes) { NoteRow(note: $0) }
                .listStyle(.plain)
                .overlay {
                    if bridge.notes.isEmpty {
                        ContentUnavailableView("Waiting for notes",
                            systemImage: "antenna.radiowaves.left.and.right",
                            description: Text("Once relays deliver kind:1 events they appear here."))
                    }
                }
                .navigationTitle("Timeline")
        }
    }
}

private struct NoteRow: View {
    let note: NoteModel
    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                Text(short(note.pubkey)).font(.caption.monospaced()).foregroundStyle(.secondary)
                Spacer()
                Text(note.createdAt, style: .relative).font(.caption2).foregroundStyle(.secondary)
            }
            Text(note.content).font(.body).lineLimit(8)
        }
        .padding(.vertical, 4)
    }
    private func short(_ h: String) -> String {
        h.count > 12 ? "\(h.prefix(8))…\(h.suffix(4))" : h
    }
}
