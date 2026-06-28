package org.nmp.android.ui.embed

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import org.nmp.android.model.EmbedEnvelopeEntry

/**
 * Per-embed claim callback. The presentation layer claims an `EventRef` URI when
 * it begins rendering and releases on `DisposableEffect.onDispose`; the kernel
 * resolves the event and ships its typed projection in the next `NEMB` sidecar.
 *
 * `LocalEventClaimer.current` is `null` outside a provider scope, so
 * [EmbeddedEvent] degrades to a static loading placeholder rather than
 * crashing — mirrors [org.nmp.android.ui.LocalProfileClaimer].
 */
typealias EventClaimer = (uri: String, consumerId: String, claim: Boolean) -> Unit

val LocalEventClaimer = compositionLocalOf<EventClaimer?> { null }

/**
 * Resolved embed envelopes from the kernel's `refs.event.envelopes` (`NEMB`)
 * sidecar, keyed by `primaryId` (event-id hex or `kind:pubkey:d` coord).
 * Provided once at each screen root and read by [EmbeddedEvent]. Defaults to an
 * empty map outside a provider scope (the loading placeholder then persists).
 */
val LocalRefEventEnvelopes = compositionLocalOf<Map<String, EmbedEnvelopeEntry>> { emptyMap() }

/**
 * What an [EmbeddedEvent] should display for a given resolved envelope. Pure
 * value (no Compose), so the loading/collapsed/resolved decision is
 * unit-testable on the JVM. Mirrors the iOS `EmbeddedEvent.content` switch.
 */
enum class EmbedRenderState { LOADING, COLLAPSED, RESOLVED }

/**
 * Decide the [EmbedRenderState] for a (possibly-absent) envelope:
 *  - `null` envelope (kernel hasn't resolved it yet) → [LOADING] (NOT a
 *    permanent placeholder — it resolves on the next snapshot tick).
 *  - `collapsed` (kernel hit its own depth/cycle guard) → [COLLAPSED].
 *  - missing projection on an otherwise-present envelope → [LOADING].
 *  - otherwise → [RESOLVED].
 */
fun embedRenderState(envelope: EmbedEnvelopeEntry?): EmbedRenderState = when {
    envelope == null -> EmbedRenderState.LOADING
    envelope.collapsed -> EmbedRenderState.COLLAPSED
    envelope.projection == null -> EmbedRenderState.LOADING
    else -> EmbedRenderState.RESOLVED
}

/**
 * Pure model of the claim → release lifecycle wired into [EmbeddedEvent]'s
 * `DisposableEffect`. Holds the verbatim `EventRef` [uri] (what the kernel
 * resolves) and the per-slot [consumerId] (the reference-count key). [enter]
 * claims; [dispose] releases. A null [claimer] (no provider scope) makes both a
 * no-op so the composable degrades to a static loading state. Factored out so
 * the claim contract is unit-testable on the JVM without a Compose runtime.
 */
class EventClaimHandle(
    private val claimer: EventClaimer?,
    private val uri: String,
    private val consumerId: String,
) {
    fun enter() {
        claimer?.invoke(uri, consumerId, true)
    }

    fun dispose() {
        claimer?.invoke(uri, consumerId, false)
    }
}

/**
 * Render one out-of-feed embedded Nostr event (#984 / F-CR-07). Android peer of
 * iOS `EmbeddedEvent.swift`.
 *
 * Lifecycle (D8 — no polling):
 *   • A [DisposableEffect] keyed on ([uri], [consumerId]) claims on enter and
 *     releases on dispose — the kernel reference-counts; Kotlin never counts.
 *   • The resolved [EmbedEnvelopeEntry] is read from [LocalRefEventEnvelopes]
 *     by [primaryId]; while absent it shows a loading placeholder (NOT a
 *     permanent "Event pending" text), which resolves to the typed render on
 *     the next snapshot tick.
 *   • Resolution dispatches through [NostrKindRegistry] to the per-kind view.
 *
 * THIN-SHELL (D0): zero protocol logic — only claim lifecycle, a map lookup,
 * collapse/loading state, and registry dispatch over an already-typed
 * projection.
 *
 * The caller (`EventRefBlock`) enforces the `MaxEmbedDepth` recursion guard
 * before instantiating this composable, so a self-referential embed chain
 * cannot recurse without bound; the kernel additionally caps depth via the
 * envelope's `collapsed` flag.
 */
@Composable
fun EmbeddedEvent(
    uri: String,
    primaryId: String,
    consumerId: String,
) {
    val claimer = LocalEventClaimer.current
    val claimKey = primaryId.ifEmpty { uri }
    DisposableEffect(claimKey, consumerId) {
        val handle = EventClaimHandle(claimer, uri, consumerId)
        handle.enter()
        onDispose { handle.dispose() }
    }

    val envelope = LocalRefEventEnvelopes.current[primaryId]
    Surface(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(8.dp),
        tonalElevation = 1.dp,
    ) {
        when (embedRenderState(envelope)) {
            EmbedRenderState.LOADING -> EmbedLoading(claimKey)
            EmbedRenderState.COLLAPSED -> EmbedCollapsed(envelope?.collapseReason)
            EmbedRenderState.RESOLVED -> EmbedResolved {
                NostrKindRegistry.Render(envelope!!.projection!!)
            }
        }
    }
}

@Composable
private fun EmbedResolved(content: @Composable () -> Unit) {
    androidx.compose.foundation.layout.Column(Modifier.padding(10.dp)) {
        content()
    }
}

@Composable
private fun EmbedCollapsed(reason: String?) {
    Text(
        "Embedded event ${reason ?: "collapsed"}",
        modifier = Modifier.fillMaxWidth().padding(10.dp),
        style = MaterialTheme.typography.labelMedium,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
    )
}

@Composable
private fun EmbedLoading(label: String) {
    androidx.compose.foundation.layout.Column(
        modifier = Modifier
            .fillMaxWidth()
            .border(1.dp, MaterialTheme.colorScheme.outline.copy(alpha = 0.4f), RoundedCornerShape(8.dp))
            .background(Color.Gray.copy(alpha = 0.06f), RoundedCornerShape(8.dp))
            .padding(10.dp),
    ) {
        Text(
            "Loading embedded event…",
            style = MaterialTheme.typography.labelMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Text(
            shortHex(label),
            style = MaterialTheme.typography.labelSmall,
            fontFamily = FontFamily.Monospace,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}
