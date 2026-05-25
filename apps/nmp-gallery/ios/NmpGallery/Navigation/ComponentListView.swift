import SwiftUI

/// Middle column / pushed list: components inside one section.
struct ComponentListView: View {
    let section: RegistrySection
    @Binding var selection: RegistryComponent?

    var body: some View {
        List(section.components, selection: $selection) { component in
            NavigationLink(value: component) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(component.label)
                        .font(.headline)
                    Text(component.description)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(2)
                }
                .padding(.vertical, 2)
            }
        }
        .listStyle(.plain)
    }
}
