use serde::Deserialize;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

const BUILTIN_REGISTRY: &str = include_str!("../../registry/registry.toml");
const BUILTIN_FILES: &[(&str, &str)] = &[
    (
        "swiftui/content-core/NostrContentRenderer.swift",
        include_str!("../../registry/swiftui/content-core/NostrContentRenderer.swift"),
    ),
    (
        "swiftui/content-core/ContentTreeWire.swift",
        include_str!("../../registry/swiftui/content-core/ContentTreeWire.swift"),
    ),
    (
        "swiftui/content-minimal/NostrMinimalContentView.swift",
        include_str!("../../registry/swiftui/content-minimal/NostrMinimalContentView.swift"),
    ),
    (
        "swiftui/content-minimal/Examples/NostrMinimalContentPreview.swift",
        include_str!(
            "../../registry/swiftui/content-minimal/Examples/NostrMinimalContentPreview.swift"
        ),
    ),
    (
        "swiftui/content-mention-chip/NostrMentionChip.swift",
        include_str!("../../registry/swiftui/content-mention-chip/NostrMentionChip.swift"),
    ),
    (
        "swiftui/content-media-grid/NostrMediaGrid.swift",
        include_str!("../../registry/swiftui/content-media-grid/NostrMediaGrid.swift"),
    ),
    (
        "swiftui/content-quote-card/NostrQuoteCard.swift",
        include_str!("../../registry/swiftui/content-quote-card/NostrQuoteCard.swift"),
    ),
    (
        "swiftui/content-view/NostrContentView.swift",
        include_str!("../../registry/swiftui/content-view/NostrContentView.swift"),
    ),
    (
        "swiftui/content-view/NostrContentGrouping.swift",
        include_str!("../../registry/swiftui/content-view/NostrContentGrouping.swift"),
    ),
    (
        "swiftui/content-view/Examples/NostrContentViewPreview.swift",
        include_str!("../../registry/swiftui/content-view/Examples/NostrContentViewPreview.swift"),
    ),
    (
        "swiftui/login-block/NostrLoginBlock.swift",
        include_str!("../../registry/swiftui/login-block/NostrLoginBlock.swift"),
    ),
    // Compose (M16-C4)
    (
        "compose/content-core/NostrContentRenderer.kt",
        include_str!("../../registry/compose/content-core/NostrContentRenderer.kt"),
    ),
    (
        "compose/content-core/ContentTreeWire.kt",
        include_str!("../../registry/compose/content-core/ContentTreeWire.kt"),
    ),
    (
        "compose/content-mention-chip/NostrMentionChip.kt",
        include_str!("../../registry/compose/content-mention-chip/NostrMentionChip.kt"),
    ),
    (
        "compose/content-media-grid/NostrMediaGrid.kt",
        include_str!("../../registry/compose/content-media-grid/NostrMediaGrid.kt"),
    ),
    (
        "compose/content-quote-card/NostrQuoteCard.kt",
        include_str!("../../registry/compose/content-quote-card/NostrQuoteCard.kt"),
    ),
    (
        "compose/content-view/NostrContentView.kt",
        include_str!("../../registry/compose/content-view/NostrContentView.kt"),
    ),
    (
        "compose/content-view/NostrContentGrouping.kt",
        include_str!("../../registry/compose/content-view/NostrContentGrouping.kt"),
    ),
    // Ratatui content widgets.
    (
        "tui/content-core/content_tree_wire.rs",
        include_str!("../../registry/tui/content-core/content_tree_wire.rs"),
    ),
    (
        "tui/content-core/content_render_data.rs",
        include_str!("../../registry/tui/content-core/content_render_data.rs"),
    ),
    (
        "tui/content-minimal/nostr_minimal_content.rs",
        include_str!("../../registry/tui/content-minimal/nostr_minimal_content.rs"),
    ),
    (
        "tui/content-mention-chip/nostr_mention_chip.rs",
        include_str!("../../registry/tui/content-mention-chip/nostr_mention_chip.rs"),
    ),
    (
        "tui/content-media-grid/nostr_media_grid.rs",
        include_str!("../../registry/tui/content-media-grid/nostr_media_grid.rs"),
    ),
    (
        "tui/content-quote-card/nostr_quote_card.rs",
        include_str!("../../registry/tui/content-quote-card/nostr_quote_card.rs"),
    ),
    (
        "tui/content-view/nostr_content_view.rs",
        include_str!("../../registry/tui/content-view/nostr_content_view.rs"),
    ),
    // Ratatui user profile widgets.
    (
        "tui/user-avatar/profile_wire.rs",
        include_str!("../../registry/tui/user-avatar/profile_wire.rs"),
    ),
    (
        "tui/user-avatar/nostr_avatar.rs",
        include_str!("../../registry/tui/user-avatar/nostr_avatar.rs"),
    ),
    (
        "tui/user-name/nostr_profile_name.rs",
        include_str!("../../registry/tui/user-name/nostr_profile_name.rs"),
    ),
    (
        "tui/user-nip05/nostr_nip05_badge.rs",
        include_str!("../../registry/tui/user-nip05/nostr_nip05_badge.rs"),
    ),
    (
        "tui/user-npub/nostr_npub_chip.rs",
        include_str!("../../registry/tui/user-npub/nostr_npub_chip.rs"),
    ),
    (
        "tui/user-card/nostr_user_card.rs",
        include_str!("../../registry/tui/user-card/nostr_user_card.rs"),
    ),
];

