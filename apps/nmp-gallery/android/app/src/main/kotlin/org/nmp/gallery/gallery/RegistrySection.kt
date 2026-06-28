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
 */
fun parseRegistryJson(raw: String): List<RegistrySection> {
    require(raw.isNotBlank()) { "gallery registry JSON is empty" }
    val wire = registryJsonParser.decodeFromString(RegistryJson.serializer(), raw)
    require(wire.schema == "nmp.gallery.registry/1") {
        "unexpected gallery registry schema: ${wire.schema}"
    }
    val sections = wire.sections.map { s ->
        RegistrySection(
            id = s.id,
            label = s.label,
            components = s.components.map { c ->
                RegistryComponent(id = c.id, label = c.label, description = c.description)
            },
        )
    }
    require(sections.isNotEmpty() && sections.all { it.components.isNotEmpty() }) {
        "gallery registry must contain non-empty sections"
    }
    return sections
}

/** Resolve a component id back to its (section, component) tuple. */
fun findComponent(
    componentId: String,
    sections: List<RegistrySection>,
): Pair<RegistrySection, RegistryComponent>? {
    for (section in sections) {
        section.components.firstOrNull { it.id == componentId }?.let { return section to it }
    }
    return null
}

/** Resolve a section id back to its [RegistrySection]. */
fun findSection(
    sectionId: String,
    sections: List<RegistrySection>,
): RegistrySection? = sections.firstOrNull { it.id == sectionId }
