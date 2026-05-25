//! Regression guard for the web registry mirror.
//!
//! The CLI manifest is the install authority. The showcase site may add
//! marketing copy, screenshots, and customization notes, but the fields that
//! decide what users install must match `registry.toml`.

use serde::Deserialize;

const CLI_REGISTRY: &str = include_str!("../registry/registry.toml");
const WEB_REGISTRY_INDEX: &str = include_str!("../../../web/registry/src/registry.ts");
const WEB_REGISTRY_TYPES: &str = include_str!("../../../web/registry/src/registry/types.ts");
const WEB_REGISTRY_CONTENT: &str = include_str!("../../../web/registry/src/registry/content.ts");
const WEB_REGISTRY_USER: &str = include_str!("../../../web/registry/src/registry/user.ts");
const WEB_REGISTRY_RELAY: &str = include_str!("../../../web/registry/src/registry/relay.ts");

#[derive(Deserialize)]
struct RegistryManifest {
    components: Vec<RegistryComponent>,
}

#[derive(Deserialize)]
struct RegistryComponent {
    id: String,
    version: String,
    target: String,
    #[serde(default)]
    dependencies: Vec<String>,
    files: Vec<RegistryFile>,
}

#[derive(Deserialize)]
struct RegistryFile {
    source: String,
    target: String,
    role: String,
}

#[test]
fn web_registry_install_metadata_mirrors_cli_manifest() {
    let manifest = toml::from_str::<RegistryManifest>(CLI_REGISTRY).unwrap();
    let web_registry = web_registry_source();
    let mut cli_ids = manifest
        .components
        .iter()
        .map(|component| component.id.clone())
        .collect::<Vec<_>>();
    cli_ids.sort();

    let mut web_ids = web_install_ids(&web_registry);
    web_ids.sort();
    assert_eq!(web_ids, cli_ids, "web registry component ids drifted");

    for component in manifest.components {
        assert!(
            component.id.starts_with(&format!("{}/", component.target)),
            "{} id/target mismatch in CLI manifest",
            component.id
        );

        let block = web_impl_block(&web_registry, &component.id);
        assert_contains(&block, &format!("version: \"{}\"", component.version));
        assert_contains(&block, &format!("installId: \"{}\"", component.id));
        let expected_dependencies = component
            .dependencies
            .iter()
            .map(|dependency| component_slug(dependency))
            .collect::<Vec<_>>();
        assert_eq!(
            quoted_values(array_field(&block, "dependencies")),
            expected_dependencies,
            "{} dependency mirror drifted",
            component.id
        );

        let files = array_field(&block, "files");
        for file in component.files {
            assert_contains(&files, &format!("source: \"{}\"", file.source));
            assert_contains(&files, &format!("target: \"{}\"", file.target));
            assert_contains(&files, &format!("role: \"{}\"", file.role));
        }
    }
}

fn web_registry_source() -> String {
    [
        WEB_REGISTRY_INDEX,
        WEB_REGISTRY_TYPES,
        WEB_REGISTRY_CONTENT,
        WEB_REGISTRY_USER,
        WEB_REGISTRY_RELAY,
    ]
    .join("\n")
}

fn web_install_ids(source: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let mut rest = source;
    while let Some(index) = rest.find("installId: \"") {
        let start = index + "installId: \"".len();
        let after_start = &rest[start..];
        let end = after_start.find('"').unwrap();
        ids.push(after_start[..end].to_string());
        rest = &after_start[end..];
    }
    ids
}

fn web_impl_block(source: &str, id: &str) -> String {
    let needle = format!("installId: \"{id}\"");
    let id_index = source
        .find(&needle)
        .unwrap_or_else(|| panic!("web registry missing implementation {id}"));
    let object_start = source[..id_index]
        .rfind('{')
        .unwrap_or_else(|| panic!("web registry implementation {id} has no object start"));
    let slice = &source[object_start..];
    let mut depth = 0usize;
    for (offset, ch) in slice.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return slice[..=offset].to_string();
                }
            }
            _ => {}
        }
    }
    panic!("web registry implementation {id} has no object end");
}

fn component_slug(id: &str) -> String {
    id.split_once('/')
        .map(|(_, slug)| slug.to_string())
        .unwrap_or_else(|| id.to_string())
}

fn array_field<'a>(block: &'a str, field: &str) -> &'a str {
    let needle = format!("{field}: [");
    let field_index = block
        .find(&needle)
        .unwrap_or_else(|| panic!("missing array field {field}"));
    let slice = &block[field_index + field.len() + 2..];
    let mut depth = 0usize;
    for (offset, ch) in slice.char_indices() {
        match ch {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    return &slice[..=offset];
                }
            }
            _ => {}
        }
    }
    panic!("array field {field} has no end");
}

fn quoted_values(value: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut rest = value;
    while let Some(index) = rest.find('"') {
        let after_open = &rest[index + 1..];
        let end = after_open.find('"').unwrap();
        values.push(after_open[..end].to_string());
        rest = &after_open[end + 1..];
    }
    values
}

fn assert_contains(haystack: &str, needle: &str) {
    assert!(
        haystack.contains(needle),
        "expected web registry block to contain `{needle}`"
    );
}
