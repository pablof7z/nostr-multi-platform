package org.nmp.android.model

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

@Serializable
data class RelayRoleOption(
    val value: String = "",
    val label: String = "",
    val tint: String = "",
    @SerialName("is_default") val isDefault: Boolean = false,
)

fun defaultRelayRoleOptions(): List<RelayRoleOption> = listOf(
    RelayRoleOption(value = "both,indexer", label = "Both + Index", tint = "accent"),
    RelayRoleOption(value = "both", label = "Both", tint = "accent", isDefault = true),
    RelayRoleOption(value = "read", label = "Read", tint = "info"),
    RelayRoleOption(value = "write", label = "Write", tint = "success"),
    RelayRoleOption(value = "indexer", label = "Index", tint = "neutral"),
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
