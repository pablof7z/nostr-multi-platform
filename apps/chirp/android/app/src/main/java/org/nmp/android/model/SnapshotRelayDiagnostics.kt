package org.nmp.android.model

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * Detailed relay diagnostics — `projections["relay_diagnostics"]` (`KRDG`).
 * Android peer of iOS `RelayDiagnosticsSnapshot` (`TypedProjectionGlue`).
 *
 * Field-for-field mirror of the kernel projection. Raw values (role,
 * connection, auth as lowercase strings; bytes as Long counters;
 * discoveryKinds as List<Long>) are carried; display formatting is the
 * shell's job via computed properties.
 */
@Serializable
data class RelayDiagnosticsSnapshot(
    val relays: List<RelayDiagnosticsRow> = emptyList(),
    val interests: List<RelayDiagnosticsInterest> = emptyList(),
)

@Serializable
data class RelayDiagnosticsRow(
    val relayUrl: String = "",
    /** Raw role string from Rust (e.g. "content", "indexer", "both"). Shell formats. */
    val role: String = "",
    /** Raw connection string from Rust (e.g. "connected", "closed"). Shell formats. */
    val connection: String = "",
    /** Raw auth string from Rust (e.g. "ok", "pending", "—"). Shell formats. */
    val auth: String = "",
    val totalSubCount: Int = 0,
    val activeSubCount: Int = 0,
    val eosedSubCount: Int = 0,
    val totalEventsRx: Long = 0,
    val reconnectCount: Int = 0,
    /** Raw bytes received counter. Shell derives display string. */
    val bytesRx: Long = 0,
    /** Raw bytes transmitted counter. Shell derives display string. */
    val bytesTx: Long = 0,
    // aim.md §62: raw Unix-ms on wire; shells format at render time.
    val lastConnectedMs: Long = 0,
    val lastEventMs: Long = 0,
    val lastNotice: String? = null,
    val lastError: String? = null,
    val wireSubs: List<RelayDiagnosticsWireSub> = emptyList(),
    // ADR-0051 — the relay's NIP-11 information document. `null` until
    // `nmp-nip11` has fetched it (or the relay serves no document); the typed
    // wire carries this as an OPTIONAL child table (presence is the
    // discriminator — no `has_info` flag), and the JSON path as `info: null`.
    val info: RelayDiagnosticsInfo? = null,
    /** Raw kind numbers for discovery (NIP-65 etc). Shell formats for display. */
    val discoveryKinds: List<Long> = emptyList(),
) {
    // Shell-side computed display helpers

    /** URL without scheme and without trailing slash. */
    val shortUrl: String get() {
        var s = relayUrl
        for (prefix in listOf("wss://", "ws://")) {
            if (s.startsWith(prefix)) { s = s.removePrefix(prefix); break }
        }
        return s.trimEnd('/')
    }

    /** Title-cased role label (e.g. "content" → "Content"). */
    val roleLabel: String get() = role.replaceFirstChar { it.uppercase() }

    /** Title-cased connection label (e.g. "connected" → "Connected"). */
    val connectionLabel: String get() = connection.replaceFirstChar { it.uppercase() }

    /** Auth label: "—" passthrough; otherwise title-cased. */
    val authLabel: String get() = if (auth == "—") "—" else auth.replaceFirstChar { it.uppercase() }

    /** Shell-derived semantic hue (#1768) from the raw tokens above. */
    val roleTone: String get() = RelayDiagnosticsTone.role(role)
    val connectionTone: String get() = RelayDiagnosticsTone.connection(connection)
    val authTone: String get() = RelayDiagnosticsTone.auth(auth)

    /** Compact formatted total events received. */
    val totalEventsDisplay: String get() = compactCount(totalEventsRx)

    /** Formatted bytes received when > 0; null otherwise. */
    val bytesRxDisplay: String? get() = if (bytesRx > 0) formatBytes(bytesRx) else null

    /** Formatted bytes transmitted when > 0; null otherwise. */
    val bytesTxDisplay: String? get() = if (bytesTx > 0) formatBytes(bytesTx) else null
}

/**
 * ADR-0051 relay-information document (NIP-11), Android peer of iOS
 * `RelayDiagnosticsInfo`. Field-for-field mirror of the kernel projection
 * (`crates/nmp-core/src/kernel/relay_diagnostics.rs::RelayDiagnosticsInfo`).
 *
 * Every `Option<String>` collapses to `null` when absent (byte-faithful to the
 * typed wire's `has_*`-companion semantics and the JSON path's `null`). The
 * three `limitation` booleans are tri-state (`null` = the relay did not
 * advertise the limitation). `supportedNips` is a possibly-empty list. The
 * presentation layer renders these directly — no HTTP, no JSON, no NIP-11
 * awareness (thin-shell rule).
 */
@Serializable
data class RelayDiagnosticsInfo(
    val name: String? = null,
    val description: String? = null,
    val icon: String? = null,
    val pubkey: String? = null,
    val contact: String? = null,
    val software: String? = null,
    val version: String? = null,
    @SerialName("supported_nips") val supportedNips: List<Int> = emptyList(),
    @SerialName("payment_required") val paymentRequired: Boolean? = null,
    @SerialName("auth_required") val authRequired: Boolean? = null,
    @SerialName("restricted_writes") val restrictedWrites: Boolean? = null,
)

