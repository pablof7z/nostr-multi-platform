import SwiftUI

/// Middle column / pushed list: components inside one section.
struct ComponentListView: View {
    let section: RegistrySection

    var body: some View {
        List(section.components) { component in
            NavigationLink(value: component) {
                HStack(spacing: 12) {
                    Image(systemName: symbolName(for: component.id))
                        .foregroundStyle(.tint)
                        .frame(width: 24)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(component.label)
                            .font(.headline)
                        Text(component.description)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                            .lineLimit(2)
                    }
                }
                .padding(.vertical, 2)
            }
        }
        .listStyle(.insetGrouped)
    }

    private func symbolName(for componentId: String) -> String {
        if componentId.hasPrefix("relay-") {
            return "antenna.radiowaves.left.and.right"
        }
        if componentId.hasPrefix("user-") {
            return "person.crop.circle"
        }
        if componentId.hasPrefix("content-") {
            return "text.bubble"
        }
        return "square.grid.2x2"
    }
}
