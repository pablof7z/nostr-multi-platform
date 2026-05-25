import SwiftUI

/// Sidebar list of registry sections.
struct SectionListView: View {
    @Binding var selection: RegistrySection?
    @Environment(\.horizontalSizeClass) private var sizeClass

    var body: some View {
        List(selection: $selection) {
            ForEach(REGISTRY_SECTIONS) { section in
                if sizeClass == .regular {
                    NavigationLink(value: section) {
                        sectionRow(section)
                    }
                } else {
                    NavigationLink(value: section) {
                        sectionRow(section)
                    }
                }
            }
        }
        .listStyle(.sidebar)
    }

    private func sectionRow(_ section: RegistrySection) -> some View {
        HStack {
            Text(section.label)
                .font(.body.weight(.medium))
            Spacer()
            Text("\(section.components.count)")
                .font(.caption.monospaced())
                .foregroundStyle(.secondary)
        }
    }
}
