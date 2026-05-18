import SwiftUI

/// Lets the user import another nostr account's relay list. Takes an npub
/// (or hex pubkey), fetches that user's kind:10002 via the Indexer pool,
/// and shows the discovered relays with checkboxes. Merging is opt-in —
/// only rows the user ticks get upserted.
struct ImportRelaysSheet: View {
    let store: NetworkSettingsStore

    @Environment(HighlighterStore.self) private var appStore
    @Environment(\.dismiss) private var dismiss

    @State private var npubText: String = ""
    @State private var fetched: [RelayConfig] = []
    @State private var selected: Set<String> = []
    @State private var isFetching = false
    @State private var errorText: String?
    @State private var isApplying = false

    var body: some View {
        NavigationStack {
            Form {
                npubSection
                if !fetched.isEmpty {
                    foundSection
                }
                if let err = errorText {
                    errorSection(err)
                }
            }
            .navigationTitle("Import from npub")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button("Cancel") { dismiss() }
                }
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Add \(selected.count)") {
                        Task { await applySelected() }
                    }
                    .disabled(selected.isEmpty || isApplying)
                }
            }
        }
    }

    // MARK: - Sections

    private var npubSection: some View {
        Section {
            TextField("npub1… or hex pubkey", text: $npubText)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
                .monospaced()
            Button {
                Task { await fetch() }
            } label: {
                if isFetching {
                    HStack {
                        ProgressView().scaleEffect(0.7)
                        Text("Fetching…")
                    }
                } else {
                    Label("Fetch relays", systemImage: "arrow.down.circle")
                }
            }
            .disabled(npubText.trimmingCharacters(in: .whitespaces).isEmpty || isFetching)
        } header: {
            Text("Source")
        } footer: {
            Text("Highlighter will fetch the user's kind:10002 event through your Indexer relays. Turn on Indexer for at least one relay first.")
        }
    }

    private var foundSection: some View {
        Section {
            ForEach(fetched, id: \.url) { row in
                Button {
                    toggle(row.url)
                } label: {
                    HStack {
                        Image(systemName: selected.contains(row.url) ? "checkmark.circle.fill" : "circle")
                            .foregroundStyle(selected.contains(row.url) ? Color.accentColor : .secondary)
                        VStack(alignment: .leading, spacing: 2) {
                            Text(displayURL(row.url))
                                .font(.subheadline)
                                .lineLimit(1)
                                .truncationMode(.middle)
                            Text(roleLabel(row))
                                .font(.caption2)
                                .foregroundStyle(.secondary)
                        }
                        Spacer()
                    }
                }
                .buttonStyle(.plain)
            }
        } header: {
            Text("Found \(fetched.count) relay\(fetched.count == 1 ? "" : "s")")
        } footer: {
            Text("Selected relays will be added or updated in your list with their original Read/Write roles. Rooms and Indexer stay off — tap a relay later to turn them on.")
        }
    }

    private func errorSection(_ err: String) -> some View {
        Section {
            Label(err, systemImage: "exclamationmark.triangle")
                .font(.caption)
                .foregroundStyle(.orange)
        }
    }

    // MARK: - Actions

    private func fetch() async {
        errorText = nil
        fetched = []
        selected = []
        isFetching = true
        defer { isFetching = false }
        do {
            let rows = try await appStore.safeCore
                .importRelaysFromNpub(npubText.trimmingCharacters(in: .whitespaces))
            fetched = rows
            selected = Set(rows.map { $0.url })
            if rows.isEmpty {
                errorText = "No kind:10002 found for this user — they may not have published a relay list yet."
            }
        } catch {
            errorText = String(describing: error)
        }
    }

    private func applySelected() async {
        isApplying = true
        defer { isApplying = false }
        for row in fetched where selected.contains(row.url) {
            await store.upsert(row)
        }
        dismiss()
    }

    private func toggle(_ url: String) {
        if selected.contains(url) {
            selected.remove(url)
        } else {
            selected.insert(url)
        }
    }

    private func displayURL(_ raw: String) -> String {
        if raw.hasPrefix("wss://") { return String(raw.dropFirst(6)) }
        return raw
    }

    private func roleLabel(_ row: RelayConfig) -> String {
        switch (row.read, row.write) {
        case (true, true): return "Read + Write"
        case (true, false): return "Read"
        case (false, true): return "Write"
        default: return "No roles"
        }
    }
}
