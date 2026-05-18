import SwiftUI

/// Scrollable, labeled gallery — one cell per scenario. Each cell shows
/// the title, a collapsible raw-event JSON disclosure, and the
/// NMP-rendered output produced by the real `nmp-content` / `nmp-nip23`
/// path (pre-tokenized in STAGE 2, walked here per `SegmentDto`).
struct GalleryView: View {
    let bundle: GalleryBundle

    private var categories: [String] {
        var seen: [String] = []
        for s in bundle.scenarios where !seen.contains(s.category) {
            seen.append(s.category)
        }
        return seen
    }

    var body: some View {
        NavigationStack {
            List {
                Section {
                    Text("\(bundle.scenarios.count) scenarios "
                        + "· bundle v\(bundle.version)")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                }
                ForEach(categories, id: \.self) { category in
                    Section(category.capitalized) {
                        ForEach(scenarios(in: category)) { scenario in
                            ScenarioCell(scenario: scenario)
                        }
                    }
                }
            }
            .listStyle(.insetGrouped)
            .navigationTitle("NMP Content Gallery")
            .navigationBarTitleDisplayMode(.inline)
        }
    }

    private func scenarios(in category: String) -> [Scenario] {
        bundle.scenarios.filter { $0.category == category }
    }
}

/// Routes a scenario to its category renderer + a JSON disclosure.
struct ScenarioCell: View {
    let scenario: Scenario
    @State private var showJSON = false

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text(scenario.id)
                    .font(.caption.monospaced())
                    .padding(.horizontal, 6)
                    .padding(.vertical, 2)
                    .background(Color.accentColor.opacity(0.15))
                    .clipShape(Capsule())
                Text(scenario.title)
                    .font(.subheadline.bold())
            }
            Text(scenario.exercises)
                .font(.caption2)
                .foregroundStyle(.secondary)

            ScenarioRenderer(scenario: scenario)
                .padding(10)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(Color(.secondarySystemBackground))
                .clipShape(RoundedRectangle(cornerRadius: 10))

            DisclosureGroup("Event JSON", isExpanded: $showJSON) {
                ForEach(Array(scenario.events.enumerated()),
                        id: \.offset) { _, ev in
                    Text(eventSummary(ev))
                        .font(.caption2.monospaced())
                        .textSelection(.enabled)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
            }
            .font(.caption)
        }
        .padding(.vertical, 4)
    }

    private func eventSummary(_ ev: SignedEventJson) -> String {
        """
        kind \(ev.kind) · id \(ev.id.prefix(12))…
        pubkey \(ev.pubkey.prefix(12))… · sig \(ev.sig.prefix(12))…
        content: \(ev.content.prefix(120))
        """
    }
}