@Serializable
data class RelayDiagnosticsWireSub(
    val wireId: String = "",
    val relayUrl: String = "",
    val filterSummary: String = "",
    /** Raw state string from Rust (e.g. "open", "closed", "pending"). Shell formats. */
    val state: String = "",
    /** Raw consumer count. Shell derives display string. */
    val consumerCount: Int = 0,
    /** Raw events received counter. Shell derives display string. */
    val eventsRx: Long = 0,
    val eoseObserved: Boolean = false,
    // aim.md §62: raw Unix-ms on wire; shells format at render time.
    val openedMs: Long = 0,
    val lastEventMs: Long = 0,
    val eoseMs: Long = 0,
    val closeReason: String? = null,
) {
    // Shell-side computed display helpers

    /** Truncated wire ID for display (≤12 chars passthrough; longer → 8-char prefix + "…"). */
    val shortWireId: String get() = if (wireId.length <= 12) wireId else "${wireId.take(8)}…"

    /** Title-cased state label (e.g. "open" → "Open"). */
    val stateLabel: String get() = state.replaceFirstChar { it.uppercase() }

    /** Shell-derived semantic hue (#1768) from the raw `state` token. */
    val stateTone: String get() = RelayDiagnosticsTone.wireSubState(state)

    /** Human-readable consumer count (empty when 0). */
    val consumerCountLabel: String get() = when (consumerCount) {
        0 -> ""
        1 -> "1 consumer"
        else -> "$consumerCount consumers"
    }

    /** Compact event count when > 0; null when zero. */
    val eventsRxDisplay: String? get() = if (eventsRx > 0) compactCount(eventsRx) else null
}

@Serializable
data class RelayDiagnosticsInterest(
    val key: String = "",
    val state: String = "",
    val refcount: Int = 0,
    val cacheCoverage: String = "",
    val relayUrls: List<String> = emptyList(),
) {
    /** Shell-derived semantic hue (#1768) from the raw `state` token. */
    val stateTone: String get() = RelayDiagnosticsTone.interestState(state)
}

/**
 * Shell-side tone policy (#1768): derive a semantic hue token from the RAW
 * protocol tokens the `relay_diagnostics` projection now emits. The kernel
 * emits only raw `role` / `connection` / `auth` / `state` / reason `kind`
 * strings; deciding which hue class each belongs to is the app's job. Ported
 * verbatim from the former kernel `relay_diagnostics/format.rs` + `reasons.rs`
 * selectors. The presentation layer maps these tokens to a Color.
 */
internal object RelayDiagnosticsTone {
    fun role(role: String): String = if (role == "write") "write" else "accent"

    fun connection(connection: String): String {
        val lower = connection.lowercase()
        return when {
            lower == "connected" -> "ok"
            lower.startsWith("disconnect") || lower == "failed" -> "error"
            lower.contains("connect") -> "warn"
            lower == "unknown" || lower == "idle" || lower == "—" || lower == "blocked" -> "muted"
            else -> "error"
        }
    }

    fun auth(auth: String): String {
        val lower = auth.lowercase()
        return when {
            lower == "ok" || lower == "authenticated" -> "ok"
            lower == "pending" -> "warn"
            else -> "muted"
        }
    }

    fun wireSubState(state: String): String = when (state.lowercase()) {
        "open", "active", "live" -> "ok"
        "pending", "warming", "opening", "auth_paused" -> "warn"
        else -> "muted"
    }

    fun interestState(state: String): String = when (state) {
        "active", "warming", "tailing", "complete" -> "ok"
        "idle" -> "muted"
        else -> "warn"
    }

    fun reason(kind: String): String = when (kind) {
        "blocked" -> "muted"
        "nip65" -> "accent"
        "hint" -> "warn"
        else -> "ok"
    }
}

// ── Shell-side display helpers for relay diagnostics ─────────────────────────
// These are file-private so they can be used by RelayDiagnosticsRow and
// RelayDiagnosticsWireSub computed properties without polluting the public API.

/** Compact count: < 1 000 → raw number; ≥ 1 000 → "1.2K" etc. */
internal fun compactCount(n: Long): String {
    // Mirrors the former kernel `compact_count`: whole magnitudes drop the
    // decimal (`1K`, not `1.0K`) so the rendered text matches iOS / TUI.
    fun magnitude(v: Double, suffix: String): String =
        if (v % 1.0 == 0.0) "${v.toLong()}$suffix" else String.format("%.1f%s", v, suffix)
    val d = n.toDouble()
    return when {
        d < 1_000.0 -> "$n"
        d < 1_000_000.0 -> magnitude(d / 1_000.0, "K")
        d < 1_000_000_000.0 -> magnitude(d / 1_000_000.0, "M")
        else -> String.format("%.1fB", d / 1_000_000_000.0)
    }
}

/**
 * Human-readable byte count. Mirrors the former kernel `format_bytes` helper
 * exactly (1024-divisor magnitudes, `B` / `KB` / `MB` labels) so the Android
 * diagnostics text stays byte-identical to what the projection used to emit and
 * matches the iOS / TUI shells (cross-shell parity).
 */
internal fun formatBytes(bytes: Long): String {
    val kb = bytes / 1024.0
    return when {
        kb < 1.0 -> "$bytes B"
        kb < 1024.0 -> String.format("%.1f KB", kb)
        else -> String.format("%.1f MB", kb / 1024.0)
    }
}
