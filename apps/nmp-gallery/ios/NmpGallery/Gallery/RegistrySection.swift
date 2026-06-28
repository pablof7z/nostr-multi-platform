import Foundation

/// One section in the gallery sidebar (e.g. "User", "Content"). Groups a set
/// of `RegistryComponent` rows that share a domain.
struct RegistrySection: Decodable, Identifiable, Hashable {
    let id: String
    let label: String
    let components: [RegistryComponent]
}

/// One component row inside a section. `id` doubles as the dispatch key the
/// detail view uses to pick the right page builder.
struct RegistryComponent: Decodable, Identifiable, Hashable {
    /// Stable registry slug (e.g. `"user-avatar"`). MUST match the slugs
    /// `crates/nmp-cli/registry/swiftui/` uses on disk.
    let id: String
    /// Display label — the public Swift type name the component exports.
    let label: String
    /// Short, single-sentence description shown under `label` in the list.
    let description: String
}

private struct GalleryRegistryManifest: Decodable {
    let schema: String
    let sections: [RegistrySection]
}

private enum GalleryRegistryLoader {
    static func loadSections() -> [RegistrySection] {
        guard let ptr = nmp_app_gallery_registry_json() else {
            fatalError("nmp_app_gallery_registry_json returned null")
        }
        let json = String(cString: ptr)
        do {
            let manifest = try JSONDecoder().decode(
                GalleryRegistryManifest.self,
                from: Data(json.utf8))
            guard manifest.schema == "nmp.gallery.registry/1" else {
                fatalError("unexpected gallery registry schema: \(manifest.schema)")
            }
            guard !manifest.sections.isEmpty,
                  manifest.sections.allSatisfy({ !$0.components.isEmpty }) else {
                fatalError("gallery registry must contain non-empty sections")
            }
            return manifest.sections
        } catch {
            fatalError("failed to decode gallery registry: \(error)")
        }
    }
}

/// Authoritative catalog of gallery components, decoded from the Rust-embedded
/// `apps/nmp-gallery/registry.json`.
let GALLERY_SECTIONS: [RegistrySection] = GalleryRegistryLoader.loadSections()
