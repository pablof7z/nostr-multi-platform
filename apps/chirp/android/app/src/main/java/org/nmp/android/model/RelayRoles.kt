package org.nmp.android.model

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * A single relay-role option emitted by the `relay_role_options` projection.
 *
 * `label` was removed from the wire (#1678, D7 — presentation artifact); it is
 * now a computed shell property derived from `value`. Class-body properties are
 * automatically excluded from kotlinx.serialization (not in the primary
 * constructor), so no `@Transient` is required.
 */
@Serializable
data class RelayRoleOption(
    val value: String = "",
    val tint: String = "",
    @SerialName("is_default") val isDefault: Boolean = false,
) {
    /** Human-readable label derived from the raw `value` token (#1678 / D7). */
    val label: String
        get() = when (value) {
            "both,indexer" -> "Both + Index"
            "both"         -> "Both"
            "read"         -> "Read"
            "write"        -> "Write"
            "indexer"      -> "Index"
            else           -> value
        }
}

fun defaultRelayRoleOptions(): List<RelayRoleOption> = listOf(
    RelayRoleOption(value = "both,indexer", tint = "accent"),
    RelayRoleOption(value = "both", tint = "accent", isDefault = true),
    RelayRoleOption(value = "read", tint = "info"),
    RelayRoleOption(value = "write", tint = "success"),
    RelayRoleOption(value = "indexer", tint = "neutral"),
)

fun defaultRelayRoleValue(options: List<RelayRoleOption>): String =
    options.firstOrNull { it.isDefault }?.value
        ?: options.firstOrNull()?.value
        ?: "both"

fun canonicalRelayRoleInput(input: String, options: List<RelayRoleOption>): String? {
    val trimmed = input.trim()
    if (trimmed.isEmpty()) return null
    options.firstOrNull { it.value.equals(trimmed, ignoreCase = true) }?.let { return it.value }
    options.firstOrNull { it.label.equals(trimmed, ignoreCase = true) }?.let { return it.value }
    return when (trimmed.lowercase()) {
        "readwrite", "read write", "read+write", "read,write" ->
            options.firstOrNull { it.value == "both" }?.value
        else -> null
    }
}

fun relayRoleLabel(value: String, options: List<RelayRoleOption>): String =
    options.firstOrNull { it.value == value }?.label ?: value
