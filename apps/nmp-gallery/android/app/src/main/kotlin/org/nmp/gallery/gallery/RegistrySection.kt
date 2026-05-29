package org.nmp.gallery.gallery

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json

/**
 * Registry-section / component manifest the gallery navigation iterates over.
 * Mirrors the SwiftUI gallery's `RegistrySection` / `RegistryComponent` shape
 * so iOS / Android stay 1:1.
 *
 * Keep IDs stable — they are part of the navigation route URL.
 *
 * The live registry is sourced from `bridge.registryJson()` (the canonical
 * `registry.json` embedded in the Rust crate) via [parseRegistryJson].
 * [REGISTRY_SECTIONS] is kept as a compile-time fallback only.
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

// ── JSON wire shapes (kotlinx.serialization) ─────────────────────────────────

@Serializable
private data class RegistryJson(
    @SerialName("schema") val schema: String = "",
    @SerialName("sections") val sections: List<SectionJson> = emptyList(),
)

@Serializable
private data class SectionJson(
    @SerialName("id") val id: String = "",
    @SerialName("label") val label: String = "",
    @SerialName("components") val components: List<ComponentJson> = emptyList(),
)

@Serializable
private data class ComponentJson(
    @SerialName("id") val id: String = "",
    @SerialName("label") val label: String = "",
    @SerialName("description") val description: String = "",
)

private val registryJsonParser = Json {
    ignoreUnknownKeys = true
    isLenient = true
}

/**
 * Parse the JSON produced by `bridge.registryJson()` into a typed list.
 *
 * Returns `null` on any parse failure so callers can fall back to
 * [REGISTRY_SECTIONS].
 */
fun parseRegistryJson(raw: String): List<RegistrySection>? {
    if (raw.isBlank()) return null
    return runCatching {
        val wire = registryJsonParser.decodeFromString(RegistryJson.serializer(), raw)
        wire.sections.map { s ->
            RegistrySection(
                id = s.id,
                label = s.label,
                components = s.components.map { c ->
                    RegistryComponent(id = c.id, label = c.label, description = c.description)
                },
            )
        }.takeIf { it.isNotEmpty() }
    }.getOrNull()
}

// ── Compile-time fallback ─────────────────────────────────────────────────────

/**
 * Hardcoded fallback used only when [parseRegistryJson] returns null (e.g.
 * during unit tests that do not link the native library). Production paths
 * should always use the live JSON returned by `bridge.registryJson()`.
 */
val REGISTRY_SECTIONS: List<RegistrySection> = listOf(
    RegistrySection(
        id = "relay",
        label = "Relay",
        components = listOf(
            RegistryComponent("relay-list", "NostrRelayList", "Relay URLs with role badges and connection status dots"),
        ),
    ),
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
    RegistrySection(
        id = "embeds",
        label = "Embeds & Kinds",
        components = listOf(
            RegistryComponent("embed-article", "ArticleEmbed", "Kind:30023 long-form article — hero image, title, summary"),
            RegistryComponent("embed-profile", "ProfileEmbed", "Inline npub mention chip — kind:0 profile"),
            RegistryComponent("embed-note", "NoteEmbed", "Kind:1 short text note via nevent claim"),
            RegistryComponent("embed-highlight", "HighlightEmbed", "Kind:9802 highlight — pull-quote + source"),
        ),
    ),
)

/** Resolve a component id back to its (section, component) tuple. */
fun findComponent(
    componentId: String,
    sections: List<RegistrySection> = REGISTRY_SECTIONS,
): Pair<RegistrySection, RegistryComponent>? {
    for (section in sections) {
        section.components.firstOrNull { it.id == componentId }?.let { return section to it }
    }
    return null
}

/** Resolve a section id back to its [RegistrySection]. */
fun findSection(
    sectionId: String,
    sections: List<RegistrySection> = REGISTRY_SECTIONS,
): RegistrySection? = sections.firstOrNull { it.id == sectionId }
