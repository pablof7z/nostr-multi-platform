import SwiftUI

/// Root container view. One stack drives section -> component -> detail on
/// every device; size class changes only affect how much space the same views
/// have to render.
struct GalleryNavigation: View {
    @State private var navPath = NavigationPath()

    var body: some View {
        NavigationStack(path: $navPath) {
            SectionListView()
                .navigationTitle("NMP Gallery")
                .navigationDestination(for: RegistrySection.self) { section in
                    ComponentListView(section: section)
                        .navigationTitle(section.label)
                }
                .navigationDestination(for: RegistryComponent.self) { component in
                    ComponentDetailView(component: component)
                        .navigationTitle(component.label)
                }
        }
    }
}