#[derive(Deserialize)]
struct RegistryManifest {
    registry_id: String,
    components: Vec<RegistryComponent>,
}

#[derive(Deserialize)]
pub(super) struct RegistryComponent {
    pub(super) id: String,
    pub(super) version: String,
    pub(super) target: String,
    #[serde(default)]
    dependencies: Vec<String>,
    pub(super) files: Vec<RegistryFile>,
}

#[derive(Deserialize)]
pub(super) struct RegistryFile {
    pub(super) source: String,
    pub(super) target: String,
    pub(super) role: String,
}

pub(super) struct Registry {
    pub(super) id: String,
    root: RegistryRoot,
    components: Vec<RegistryComponent>,
}

enum RegistryRoot {
    Builtin,
    Filesystem(PathBuf),
}

impl Registry {
    pub(super) fn load(path: Option<PathBuf>) -> Result<Self, String> {
        let (manifest, root) = match path {
            Some(path) => {
                let manifest = if path.is_dir() {
                    path.join("registry.toml")
                } else {
                    path.clone()
                };
                let root = manifest.parent().unwrap_or(Path::new(".")).to_path_buf();
                let content = fs::read_to_string(&manifest)
                    .map_err(|e| format!("{}: {e}", manifest.display()))?;
                (content, RegistryRoot::Filesystem(root))
            }
            None => (BUILTIN_REGISTRY.to_string(), RegistryRoot::Builtin),
        };
        let parsed = toml::from_str::<RegistryManifest>(&manifest)
            .map_err(|e| format!("invalid component registry: {e}"))?;
        Ok(Self {
            id: parsed.registry_id,
            root,
            components: parsed.components,
        })
    }

    pub(super) fn resolve(&self, id: &str) -> Result<Vec<&RegistryComponent>, String> {
        let mut seen = HashSet::new();
        let mut order = Vec::new();
        self.collect(id, &mut seen, &mut order)?;
        Ok(order)
    }

    pub(super) fn read_source(&self, path: &Path) -> Result<String, String> {
        match &self.root {
            RegistryRoot::Builtin => BUILTIN_FILES
                .iter()
                .find(|(candidate, _)| Path::new(candidate) == path)
                .map(|(_, content)| (*content).to_string())
                .ok_or_else(|| format!("builtin component source missing: {}", path.display())),
            RegistryRoot::Filesystem(root) => fs::read_to_string(root.join(path))
                .map_err(|e| format!("{}: {e}", root.join(path).display())),
        }
    }

    fn collect<'a>(
        &'a self,
        id: &str,
        seen: &mut HashSet<String>,
        order: &mut Vec<&'a RegistryComponent>,
    ) -> Result<(), String> {
        if !seen.insert(id.to_string()) {
            return Ok(());
        }
        let component = self
            .components
            .iter()
            .find(|component| component.id == id)
            .ok_or_else(|| format!("unknown component `{id}`"))?;
        for dependency in &component.dependencies {
            self.collect(dependency, seen, order)?;
        }
        order.push(component);
        Ok(())
    }
}
