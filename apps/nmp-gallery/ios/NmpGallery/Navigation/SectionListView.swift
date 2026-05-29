import SwiftUI

/// Sidebar list of registry sections.
struct SectionListView: View {
    var body: some View {
        List {
            ForEach(REGISTRY_SECTIONS) { section in
                NavigationLink(value: section) {
                    HStack(spacing: 12) {
                        Image(systemName: symbolName(for: section.id))
                            .foregroundStyle(.tint)
                            .frame(width: 24)
                        Text(section.label)
                        Spacer()
                        Text("\(section.components.count)")
                            .font(.caption.monospaced())
                            .foregroundStyle(.secondary)
                    }
                }
            }
        }
        .listStyle(.insetGrouped)
    }

    private func symbolName(for sectionId: String) -> String {
        switch sectionId {
        case "relay":
            return "antenna.radiowaves.left.and.right"
        case "user":
            return "person.crop.circle"
        case "content":
            return "text.bubble.fill"
        case "embeds":
            return "link.badge.plus"
        default:
            return "square.grid.2x2"
        }
    }
}
