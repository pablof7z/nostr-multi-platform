import SwiftUI

/// Root container view. Picks the three-column `NavigationSplitView` layout
/// on iPad / regular size class and falls back to a stacked
/// `NavigationStack` on compact iPhone widths.
struct GalleryNavigation: View {
    @Environment(\.horizontalSizeClass) private var sizeClass
    @State private var selectedSection: RegistrySection? = REGISTRY_SECTIONS.first
    @State private var selectedComponent: RegistryComponent?

    var body: some View {
        if sizeClass == .regular {
            NavigationSplitView {
                SectionListView(selection: $selectedSection)
                    .navigationTitle("NMP Gallery")
            } content: {
                if let section = selectedSection {
                    ComponentListView(
                        section: section,
                        selection: $selectedComponent
                    )
                    .navigationTitle(section.label)
                } else {
                    Text("Pick a section")
                        .foregroundStyle(.secondary)
                }
            } detail: {
                if let component = selectedComponent {
                    ComponentDetailView(component: component)
                        .navigationTitle(component.label)
                } else {
                    Text("Pick a component")
                        .foregroundStyle(.secondary)
                }
            }
        } else {
            NavigationStack {
                SectionListView(selection: $selectedSection)
                    .navigationTitle("NMP Gallery")
                    .navigationDestination(for: RegistrySection.self) { section in
                        ComponentListView(
                            section: section,
                            selection: $selectedComponent
                        )
                        .navigationTitle(section.label)
                        .navigationDestination(for: RegistryComponent.self) { component in
                            ComponentDetailView(component: component)
                                .navigationTitle(component.label)
                        }
                    }
            }
        }
    }
}
