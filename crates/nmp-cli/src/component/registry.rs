use serde::Deserialize;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

// The catalog is split by platform target (file-size rule); merge order must
// match REGISTRY_SECTION_FILES in crate::registry_manifest.
const BUILTIN_REGISTRY_SECTIONS: &[&str] = &[
    include_str!("../../registry/registry.toml"),
    include_str!("../../registry/registry.swiftui.toml"),
    include_str!("../../registry/registry.compose.toml"),
    include_str!("../../registry/registry.tui.toml"),
    include_str!("../../registry/registry.desktop.toml"),
    include_str!("../../registry/registry.web.toml"),
];
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
        "swiftui/render-identity/RenderIdentifiable.swift",
        include_str!("../../registry/swiftui/render-identity/RenderIdentifiable.swift"),
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
    (
        "swiftui/login-block/KnownSigners.generated.swift",
        include_str!("../../registry/swiftui/login-block/KnownSigners.generated.swift"),
    ),
    (
        "swiftui/relay-list/NostrRelayList.swift",
        include_str!("../../registry/swiftui/relay-list/NostrRelayList.swift"),
    ),
    (
        "swiftui/relay-list/Examples/NostrRelayListPreview.swift",
        include_str!("../../registry/swiftui/relay-list/Examples/NostrRelayListPreview.swift"),
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
        "tui/content-core/ratatui_text_wrap.rs",
        include_str!("../../registry/tui/content-core/ratatui_text_wrap.rs"),
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
        "tui/content-kind-registry/mod.rs",
        include_str!("../../registry/tui/content-kind-registry/mod.rs"),
    ),
    (
        "tui/content-kind-registry/kind_renderer.rs",
        include_str!("../../registry/tui/content-kind-registry/kind_renderer.rs"),
    ),
    (
        "tui/content-kind-registry/nostr_kind_registry.rs",
        include_str!("../../registry/tui/content-kind-registry/nostr_kind_registry.rs"),
    ),
    (
        "tui/content-kind-registry/embed_chrome_container.rs",
        include_str!("../../registry/tui/content-kind-registry/embed_chrome_container.rs"),
    ),
    (
        "tui/content-kind-registry/embedded_event.rs",
        include_str!("../../registry/tui/content-kind-registry/embedded_event.rs"),
    ),
    (
        "tui/content-view/nostr_content_view.rs",
        include_str!("../../registry/tui/content-view/nostr_content_view.rs"),
    ),
    (
        "tui/content-view/nostr_content_widget.rs",
        include_str!("../../registry/tui/content-view/nostr_content_widget.rs"),
    ),
    // Ratatui user profile widgets.
    (
        "tui/user-core/profile_wire.rs",
        include_str!("../../registry/tui/user-core/profile_wire.rs"),
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
    (
        "swiftui/user-avatar/ProfileWire.swift",
        include_str!("../../registry/swiftui/user-avatar/ProfileWire.swift"),
    ),
    (
        "swiftui/user-avatar/NostrProfileHost.swift",
        include_str!("../../registry/swiftui/user-avatar/NostrProfileHost.swift"),
    ),
    (
        "swiftui/user-avatar/NostrAvatar.swift",
        include_str!("../../registry/swiftui/user-avatar/NostrAvatar.swift"),
    ),
    (
        "swiftui/user-name/NostrProfileName.swift",
        include_str!("../../registry/swiftui/user-name/NostrProfileName.swift"),
    ),
    (
        "swiftui/user-nip05/NostrNip05Badge.swift",
        include_str!("../../registry/swiftui/user-nip05/NostrNip05Badge.swift"),
    ),
    (
        "swiftui/user-npub/NostrNpubChip.swift",
        include_str!("../../registry/swiftui/user-npub/NostrNpubChip.swift"),
    ),
    (
        "swiftui/user-card/NostrUserCard.swift",
        include_str!("../../registry/swiftui/user-card/NostrUserCard.swift"),
    ),
    (
        "compose/user-avatar/ProfileWire.kt",
        include_str!("../../registry/compose/user-avatar/ProfileWire.kt"),
    ),
    (
        "compose/user-avatar/NostrProfileHost.kt",
        include_str!("../../registry/compose/user-avatar/NostrProfileHost.kt"),
    ),
    (
        "compose/user-avatar/NostrAvatar.kt",
        include_str!("../../registry/compose/user-avatar/NostrAvatar.kt"),
    ),
    (
        "compose/user-name/NostrProfileName.kt",
        include_str!("../../registry/compose/user-name/NostrProfileName.kt"),
    ),
    (
        "compose/user-nip05/NostrNip05Badge.kt",
        include_str!("../../registry/compose/user-nip05/NostrNip05Badge.kt"),
    ),
    (
        "compose/user-npub/NostrNpubChip.kt",
        include_str!("../../registry/compose/user-npub/NostrNpubChip.kt"),
    ),
    (
        "compose/user-card/NostrUserCard.kt",
        include_str!("../../registry/compose/user-card/NostrUserCard.kt"),
    ),
    (
        "desktop/user-core/profile_wire.rs",
        include_str!("../../registry/desktop/user-core/profile_wire.rs"),
    ),
    (
        "desktop/user-avatar/user_avatar.rs",
        include_str!("../../registry/desktop/user-avatar/user_avatar.rs"),
    ),
    (
        "desktop/user-name/user_name.rs",
        include_str!("../../registry/desktop/user-name/user_name.rs"),
    ),
    (
        "desktop/user-nip05/user_nip05.rs",
        include_str!("../../registry/desktop/user-nip05/user_nip05.rs"),
    ),
    (
        "desktop/user-npub/user_npub.rs",
        include_str!("../../registry/desktop/user-npub/user_npub.rs"),
    ),
    (
        "desktop/user-card/user_card.rs",
        include_str!("../../registry/desktop/user-card/user_card.rs"),
    ),
    (
        "swiftui/content-kind-registry/EmbedKindProjection.swift",
        include_str!("../../registry/swiftui/content-kind-registry/EmbedKindProjection.swift"),
    ),
    (
        "swiftui/content-kind-registry/NostrKindRegistry.swift",
        include_str!("../../registry/swiftui/content-kind-registry/NostrKindRegistry.swift"),
    ),
    (
        "swiftui/content-kind-registry/EmbedChromeContainer.swift",
        include_str!("../../registry/swiftui/content-kind-registry/EmbedChromeContainer.swift"),
    ),
    (
        "swiftui/content-kind-registry/EmbeddedEvent.swift",
        include_str!("../../registry/swiftui/content-kind-registry/EmbeddedEvent.swift"),
    ),
    (
        "swiftui/content-kind-30023/ArticleEmbed.swift",
        include_str!("../../registry/swiftui/content-kind-30023/ArticleEmbed.swift"),
    ),
    (
        "swiftui/content-kind-0/ProfileEmbed.swift",
        include_str!("../../registry/swiftui/content-kind-0/ProfileEmbed.swift"),
    ),
    (
        "swiftui/content-kind-9802/HighlightEmbed.swift",
        include_str!("../../registry/swiftui/content-kind-9802/HighlightEmbed.swift"),
    ),
    (
        "compose/content-kind-30023/NostrArticleCard.kt",
        include_str!("../../registry/compose/content-kind-30023/NostrArticleCard.kt"),
    ),
    (
        "compose/content-kind-0/NostrProfileCard.kt",
        include_str!("../../registry/compose/content-kind-0/NostrProfileCard.kt"),
    ),
    (
        "compose/login-block/NostrLoginBlock.kt",
        include_str!("../../registry/compose/login-block/NostrLoginBlock.kt"),
    ),
    (
        "compose/login-block/ExternalSignerWire.kt",
        include_str!("../../registry/compose/login-block/ExternalSignerWire.kt"),
    ),
    (
        "compose/login-block/KnownSigners.generated.kt",
        include_str!("../../registry/compose/login-block/KnownSigners.generated.kt"),
    ),
    (
        "compose/login-block/ExternalSignerCapabilityBridge.kt",
        include_str!("../../registry/compose/login-block/ExternalSignerCapabilityBridge.kt"),
    ),
    (
        "compose/login-block/AmberIntentCodec.kt",
        include_str!("../../registry/compose/login-block/AmberIntentCodec.kt"),
    ),
    (
        "desktop/content-kind-30023/embed_article.rs",
        include_str!("../../registry/desktop/content-kind-30023/embed_article.rs"),
    ),
    (
        "desktop/content-kind-0/profile_card.rs",
        include_str!("../../registry/desktop/content-kind-0/profile_card.rs"),
    ),
    (
        "web/login-block/NostrLoginBlock.tsx",
        include_str!("../../registry/web/login-block/NostrLoginBlock.tsx"),
    ),
    (
        "web/relay-list/NostrRelayList.tsx",
        include_str!("../../registry/web/relay-list/NostrRelayList.tsx"),
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
                let content = crate::registry_manifest::read_manifest_with_sections(&manifest)?;
                (content, RegistryRoot::Filesystem(root))
            }
            None => (BUILTIN_REGISTRY_SECTIONS.join("\n"), RegistryRoot::Builtin),
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

#[cfg(test)]
mod tests {
    use super::*;

    // Regression gate: every `source` file declared in the builtin manifest must
    // be embedded in BUILTIN_FILES so `read_source` resolves it. Without this,
    // manifest-vs-BUILTIN_FILES drift ships silently (a component declares a file
    // the embedded binary can't install — install fails with
    // "builtin component source missing"). This caught the unwired
    // swiftui/user-*, compose/user-*, desktop/user-*, login-block, and
    // content-kind-* sources.
    #[test]
    fn every_declared_builtin_source_resolves() {
        let registry = Registry::load(None).expect("builtin registry must load");
        let mut missing = Vec::new();
        for component in &registry.components {
            for file in &component.files {
                if file.role != "source" {
                    continue;
                }
                if registry.read_source(Path::new(&file.source)).is_err() {
                    missing.push(format!("{} ({})", file.source, component.id));
                }
            }
        }
        assert!(
            missing.is_empty(),
            "builtin manifest declares source files not embedded in BUILTIN_FILES \
             (add include_str! entries in registry.rs):\n{}",
            missing.join("\n")
        );
    }
}
