import SwiftUI

/// Root container view. Uses `NavigationSplitView` on iPad (regular width)
/// and an explicit-path `NavigationStack` on iPhone (compact width).
struct GalleryNavigation: View {
    @Environment(\.horizontalSizeClass) private var sizeClass
    @State private var selectedSection: RegistrySection? = REGISTRY_SECTIONS.first
    @State private var selectedComponent: RegistryComponent?
    @State private var navPath = NavigationPath()

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
            NavigationStack(path: $navPath) {
                SectionListView(selection: $selectedSection)
                    .navigationTitle("NMP Gallery")
                    .navigationDestination(for: RegistrySection.self) { section in
                        CompactComponentListView(section: section, navPath: $navPath)
                            .navigationTitle(section.label)
                    }
                    .navigationDestination(for: RegistryComponent.self) { component in
                        ComponentDetailView(component: component)
                            .navigationTitle(component.label)
                    }
            }
        }
    }
}

/// iPhone-only component list that pushes `RegistryComponent` onto
/// the explicit navigation path instead of using `List(selection:)`.
private struct CompactComponentListView: View {
    let section: RegistrySection
    @Binding var navPath: NavigationPath

    var body: some View {
        List(section.components) { component in
            Button {
                navPath.append(component)
            } label: {
                VStack(alignment: .leading, spacing: 2) {
                    Text(component.label)
                        .font(.headline)
                        .foregroundStyle(.primary)
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
