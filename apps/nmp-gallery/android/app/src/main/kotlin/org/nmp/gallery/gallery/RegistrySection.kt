package org.nmp.gallery.gallery

/**
 * Static registry-section / component manifest the gallery navigation
 * iterates over. Mirrors the SwiftUI gallery's `RegistrySection` /
 * `RegistryComponent` shape so iOS / Android stay 1:1.
 *
 * Keep IDs stable — they are part of the navigation route URL.
 */
data class RegistrySection(
    val id: String,
    val label: String,
    val components: List<RegistryComponent>,
)

data class RegistryComponent(
    val id: String,
    val label: String,
    val description: String,
)

val REGISTRY_SECTIONS: List<RegistrySection> = listOf(
    RegistrySection(
        id = "user",
        label = "User",
        components = listOf(
            RegistryComponent("user-avatar", "NostrAvatar", "Circular avatar with identicon fallback"),
            RegistryComponent("user-name", "NostrProfileName", "Display name with npub fallback"),
            RegistryComponent("user-nip05", "NostrNip05Badge", "NIP-05 verified identity badge"),
            RegistryComponent("user-npub", "NostrNpubChip", "Copyable npub chip"),
            RegistryComponent("user-card", "NostrUserCard", "Compact avatar + name + nip05 row"),
        ),
    ),
    RegistrySection(
        id = "content",
        label = "Content",
        components = listOf(
            RegistryComponent("content-core", "ContentTreeWire", "Wire type + identicon renderer"),
            RegistryComponent("content-view", "NostrContentView", "Full rich content renderer"),
            RegistryComponent("content-mention-chip", "NostrMentionChip", "Tappable @mention chip"),
            RegistryComponent("content-minimal", "NostrMinimalContentView", "Flow-layout minimal renderer"),
            RegistryComponent("content-media-grid", "NostrMediaGrid", "Photo-style image grid"),
            RegistryComponent("content-quote-card", "NostrQuoteCard", "Embedded event quote card"),
        ),
    ),
)

/** Resolve a component id back to its (section, component) tuple. */
fun findComponent(componentId: String): Pair<RegistrySection, RegistryComponent>? {
    for (section in REGISTRY_SECTIONS) {
        section.components.firstOrNull { it.id == componentId }?.let { return section to it }
    }
    return null
}

/** Resolve a section id back to its [RegistrySection]. */
fun findSection(sectionId: String): RegistrySection? =
    REGISTRY_SECTIONS.firstOrNull { it.id == sectionId }
