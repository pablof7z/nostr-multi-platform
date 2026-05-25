import SwiftUI

/// Sidebar list of registry sections.
struct SectionListView: View {
    @Binding var selection: RegistrySection?
    @Environment(\.horizontalSizeClass) private var sizeClass

    var body: some View {
        if sizeClass == .regular {
            // iPad split view: List(selection:) drives the middle column
            List(selection: $selection) {
                ForEach(REGISTRY_SECTIONS) { section in
                    NavigationLink(value: section) {
                        sectionRow(section)
                    }
                }
            }
            .listStyle(.sidebar)
        } else {
            // iPhone stack: NavigationLink(value:) pushes to GalleryNavigation's
            // .navigationDestination(for: RegistrySection.self) handler
            List {
                ForEach(REGISTRY_SECTIONS) { section in
                    NavigationLink(value: section) {
                        sectionRow(section)
                    }
                }
            }
            .listStyle(.insetGrouped)
        }
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
