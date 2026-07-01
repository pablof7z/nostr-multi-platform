// Requires: compose-ui, compose-foundation, compose-material3. Kotlin 1.9+.
// Depends on `compose/content-core` and `compose/content-kind-registry`.
//
// Compose composable that renders one embedded Nostr event by dispatching
// through [NostrKindRegistry]. Compose mirror of the SwiftUI `EmbeddedEvent`
// view and the TUI `EmbeddedEvent` widget.
//
// Lifecycle (D8 — no polling):
//   • A [DisposableEffect] keyed on (uri, consumerId) resolves an event ref on
//     enter and releases on dispose — the kernel reference-counts; Kotlin never
//     counts.
//   • The resolved [EmbeddedEventEnvelope] is read from [LocalResolvedEventEmbeds]
//     by `primaryId`; while absent it shows a loading placeholder (NOT a
//     permanent "Event pending" text), which resolves on the next snapshot tick.
//   • Resolution dispatches through the [NostrKindRegistry] to the per-kind
//     renderer, wrapped in [EmbedChromeContainer].
//
// THIN-SHELL (D0): zero protocol logic — only ref lifecycle, a map lookup,
// collapse/loading state, and registry dispatch over an already-typed
// projection.

package nmp.content

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp

/**
 * Per-embed event-ref resolver. The presentation layer resolves an `EventRef`
 * URI when it begins rendering and releases on dispose; the host adapter
 * decodes the URI at the app edge and calls the kernel's unified ref resolver.
 *
 * `LocalEventRefResolver.current` is `null` outside a provider scope, so
 * [EmbeddedEvent] degrades to a static loading placeholder rather than crashing.
 */
public interface EventRefResolver {
    public fun resolveEventRef(uri: String, consumerId: String)
    public fun releaseEventRef(uri: String, consumerId: String)
}

public val LocalEventRefResolver: androidx.compose.runtime.ProvidableCompositionLocal<EventRefResolver?> =
    compositionLocalOf { null }

/**
 * Resolved event-ref embed envelopes from Rust's derived
 * `refs.event.envelopes` sidecar, keyed by `primaryId` (event-id hex or
 * `kind:pubkey:d` coord). The sidecar is generated from authoritative
 * `refs.event` rows by `nmp_content::derive_ref_event_envelopes`; Compose only
 * mirrors it inside the app-level provider. Defaults to an empty map outside a
 * provider scope (the loading placeholder then persists).
 */
public val LocalResolvedEventEmbeds: androidx.compose.runtime.ProvidableCompositionLocal<Map<String, EmbeddedEventEnvelope>> =
    compositionLocalOf { emptyMap() }

/**
 * The kind registry consulted by [EmbeddedEvent]. Provided once at each screen
 * root; defaults to [NostrKindRegistry.makeDefault] so the composable renders
 * the built-in handlers even when the host forgets to bind a registry.
 */
public val LocalNostrKindRegistry: androidx.compose.runtime.ProvidableCompositionLocal<NostrKindRegistry> =
    compositionLocalOf { NostrKindRegistry.makeDefault() }

/** What an [EmbeddedEvent] should display for a given resolved envelope. */
public enum class EmbedRenderState { LOADING, COLLAPSED, RESOLVED }

/**
 * Decide the [EmbedRenderState] for a (possibly-absent) envelope. Pure value
 * (no Compose) so the loading/collapsed/resolved decision is unit-testable.
 */
public fun embedRenderState(envelope: EmbeddedEventEnvelope?): EmbedRenderState = when {
    envelope == null -> EmbedRenderState.LOADING
    envelope.collapsed -> EmbedRenderState.COLLAPSED
    envelope.projection == null -> EmbedRenderState.LOADING
    else -> EmbedRenderState.RESOLVED
}

@Composable
public fun EmbeddedEvent(
    uri: String,
    primaryId: String,
    consumerId: String = "nmp-gallery-android.embed",
) {
    val eventRefResolver = LocalEventRefResolver.current
    val lifecycleKey = primaryId.ifEmpty { uri }
    DisposableEffect(lifecycleKey, consumerId) {
        eventRefResolver?.resolveEventRef(uri, consumerId)
        onDispose { eventRefResolver?.releaseEventRef(uri, consumerId) }
    }

    val envelope = LocalResolvedEventEmbeds.current[primaryId]
        ?: LocalResolvedEventEmbeds.current[uri]
    val registry = LocalNostrKindRegistry.current

    EmbedChromeContainer(
        depth = envelope?.depth ?: 0,
        collapsed = envelope?.collapsed ?: false,
    ) {
        when (embedRenderState(envelope)) {
            EmbedRenderState.LOADING -> EmbedLoading(lifecycleKey)
            EmbedRenderState.COLLAPSED -> EmbedCollapsed(envelope?.collapseReason)
            EmbedRenderState.RESOLVED -> {
                val projection = envelope!!.projection!!
                registry.resolve(projection).Render(projection, registry)
            }
        }
    }
}

@Composable
private fun EmbedCollapsed(reason: String?) {
    Text(
        "embedded event ${reason ?: "collapsed"}",
        modifier = Modifier.fillMaxWidth(),
        style = MaterialTheme.typography.labelMedium,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
    )
}

@Composable
private fun EmbedLoading(label: String) {
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .border(1.dp, MaterialTheme.colorScheme.outline.copy(alpha = 0.4f), RoundedCornerShape(8.dp))
            .background(Color.Gray.copy(alpha = 0.06f), RoundedCornerShape(8.dp))
            .padding(10.dp),
    ) {
        Text(
            "loading embedded event…",
            style = MaterialTheme.typography.labelMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Text(
            shortUri(label),
            style = MaterialTheme.typography.labelSmall,
            fontFamily = FontFamily.Monospace,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

private fun shortUri(value: String): String {
    if (value.length <= 24) return value
    return "${value.take(14)}…${value.takeLast(8)}"
}
