use serde::Deserialize;

const REGISTRY_SECTIONS: &[&str] = &[
    include_str!("../../nmp-component-registry/registry/registry.toml"),
    include_str!("../../nmp-component-registry/registry/registry.swiftui.toml"),
    include_str!("../../nmp-component-registry/registry/registry.compose.toml"),
    include_str!("../../nmp-component-registry/registry/registry.tui.toml"),
    include_str!("../../nmp-component-registry/registry/registry.desktop.toml"),
    include_str!("../../nmp-component-registry/registry/registry.web.toml"),
];

#[derive(Deserialize)]
struct Manifest {
    components: Vec<Component>,
}

#[derive(Deserialize)]
struct Component {
    id: String,
    target: String,
    #[serde(default)]
    dependencies: Vec<String>,
    files: Vec<ComponentFile>,
}

#[derive(Deserialize)]
struct ComponentFile {
    source: String,
    target: String,
    role: String,
}

#[test]
fn chat_component_family_has_swiftui_and_compose_minimum_contract() {
    let manifest = toml::from_str::<Manifest>(&REGISTRY_SECTIONS.join("\n")).unwrap();

    for platform in ["swiftui", "compose"] {
        assert_component(&manifest, platform, "chat-core", &[], "NostrGroupChatWire");
        assert_component(
            &manifest,
            platform,
            "chat-message-row",
            &["chat-core", "user-avatar", "user-name"],
            "NostrGroupMessageRow",
        );
        assert_component(
            &manifest,
            platform,
            "chat-composer",
            &["chat-core"],
            "NostrGroupComposer",
        );
        assert_component(
            &manifest,
            platform,
            "chat-roster-list",
            &["chat-core", "user-avatar", "user-name"],
            "NostrGroupRosterList",
        );
    }
}

fn assert_component(
    manifest: &Manifest,
    platform: &str,
    slug: &str,
    deps: &[&str],
    source_stem: &str,
) {
    let id = format!("{platform}/{slug}");
    let component = manifest
        .components
        .iter()
        .find(|component| component.id == id)
        .unwrap_or_else(|| panic!("missing chat component {id}"));

    assert_eq!(component.target, platform, "{id} target drifted");

    let expected_deps = deps
        .iter()
        .map(|dep| format!("{platform}/{dep}"))
        .collect::<Vec<_>>();
    assert_eq!(component.dependencies, expected_deps, "{id} deps drifted");

    let source = component
        .files
        .iter()
        .find(|file| file.role == "source")
        .unwrap_or_else(|| panic!("{id} has no source file"));
    assert!(
        source.source.starts_with(&format!("{platform}/{slug}/")),
        "{id} source should stay in its platform/slug namespace"
    );
    assert!(
        source.source.contains(source_stem),
        "{id} source should expose {source_stem}"
    );
    assert!(
        source.target.starts_with("Components/NostrChat/"),
        "{id} target must install under the chat component namespace"
    );
}
